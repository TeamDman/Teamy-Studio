use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, mpsc};

use eyre::Context;
use libghostty_vt::key;
use libghostty_vt::render::{CellIterator, CursorViewport, RenderState, RowIterator};
use libghostty_vt::style::RgbColor;
use libghostty_vt::{Terminal, TerminalOptions};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tracing::{debug, info};
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, HDC, SetBkMode, SetTextColor, TRANSPARENT, TextOutW,
};
use windows::Win32::System::Console::{
    CAPSLOCK_ON, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, NUMLOCK_ON, RIGHT_ALT_PRESSED,
    RIGHT_CTRL_PRESSED, SHIFT_PRESSED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;

use crate::paths::AppHome;

pub const DRAG_STRIP_HEIGHT: i32 = 38;
pub const WINDOW_PADDING: i32 = 12;
pub const TOP_PADDING: i32 = WINDOW_PADDING + DRAG_STRIP_HEIGHT;
pub const POLL_TIMER_ID: usize = 1;
pub const POLL_INTERVAL_MS: u32 = 16;
pub const WINDOW_ALPHA: u8 = 208;
pub const WINDOW_BACKGROUND: COLORREF = COLORREF(0x0068_1F4A);
pub const WINDOW_ACCENT: COLORREF = COLORREF(0x008F_4D78);
pub const WINDOW_TEXT: COLORREF = COLORREF(0x00F5_EBFF);

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const MAX_SCROLLBACK: usize = 20_000;
const WIN32_INPUT_MODE_ENABLE: &[u8] = b"\x1b[?9001h";
const WIN32_INPUT_MODE_DISABLE: &[u8] = b"\x1b[?9001l";

type PtyWriter = Box<dyn Write + Send>;

#[derive(Clone, Copy, Debug)]
struct SuppressedChar {
    primary: u32,
    alternate: Option<u32>,
}

impl SuppressedChar {
    fn single(primary: u32) -> Self {
        Self {
            primary,
            alternate: None,
        }
    }

    fn with_alternate(primary: u32, alternate: u32) -> Self {
        Self {
            primary,
            alternate: Some(alternate),
        }
    }

    fn matches(self, code_unit: u32) -> bool {
        self.primary == code_unit || self.alternate == Some(code_unit)
    }
}

pub struct PumpResult {
    pub should_close: bool,
    pub needs_repaint: bool,
}

#[derive(Clone, Copy)]
struct PendingWin32CharKey {
    vkey: u32,
    lparam: isize,
    mapped_key: key::Key,
    unshifted_codepoint: char,
    mods: key::Mods,
}

pub struct TerminalSession {
    terminal: Terminal<'static, 'static>,
    render_state: RenderState<'static>,
    key_encoder: key::Encoder<'static>,
    key_event: key::Event<'static>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send>,
    writer: Arc<Mutex<PtyWriter>>,
    reader: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    cols: u16,
    rows: u16,
    needs_repaint: bool,
    full_repaint_pending: bool,
    input_trace: Vec<Vec<u8>>,
    suppressed_chars: VecDeque<SuppressedChar>,
    win32_input_mode: bool,
    win32_input_mode_buffer: Vec<u8>,
    pending_win32_char_key: Option<PendingWin32CharKey>,
    closed: bool,
}

#[derive(Clone, Copy)]
pub struct TerminalLayout {
    pub client_width: i32,
    pub client_height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
}

impl TerminalLayout {
    #[must_use]
    pub fn terminal_rect(self) -> RECT {
        RECT {
            left: WINDOW_PADDING,
            top: TOP_PADDING,
            right: (self.client_width - WINDOW_PADDING).max(WINDOW_PADDING),
            bottom: (self.client_height - WINDOW_PADDING).max(TOP_PADDING),
        }
    }

    #[must_use]
    pub fn grid_size(self) -> (u16, u16) {
        let rect = self.terminal_rect();
        let width = (rect.right - rect.left).max(self.cell_width.max(1));
        let height = (rect.bottom - rect.top).max(self.cell_height.max(1));
        let cols = (width / self.cell_width.max(1)).max(1);
        let rows = (height / self.cell_height.max(1)).max(1);
        (
            u16::try_from(cols).unwrap_or(u16::MAX),
            u16::try_from(rows).unwrap_or(u16::MAX),
        )
    }
}

impl TerminalSession {
    /// cli[impl window.appearance.shell]
    /// cli[impl window.appearance.shell-configured-default]
    pub fn new(app_home: &AppHome) -> eyre::Result<Self> {
        Self::new_with_command(crate::shell_default::load_effective_command_builder(
            app_home,
        )?)
    }

    pub fn new_with_command(shell: CommandBuilder) -> eyre::Result<Self> {
        let pty_system = native_pty_system();
        let initial_size = PtySize {
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system
            .openpty(initial_size)
            .map_err(|error| eyre::eyre!("failed to open pseudoterminal: {error}"))?;

        let writer: Arc<Mutex<PtyWriter>> =
            Arc::new(Mutex::new(pair.master.take_writer().map_err(|error| {
                eyre::eyre!("failed to open PTY writer: {error}")
            })?));
        let writer_for_effect = Arc::clone(&writer);

        let mut terminal = Terminal::new(TerminalOptions {
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            max_scrollback: MAX_SCROLLBACK,
        })
        .wrap_err("failed to create libghostty terminal")?;
        terminal
            .on_pty_write(move |_terminal, data| {
                if let Ok(mut writer) = writer_for_effect.lock() {
                    let _ = writer.write_all(data);
                    let _ = writer.flush();
                }
            })
            .wrap_err("failed to register PTY write effect")?;

        info!(
            program = shell.get_argv().first().map_or_else(
                || "<unknown>".to_owned(),
                |arg| arg.to_string_lossy().into_owned()
            ),
            "starting Teamy Studio PTY child"
        );
        let child = pair
            .slave
            .spawn_command(shell)
            .map_err(|error| eyre::eyre!("failed to spawn shell inside PTY: {error}"))?;
        drop(pair.slave);

        let mut cloned_reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| eyre::eyre!("failed to clone PTY reader: {error}"))?;
        let (reader_tx, reader_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match cloned_reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(bytes_read) => {
                        if reader_tx.send(Ok(buffer[..bytes_read].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = reader_tx.send(Err(error));
                        break;
                    }
                }
            }
        });

        Ok(Self {
            terminal,
            render_state: RenderState::new().wrap_err("failed to create render state")?,
            key_encoder: key::Encoder::new().wrap_err("failed to create key encoder")?,
            key_event: key::Event::new().wrap_err("failed to create key event")?,
            master: pair.master,
            child,
            writer,
            reader: reader_rx,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            needs_repaint: true,
            full_repaint_pending: true,
            input_trace: Vec::new(),
            suppressed_chars: VecDeque::new(),
            win32_input_mode: false,
            win32_input_mode_buffer: Vec::new(),
            pending_win32_char_key: None,
            closed: false,
        })
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn resize(&mut self, layout: TerminalLayout) -> eyre::Result<()> {
        let (cols, rows) = layout.grid_size();
        if cols == self.cols && rows == self.rows {
            return Ok(());
        }

        debug!(cols, rows, "resizing terminal grid");
        self.terminal
            .resize(
                cols,
                rows,
                u32::try_from(layout.cell_width.max(1)).unwrap_or(1),
                u32::try_from(layout.cell_height.max(1)).unwrap_or(1),
            )
            .wrap_err("failed to resize libghostty terminal")?;
        self.master
            .resize(PtySize {
                cols,
                rows,
                pixel_width: u16::try_from(layout.client_width.max(0)).unwrap_or(u16::MAX),
                pixel_height: u16::try_from(layout.client_height.max(0)).unwrap_or(u16::MAX),
            })
            .map_err(|error| eyre::eyre!("failed to resize PTY: {error}"))?;

        self.cols = cols;
        self.rows = rows;
        self.needs_repaint = true;
        self.full_repaint_pending = true;
        Ok(())
    }

    pub fn pump(&mut self) -> eyre::Result<PumpResult> {
        let mut changed = false;

        while let Ok(message) = self.reader.try_recv() {
            match message {
                Ok(bytes) => {
                    let bytes = normalize_cursor_visibility_mode_sequence(&bytes);
                    let bytes = self.strip_win32_input_mode_sequence(bytes.as_ref());
                    self.terminal.vt_write(bytes.as_ref());
                    changed = true;
                }
                Err(error) => {
                    self.terminal
                        .vt_write(format!("\r\n[pty read error: {error}]\r\n").as_bytes());
                    changed = true;
                    self.closed = true;
                }
            }
        }

        if self
            .child
            .try_wait()
            .wrap_err("failed to query shell status")?
            .is_some()
        {
            self.closed = true;
        }

        self.needs_repaint |= changed;
        Ok(PumpResult {
            should_close: self.closed,
            needs_repaint: self.needs_repaint,
        })
    }

    pub fn handle_char(&mut self, code_unit: u32, lparam: isize) -> eyre::Result<bool> {
        debug!(
            code_unit,
            lparam,
            win32_input_mode = self.win32_input_mode,
            suppressed_front = ?self.suppressed_chars.front().copied(),
            "handling character input"
        );
        if self.should_route_text_through_key_events()? {
            return Ok(false);
        }

        if !self.win32_input_mode
            && self
                .suppressed_chars
                .front()
                .copied()
                .is_some_and(|suppressed| suppressed.matches(code_unit))
        {
            self.suppressed_chars.pop_front();
            debug!(
                code_unit,
                "suppressed legacy WM_CHAR after handled key event"
            );
            return Ok(true);
        }

        let Some(character) = char::from_u32(code_unit) else {
            return Ok(false);
        };

        if self.win32_input_mode {
            let Some(pending_key) = self.pending_win32_char_key else {
                return Ok(false);
            };

            self.write_win32_input_mode_char_event(pending_key, character, lparam)?;
            self.needs_repaint = true;
            return Ok(true);
        }

        if character == '\r' || character == '\t' || character == '\u{8}' {
            return Ok(false);
        }

        if character < ' ' {
            let control = u8::try_from(u32::from(character)).unwrap_or_default();
            self.write_input(&[control])?;
            self.needs_repaint = true;
            return Ok(true);
        }

        let mut bytes = [0_u8; 4];
        let encoded = character.encode_utf8(&mut bytes);
        self.write_input(encoded.as_bytes())?;
        self.needs_repaint = true;
        Ok(true)
    }

    pub fn handle_key_event(
        &mut self,
        vkey: u32,
        lparam: isize,
        was_down: bool,
        is_release: bool,
        mods: key::Mods,
    ) -> eyre::Result<bool> {
        let Some((mapped_key, unshifted_codepoint)) = map_virtual_key(vkey, lparam) else {
            return Ok(false);
        };

        debug!(
            vkey,
            lparam,
            ?mapped_key,
            unshifted_codepoint = u32::from(unshifted_codepoint),
            ?mods,
            was_down,
            is_release,
            win32_input_mode = self.win32_input_mode,
            "handling key event"
        );

        if is_release && !self.win32_input_mode && !self.should_report_key_releases()? {
            return Ok(false);
        }

        let kitty_flags = self.current_kitty_keyboard_flags()?;
        if self.win32_input_mode {
            if is_release {
                self.write_win32_input_mode_key_event(
                    vkey,
                    lparam,
                    mapped_key,
                    unshifted_codepoint,
                    mods,
                    '\0',
                    1,
                    false,
                )?;
                if self.pending_win32_char_key.map(|pending| pending.vkey) == Some(vkey) {
                    self.pending_win32_char_key = None;
                }
                self.needs_repaint = true;
                return Ok(true);
            }

            if should_route_key_through_char_input(mapped_key, unshifted_codepoint, true) {
                self.pending_win32_char_key = Some(PendingWin32CharKey {
                    vkey,
                    lparam,
                    mapped_key,
                    unshifted_codepoint,
                    mods,
                });
                return Ok(false);
            }

            self.write_win32_input_mode_key_event(
                vkey,
                lparam,
                mapped_key,
                unshifted_codepoint,
                mods,
                legacy_key_event_character(mapped_key, unshifted_codepoint, mods).unwrap_or('\0'),
                repeat_count(lparam),
                true,
            )?;
            self.needs_repaint = true;
            return Ok(true);
        }

        if kitty_flags.is_empty() {
            if mapped_key == key::Key::Backspace {
                if is_release {
                    debug!(vkey, "ignored legacy Backspace key release");
                    return Ok(false);
                }

                self.suppressed_chars
                    .push_back(SuppressedChar::with_alternate(u32::from('\u{8}'), 0x7F));
                debug!(
                    vkey,
                    ?mods,
                    suppressed_len = self.suppressed_chars.len(),
                    "writing legacy Backspace byte and suppressing matching WM_CHAR"
                );
                self.write_input(&[0x7F])?;
                self.needs_repaint = true;
                return Ok(true);
            }

            if should_route_key_through_char_input(mapped_key, unshifted_codepoint, false) {
                return Ok(false);
            }

            self.write_input(&legacy_special_key_bytes(mapped_key, mods).unwrap_or_default())?;
            self.needs_repaint = true;
            return Ok(!legacy_special_key_bytes(mapped_key, mods)
                .unwrap_or_default()
                .is_empty());
        }

        let action = if is_release {
            key::Action::Release
        } else if was_down {
            key::Action::Repeat
        } else {
            key::Action::Press
        };
        let mut response = Vec::with_capacity(16);
        let mut consumed_mods = key::Mods::empty();
        if unshifted_codepoint != '\0' && mods.contains(key::Mods::SHIFT) {
            consumed_mods |= key::Mods::SHIFT;
        }

        self.key_event
            .set_action(action)
            .set_key(mapped_key)
            .set_mods(mods)
            .set_consumed_mods(consumed_mods)
            .set_unshifted_codepoint(unshifted_codepoint)
            .set_utf8::<String>(None);

        self.key_encoder
            .set_options_from_terminal(&self.terminal)
            .encode_to_vec(&self.key_event, &mut response)
            .wrap_err("failed to encode special key event")?;

        if response.is_empty() {
            return Ok(false);
        }

        self.write_input(&response)?;
        self.needs_repaint = true;
        Ok(true)
    }

    fn should_report_key_releases(&self) -> eyre::Result<bool> {
        let flags = self.current_kitty_keyboard_flags()?;
        Ok(flags.contains(key::KittyKeyFlags::REPORT_EVENTS))
    }

    fn should_route_text_through_key_events(&self) -> eyre::Result<bool> {
        let flags = self.current_kitty_keyboard_flags()?;
        Ok(flags.intersects(
            key::KittyKeyFlags::REPORT_ALL
                | key::KittyKeyFlags::REPORT_ASSOCIATED
                | key::KittyKeyFlags::REPORT_EVENTS,
        ))
    }

    pub fn current_kitty_keyboard_flags(&self) -> eyre::Result<key::KittyKeyFlags> {
        self.terminal
            .kitty_keyboard_flags()
            .wrap_err("failed to query kitty keyboard flags")
    }

    pub fn win32_input_mode_enabled(&self) -> bool {
        self.win32_input_mode
    }

    pub fn visible_text(&mut self) -> eyre::Result<String> {
        let snapshot = self
            .render_state
            .update(&self.terminal)
            .wrap_err("failed to update terminal render state")?;
        let mut rows = RowIterator::new().wrap_err("failed to create row iterator")?;
        let mut cells = CellIterator::new().wrap_err("failed to create cell iterator")?;
        let mut lines = Vec::new();

        let mut row_iter = rows
            .update(&snapshot)
            .wrap_err("failed to update row iterator")?;
        while let Some(row) = row_iter.next() {
            let mut line = String::new();
            let mut cell_iter = cells
                .update(row)
                .wrap_err("failed to update cell iterator")?;
            while let Some(cell) = cell_iter.next() {
                let graphemes = cell.graphemes().wrap_err("failed to read cell text")?;
                if graphemes.is_empty() {
                    line.push(' ');
                } else {
                    for grapheme in graphemes {
                        line.push(grapheme);
                    }
                }
            }
            lines.push(line.trim_end_matches(' ').to_owned());
        }

        Ok(lines.join("\n"))
    }

    pub fn paint(
        &mut self,
        hdc: HDC,
        layout: TerminalLayout,
        overlay_text: &str,
    ) -> eyre::Result<()> {
        let snapshot = self
            .render_state
            .update(&self.terminal)
            .wrap_err("failed to update terminal render state")?;
        let colors = snapshot
            .colors()
            .wrap_err("failed to fetch terminal colors")?;
        let mut rows = RowIterator::new().wrap_err("failed to create row iterator")?;
        let mut cells = CellIterator::new().wrap_err("failed to create cell iterator")?;
        let rect = layout.terminal_rect();

        if self.full_repaint_pending {
            paint_terminal_background(hdc, layout.client_width, layout.client_height)?;
        }
        paint_drag_strip(hdc, layout.client_width, overlay_text)?;

        if self.full_repaint_pending {
            paint_rect_background(hdc, rect, colors.background)
                .wrap_err("failed to paint terminal background")?;
        }

        let mut row_index = 0_i32;
        let mut row_iter = rows
            .update(&snapshot)
            .wrap_err("failed to update row iterator")?;
        while let Some(row) = row_iter.next() {
            let row_is_dirty = row.dirty().wrap_err("failed to read row dirty flag")?;
            let should_paint_row = self.full_repaint_pending || row_is_dirty;
            let y = rect.top + (row_index * layout.cell_height);
            let row_rect = RECT {
                left: rect.left,
                top: y,
                right: rect.right,
                bottom: y + layout.cell_height,
            };

            if should_paint_row {
                paint_rect_background(hdc, row_rect, colors.background)
                    .wrap_err("failed to paint row background")?;
            }

            let mut column_index = 0_i32;
            let mut cell_iter = cells
                .update(row)
                .wrap_err("failed to update cell iterator")?;

            while let Some(cell) = cell_iter.next() {
                if !should_paint_row {
                    column_index += 1;
                    continue;
                }

                let x = rect.left + (column_index * layout.cell_width);
                let cell_rect = RECT {
                    left: x,
                    top: y,
                    right: x + layout.cell_width,
                    bottom: y + layout.cell_height,
                };

                let background = cell.bg_color().wrap_err("failed to read cell background")?;
                if let Some(background) = background.filter(|color| *color != colors.background) {
                    let brush = unsafe { CreateSolidBrush(rgb_to_colorref(background)) };
                    if brush.0.is_null() {
                        eyre::bail!("failed to create cell background brush");
                    }
                    let _ = unsafe { FillRect(hdc, &cell_rect, brush) };
                    let _ = unsafe { DeleteObject(brush.into()) };
                }

                let graphemes = cell.graphemes().wrap_err("failed to read cell text")?;
                if !graphemes.is_empty() {
                    let foreground = cell
                        .fg_color()
                        .wrap_err("failed to read cell foreground")?
                        .unwrap_or(colors.foreground);
                    let text: String = graphemes.into_iter().collect();
                    let utf16 = text.encode_utf16().collect::<Vec<u16>>();
                    let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
                    let _ = unsafe { SetTextColor(hdc, rgb_to_colorref(foreground)) };
                    let _ = unsafe { TextOutW(hdc, x, y, &utf16) };
                }

                column_index += 1;
            }

            if should_paint_row {
                row.set_dirty(false)
                    .wrap_err("failed to clear row dirty flag after paint")?;
            }
            row_index += 1;
        }

        snapshot
            .set_dirty(libghostty_vt::render::Dirty::Clean)
            .wrap_err("failed to clear render dirty state")?;

        if snapshot.cursor_visible().unwrap_or(false) {
            if let Some(cursor) = snapshot
                .cursor_viewport()
                .wrap_err("failed to fetch cursor")?
            {
                paint_cursor(
                    hdc,
                    layout,
                    rect,
                    cursor,
                    colors.cursor.unwrap_or(colors.foreground),
                )?;
            }
        }

        self.needs_repaint = false;
        self.full_repaint_pending = false;
        Ok(())
    }

    #[must_use]
    pub fn take_input_trace(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.input_trace)
    }

    fn write_input(&mut self, data: &[u8]) -> eyre::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| eyre::eyre!("PTY writer mutex was poisoned"))?;
        writer
            .write_all(data)
            .wrap_err("failed to write input to PTY")?;
        writer.flush().wrap_err("failed to flush PTY input")?;
        self.input_trace.push(data.to_vec());
        Ok(())
    }

    fn write_win32_input_mode_key_event(
        &mut self,
        vkey: u32,
        lparam: isize,
        mapped_key: key::Key,
        _unshifted_codepoint: char,
        mods: key::Mods,
        unicode_char: char,
        repeat_count: u16,
        key_down: bool,
    ) -> eyre::Result<()> {
        let scancode = ((lparam >> 16) & 0xFF) as u32;
        let sequence = format!(
            "\x1b[{vkey};{scancode};{};{};{};{}_",
            u32::from(unicode_char),
            u8::from(key_down),
            control_key_state(mods),
            repeat_count.max(1),
        );

        if let Some(character) = legacy_char_suppression(mapped_key, unicode_char) {
            if key_down && !self.win32_input_mode {
                self.suppressed_chars
                    .push_back(SuppressedChar::single(character as u32));
            }
        }

        self.write_input(sequence.as_bytes())
    }

    fn write_win32_input_mode_char_event(
        &mut self,
        pending_key: PendingWin32CharKey,
        character: char,
        lparam: isize,
    ) -> eyre::Result<()> {
        self.write_win32_input_mode_key_event(
            pending_key.vkey,
            pending_key.lparam,
            pending_key.mapped_key,
            pending_key.unshifted_codepoint,
            pending_key.mods,
            character,
            repeat_count(lparam),
            true,
        )
    }

    fn strip_win32_input_mode_sequence<'a>(&mut self, data: &'a [u8]) -> Cow<'a, [u8]> {
        let mut combined = std::mem::take(&mut self.win32_input_mode_buffer);
        combined.extend_from_slice(data);
        let mut output = Vec::with_capacity(combined.len());
        let mut index = 0;

        while index < combined.len() {
            if combined[index..].starts_with(WIN32_INPUT_MODE_ENABLE) {
                self.win32_input_mode = true;
                self.pending_win32_char_key = None;
                index += WIN32_INPUT_MODE_ENABLE.len();
                continue;
            }

            if combined[index..].starts_with(WIN32_INPUT_MODE_DISABLE) {
                self.win32_input_mode = false;
                self.pending_win32_char_key = None;
                index += WIN32_INPUT_MODE_DISABLE.len();
                continue;
            }

            if WIN32_INPUT_MODE_ENABLE.starts_with(&combined[index..])
                || WIN32_INPUT_MODE_DISABLE.starts_with(&combined[index..])
            {
                self.win32_input_mode_buffer
                    .extend_from_slice(&combined[index..]);
                break;
            }

            output.push(combined[index]);
            index += 1;
        }

        if output.len() == data.len() && self.win32_input_mode_buffer.is_empty() {
            return Cow::Borrowed(data);
        }

        Cow::Owned(output)
    }
}

fn map_virtual_key(vkey: u32, lparam: isize) -> Option<(key::Key, char)> {
    let extended = ((lparam >> 24) & 1) != 0;
    let scancode = ((lparam >> 16) & 0xFF) as u8;

    match vkey {
        0x20 => Some((key::Key::Space, ' ')),
        0x30 => Some((key::Key::Digit0, '0')),
        0x31 => Some((key::Key::Digit1, '1')),
        0x32 => Some((key::Key::Digit2, '2')),
        0x33 => Some((key::Key::Digit3, '3')),
        0x34 => Some((key::Key::Digit4, '4')),
        0x35 => Some((key::Key::Digit5, '5')),
        0x36 => Some((key::Key::Digit6, '6')),
        0x37 => Some((key::Key::Digit7, '7')),
        0x38 => Some((key::Key::Digit8, '8')),
        0x39 => Some((key::Key::Digit9, '9')),
        0x41 => Some((key::Key::A, 'a')),
        0x42 => Some((key::Key::B, 'b')),
        0x43 => Some((key::Key::C, 'c')),
        0x44 => Some((key::Key::D, 'd')),
        0x45 => Some((key::Key::E, 'e')),
        0x46 => Some((key::Key::F, 'f')),
        0x47 => Some((key::Key::G, 'g')),
        0x48 => Some((key::Key::H, 'h')),
        0x49 => Some((key::Key::I, 'i')),
        0x4A => Some((key::Key::J, 'j')),
        0x4B => Some((key::Key::K, 'k')),
        0x4C => Some((key::Key::L, 'l')),
        0x4D => Some((key::Key::M, 'm')),
        0x4E => Some((key::Key::N, 'n')),
        0x4F => Some((key::Key::O, 'o')),
        0x50 => Some((key::Key::P, 'p')),
        0x51 => Some((key::Key::Q, 'q')),
        0x52 => Some((key::Key::R, 'r')),
        0x53 => Some((key::Key::S, 's')),
        0x54 => Some((key::Key::T, 't')),
        0x55 => Some((key::Key::U, 'u')),
        0x56 => Some((key::Key::V, 'v')),
        0x57 => Some((key::Key::W, 'w')),
        0x58 => Some((key::Key::X, 'x')),
        0x59 => Some((key::Key::Y, 'y')),
        0x5A => Some((key::Key::Z, 'z')),
        0xBA => Some((key::Key::Semicolon, ';')),
        0xBB => Some((key::Key::Equal, '=')),
        0xBC => Some((key::Key::Comma, ',')),
        0xBD => Some((key::Key::Minus, '-')),
        0xBE => Some((key::Key::Period, '.')),
        0xBF => Some((key::Key::Slash, '/')),
        0xC0 => Some((key::Key::Backquote, '`')),
        0xDB => Some((key::Key::BracketLeft, '[')),
        0xDC => Some((key::Key::Backslash, '\\')),
        0xDD => Some((key::Key::BracketRight, ']')),
        0xDE => Some((key::Key::Quote, '\'')),
        0x10 => Some((
            if scancode == 0x36 {
                key::Key::ShiftRight
            } else {
                key::Key::ShiftLeft
            },
            '\0',
        )),
        0x11 => Some((
            if extended {
                key::Key::ControlRight
            } else {
                key::Key::ControlLeft
            },
            '\0',
        )),
        0x12 => Some((
            if extended {
                key::Key::AltRight
            } else {
                key::Key::AltLeft
            },
            '\0',
        )),
        0x08 => Some((key::Key::Backspace, '\0')),
        0x09 => Some((key::Key::Tab, '\0')),
        0x0D => Some((key::Key::Enter, '\0')),
        0x1B => Some((key::Key::Escape, '\0')),
        0x21 => Some((key::Key::PageUp, '\0')),
        0x22 => Some((key::Key::PageDown, '\0')),
        0x23 => Some((key::Key::End, '\0')),
        0x24 => Some((key::Key::Home, '\0')),
        0x25 => Some((key::Key::ArrowLeft, '\0')),
        0x26 => Some((key::Key::ArrowUp, '\0')),
        0x27 => Some((key::Key::ArrowRight, '\0')),
        0x28 => Some((key::Key::ArrowDown, '\0')),
        0x2D => Some((key::Key::Insert, '\0')),
        0x2E => Some((key::Key::Delete, '\0')),
        _ => None,
    }
}

fn should_route_key_through_char_input(
    mapped_key: key::Key,
    unshifted_codepoint: char,
    include_control_keys: bool,
) -> bool {
    unshifted_codepoint != '\0'
        || (include_control_keys
            && matches!(
                mapped_key,
                key::Key::Backspace | key::Key::Tab | key::Key::Enter
            ))
}

fn repeat_count(lparam: isize) -> u16 {
    u16::try_from((lparam as u64 & 0xFFFF) as u32)
        .unwrap_or(1)
        .max(1)
}

pub fn keyboard_mods(vkey: u32, lparam: isize, is_release: bool) -> key::Mods {
    let mut mods = key::Mods::empty();

    if key_is_pressed(0x10) {
        mods |= key::Mods::SHIFT;
    }
    if key_is_pressed(0x11) {
        mods |= key::Mods::CTRL;
    }
    if key_is_pressed(0x12) {
        mods |= key::Mods::ALT;
    }
    if key_is_pressed(0x5B) || key_is_pressed(0x5C) {
        mods |= key::Mods::SUPER;
    }
    if key_is_toggled(0x14) {
        mods |= key::Mods::CAPS_LOCK;
    }
    if key_is_toggled(0x90) {
        mods |= key::Mods::NUM_LOCK;
    }

    if key_is_pressed(0xA1) {
        mods |= key::Mods::SHIFT | key::Mods::SHIFT_SIDE;
    }
    if key_is_pressed(0xA3) {
        mods |= key::Mods::CTRL | key::Mods::CTRL_SIDE;
    }
    if key_is_pressed(0xA5) {
        mods |= key::Mods::ALT | key::Mods::ALT_SIDE;
    }
    if key_is_pressed(0x5C) {
        mods |= key::Mods::SUPER | key::Mods::SUPER_SIDE;
    }

    if is_release {
        let extended = ((lparam >> 24) & 1) != 0;
        let scancode = ((lparam >> 16) & 0xFF) as u8;
        match vkey {
            0x10 => {
                mods |= key::Mods::SHIFT;
                if scancode == 0x36 {
                    mods |= key::Mods::SHIFT_SIDE;
                }
            }
            0x11 => {
                mods |= key::Mods::CTRL;
                if extended {
                    mods |= key::Mods::CTRL_SIDE;
                }
            }
            0x12 => {
                mods |= key::Mods::ALT;
                if extended {
                    mods |= key::Mods::ALT_SIDE;
                }
            }
            0x5B => {
                mods |= key::Mods::SUPER;
            }
            0x5C => {
                mods |= key::Mods::SUPER | key::Mods::SUPER_SIDE;
            }
            _ => {}
        }
    }

    mods
}

fn key_is_pressed(vkey: i32) -> bool {
    ((unsafe { GetKeyState(vkey) } as u16) & 0x8000) != 0
}

fn key_is_toggled(vkey: i32) -> bool {
    ((unsafe { GetKeyState(vkey) } as u16) & 0x0001) != 0
}

fn control_key_state(mods: key::Mods) -> u32 {
    let mut state = 0;

    if mods.contains(key::Mods::SHIFT) {
        state |= SHIFT_PRESSED;
    }
    if mods.contains(key::Mods::CTRL) {
        state |= if mods.contains(key::Mods::CTRL_SIDE) {
            RIGHT_CTRL_PRESSED
        } else {
            LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED
        };
    }
    if mods.contains(key::Mods::ALT) {
        state |= if mods.contains(key::Mods::ALT_SIDE) {
            RIGHT_ALT_PRESSED
        } else {
            LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED
        };
    }
    if mods.contains(key::Mods::CAPS_LOCK) {
        state |= CAPSLOCK_ON;
    }
    if mods.contains(key::Mods::NUM_LOCK) {
        state |= NUMLOCK_ON;
    }

    state
}

fn legacy_char_suppression(mapped_key: key::Key, unicode_char: char) -> Option<char> {
    match mapped_key {
        key::Key::Backspace | key::Key::Tab | key::Key::Enter => Some(unicode_char),
        _ if unicode_char != '\0' => Some(unicode_char),
        _ => None,
    }
}

fn legacy_key_event_character(
    mapped_key: key::Key,
    unshifted_codepoint: char,
    mods: key::Mods,
) -> Option<char> {
    match mapped_key {
        key::Key::Backspace => Some('\u{8}'),
        key::Key::Tab => Some('\t'),
        key::Key::Enter => Some('\r'),
        key::Key::Space => Some(' '),
        _ if unshifted_codepoint == '\0' => None,
        key::Key::A
        | key::Key::B
        | key::Key::C
        | key::Key::D
        | key::Key::E
        | key::Key::F
        | key::Key::G
        | key::Key::H
        | key::Key::I
        | key::Key::J
        | key::Key::K
        | key::Key::L
        | key::Key::M
        | key::Key::N
        | key::Key::O
        | key::Key::P
        | key::Key::Q
        | key::Key::R
        | key::Key::S
        | key::Key::T
        | key::Key::U
        | key::Key::V
        | key::Key::W
        | key::Key::X
        | key::Key::Y
        | key::Key::Z => {
            let shifted = mods.contains(key::Mods::SHIFT) ^ mods.contains(key::Mods::CAPS_LOCK);
            Some(if shifted {
                unshifted_codepoint.to_ascii_uppercase()
            } else {
                unshifted_codepoint
            })
        }
        _ => Some(apply_shift_to_punctuation(
            unshifted_codepoint,
            mods.contains(key::Mods::SHIFT),
        )),
    }
}

fn apply_shift_to_punctuation(character: char, shifted: bool) -> char {
    if !shifted {
        return character;
    }

    match character {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        ';' => ':',
        '=' => '+',
        ',' => '<',
        '-' => '_',
        '.' => '>',
        '/' => '?',
        '`' => '~',
        '[' => '{',
        '\\' => '|',
        ']' => '}',
        '\'' => '"',
        _ => character,
    }
}

fn normalize_cursor_visibility_mode_sequence(data: &[u8]) -> Cow<'_, [u8]> {
    let mut rewritten: Option<Vec<u8>> = None;
    let mut index = 0;

    while index + 5 <= data.len() {
        let matches_cursor_mode = data[index] == 0x1B
            && data[index + 1] == b'['
            && data[index + 2] == b'2'
            && data[index + 3] == b'5'
            && matches!(data[index + 4], b'h' | b'l');

        if matches_cursor_mode {
            let output = rewritten.get_or_insert_with(|| {
                let mut output = Vec::with_capacity(data.len() + 4);
                output.extend_from_slice(&data[..index]);
                output
            });
            output.extend_from_slice(b"\x1B[?25");
            output.push(data[index + 4]);
            index += 5;
            continue;
        }

        if let Some(output) = rewritten.as_mut() {
            output.push(data[index]);
        }
        index += 1;
    }

    if let Some(mut output) = rewritten {
        output.extend_from_slice(&data[index..]);
        Cow::Owned(output)
    } else {
        Cow::Borrowed(data)
    }
}

fn rgb_to_colorref(color: RgbColor) -> COLORREF {
    COLORREF(u32::from(color.r) | (u32::from(color.g) << 8) | (u32::from(color.b) << 16))
}

fn paint_rect_background(hdc: HDC, rect: RECT, color: RgbColor) -> eyre::Result<()> {
    let brush = unsafe { CreateSolidBrush(rgb_to_colorref(color)) };
    if brush.0.is_null() {
        eyre::bail!("failed to create background brush");
    }

    let _ = unsafe { FillRect(hdc, &rect, brush) };
    let _ = unsafe { DeleteObject(brush.into()) };
    Ok(())
}

fn paint_terminal_background(hdc: HDC, client_width: i32, client_height: i32) -> eyre::Result<()> {
    let brush = unsafe { CreateSolidBrush(WINDOW_BACKGROUND) };
    if brush.0.is_null() {
        eyre::bail!("failed to create window background brush");
    }

    let rect = RECT {
        left: 0,
        top: 0,
        right: client_width,
        bottom: client_height,
    };
    let _ = unsafe { FillRect(hdc, &rect, brush) };
    let _ = unsafe { DeleteObject(brush.into()) };
    Ok(())
}

fn paint_drag_strip(hdc: HDC, client_width: i32, text: &str) -> eyre::Result<()> {
    let brush = unsafe { CreateSolidBrush(WINDOW_ACCENT) };
    if brush.0.is_null() {
        eyre::bail!("failed to create drag strip brush");
    }

    let rect = RECT {
        left: 0,
        top: 0,
        right: client_width,
        bottom: DRAG_STRIP_HEIGHT,
    };
    let _ = unsafe { FillRect(hdc, &rect, brush) };
    let _ = unsafe { DeleteObject(brush.into()) };

    let utf16 = text.encode_utf16().collect::<Vec<u16>>();
    let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let _ = unsafe { SetTextColor(hdc, WINDOW_TEXT) };
    let _ = unsafe { TextOutW(hdc, WINDOW_PADDING, 9, &utf16) };
    Ok(())
}

fn paint_cursor(
    hdc: HDC,
    layout: TerminalLayout,
    rect: RECT,
    cursor: CursorViewport,
    color: RgbColor,
) -> eyre::Result<()> {
    let left = rect.left + (i32::from(cursor.x) * layout.cell_width);
    let top = rect.top + (i32::from(cursor.y) * layout.cell_height);
    let cursor_rect = RECT {
        left,
        top: top + layout.cell_height.saturating_sub(3),
        right: left + layout.cell_width,
        bottom: top + layout.cell_height,
    };
    let brush = unsafe { CreateSolidBrush(rgb_to_colorref(color)) };
    if brush.0.is_null() {
        eyre::bail!("failed to create cursor brush");
    }
    let _ = unsafe { FillRect(hdc, &cursor_rect, brush) };
    let _ = unsafe { DeleteObject(brush.into()) };
    Ok(())
}

fn legacy_special_key_bytes(mapped_key: key::Key, mods: key::Mods) -> Option<Vec<u8>> {
    let mut key_event = key::Event::new().ok()?;
    let mut encoder = key::Encoder::new().ok()?;
    let mut response = Vec::with_capacity(16);
    key_event
        .set_action(key::Action::Press)
        .set_key(mapped_key)
        .set_mods(mods)
        .set_consumed_mods(key::Mods::empty())
        .set_unshifted_codepoint('\0')
        .set_utf8::<String>(None);
    encoder.encode_to_vec(&key_event, &mut response).ok()?;
    Some(response)
}
