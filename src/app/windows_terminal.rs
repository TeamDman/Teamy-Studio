use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use tracing::trace;

use eyre::Context;
use libghostty_vt::key;
use libghostty_vt::render::{CellIterator, CursorVisualStyle, RenderState, RowIterator};
use libghostty_vt::style::RgbColor;
use libghostty_vt::{Terminal, TerminalOptions};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tracing::{debug, info};
use windows::Win32::System::Console::{
    CAPSLOCK_ON, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, NUMLOCK_ON, RIGHT_ALT_PRESSED,
    RIGHT_CTRL_PRESSED, SHIFT_PRESSED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;

use crate::paths::AppHome;

use super::spatial::{ClientRect, TerminalCellPoint};

pub const DRAG_STRIP_HEIGHT: i32 = 76;
pub const WINDOW_PADDING: i32 = 18;
pub const POLL_TIMER_ID: usize = 1;
pub const POLL_INTERVAL_MS: u32 = 16;

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const MAX_SCROLLBACK: usize = 20_000;
const CELL_PANEL_GAP: i32 = 14;
const SIDECAR_WIDTH: i32 = 86;
const RESULT_PANEL_HEIGHT: i32 = 152;
const MIN_CODE_PANEL_HEIGHT: i32 = 180;
const PLUS_BUTTON_SIZE: i32 = 42;
const SIDECAR_BUTTON_SIZE: i32 = 34;
const SIDECAR_BUTTON_GAP: i32 = 12;
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalDisplayGlyph {
    pub cell: TerminalCellPoint,
    pub character: char,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalDisplayBackground {
    pub cell: TerminalCellPoint,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalDisplayCursorStyle {
    Bar,
    Block,
    Underline,
    BlockHollow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalDisplayCursor {
    pub cell: TerminalCellPoint,
    pub color: [f32; 4],
    pub style: TerminalDisplayCursorStyle,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalDisplayState {
    pub backgrounds: Vec<TerminalDisplayBackground>,
    pub glyphs: Vec<TerminalDisplayGlyph>,
    pub cursor: Option<TerminalDisplayCursor>,
}

#[derive(Clone, Copy)]
struct PendingWin32CharKey {
    vkey: u32,
    lparam: isize,
    mapped_key: key::Key,
    unshifted_codepoint: char,
    mods: key::Mods,
}

#[derive(Clone, Copy, Default)]
struct RepaintState {
    needs_repaint: bool,
    full_repaint_pending: bool,
}

#[derive(Clone, Copy, Default)]
struct Win32InputState {
    enabled: bool,
    pending_char_key: Option<PendingWin32CharKey>,
}

#[derive(Clone, Copy)]
struct Win32InputModeKeyEvent {
    key: PendingWin32CharKey,
    unicode_char: char,
    repeat_count: u16,
    key_down: bool,
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
    repaint: RepaintState,
    input_trace: Vec<Vec<u8>>,
    suppressed_chars: VecDeque<SuppressedChar>,
    win32_input: Win32InputState,
    win32_input_mode_buffer: Vec<u8>,
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
    pub fn frame_rect(self) -> ClientRect {
        ClientRect::new(
            WINDOW_PADDING,
            WINDOW_PADDING,
            (self.client_width - WINDOW_PADDING).max(WINDOW_PADDING),
            (self.client_height - WINDOW_PADDING).max(WINDOW_PADDING),
        )
    }

    #[must_use]
    pub fn sidecar_rect(self) -> ClientRect {
        let frame = self.frame_rect();
        let code = self.code_panel_rect();
        ClientRect::new(
            frame.left(),
            frame.top(),
            (frame.left() + SIDECAR_WIDTH).min(frame.right()),
            code.bottom(),
        )
    }

    #[must_use]
    pub fn drag_handle_rect(self) -> ClientRect {
        let sidecar = self.sidecar_rect();
        ClientRect::new(
            sidecar.left(),
            sidecar.top(),
            sidecar.right(),
            (sidecar.top() + DRAG_STRIP_HEIGHT).min(sidecar.bottom()),
        )
    }

    #[must_use]
    pub fn code_panel_rect(self) -> ClientRect {
        let frame = self.frame_rect();
        let code_left = (frame.left() + SIDECAR_WIDTH + CELL_PANEL_GAP).min(frame.right());
        let code_right = frame.right();
        let plus = self.plus_button_rect();
        let result_bottom = plus.top() - CELL_PANEL_GAP;
        let desired_result_top = result_bottom - RESULT_PANEL_HEIGHT;
        let minimum_code_bottom = frame.top() + MIN_CODE_PANEL_HEIGHT;
        let code_bottom = (desired_result_top - CELL_PANEL_GAP)
            .max(minimum_code_bottom)
            .min(result_bottom - CELL_PANEL_GAP);

        ClientRect::new(
            code_left,
            frame.top(),
            code_right,
            code_bottom.max(frame.top() + 1),
        )
    }

    #[must_use]
    pub fn result_panel_rect(self) -> ClientRect {
        let code = self.code_panel_rect();
        let plus = self.plus_button_rect();
        ClientRect::new(
            code.left(),
            code.bottom() + CELL_PANEL_GAP,
            code.right(),
            plus.top() - CELL_PANEL_GAP,
        )
    }

    #[must_use]
    pub fn plus_button_rect(self) -> ClientRect {
        let frame = self.frame_rect();
        let code_left = (frame.left() + SIDECAR_WIDTH + CELL_PANEL_GAP).min(frame.right());
        let code_right = frame.right();
        let center_x = code_left + ((code_right - code_left).max(PLUS_BUTTON_SIZE) / 2);
        let left = (center_x - (PLUS_BUTTON_SIZE / 2)).max(code_left);
        ClientRect::new(
            left,
            frame.bottom() - PLUS_BUTTON_SIZE,
            (left + PLUS_BUTTON_SIZE).min(code_right),
            frame.bottom(),
        )
    }

    #[must_use]
    pub fn sidecar_button_rect(self, index: usize) -> ClientRect {
        let sidecar = self.sidecar_rect();
        let top = self.drag_handle_rect().bottom()
            + 22
            + (i32::try_from(index).unwrap_or_default()
                * (SIDECAR_BUTTON_SIZE + SIDECAR_BUTTON_GAP));
        let left =
            sidecar.left() + ((sidecar.right() - sidecar.left() - SIDECAR_BUTTON_SIZE).max(0) / 2);
        ClientRect::new(
            left,
            top,
            left + SIDECAR_BUTTON_SIZE,
            top + SIDECAR_BUTTON_SIZE,
        )
    }

    #[must_use]
    pub fn terminal_rect(self) -> ClientRect {
        let code = self.code_panel_rect();
        ClientRect::new(code.left(), code.top(), code.right(), code.bottom())
    }

    #[must_use]
    pub fn grid_size(self) -> (u16, u16) {
        let rect = self.terminal_rect();
        let width = rect.width().max(self.cell_width.max(1));
        let height = rect.height().max(self.cell_height.max(1));
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
    pub fn new(app_home: &AppHome, working_dir: Option<&Path>) -> eyre::Result<Self> {
        let mut command = crate::shell_default::load_effective_command_builder(app_home)?;
        if let Some(working_dir) = working_dir {
            command.cwd(working_dir);
        }
        Self::new_with_command(command)
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
            repaint: RepaintState {
                needs_repaint: true,
                full_repaint_pending: true,
            },
            input_trace: Vec::new(),
            suppressed_chars: VecDeque::new(),
            win32_input: Win32InputState::default(),
            win32_input_mode_buffer: Vec::new(),
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
                pixel_width: u16::try_from(layout.terminal_rect().width().max(0))
                    .unwrap_or(u16::MAX),
                pixel_height: u16::try_from(layout.terminal_rect().height().max(0))
                    .unwrap_or(u16::MAX),
            })
            .map_err(|error| eyre::eyre!("failed to resize PTY: {error}"))?;

        self.cols = cols;
        self.rows = rows;
        self.repaint.needs_repaint = true;
        self.repaint.full_repaint_pending = true;
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

        self.repaint.needs_repaint |= changed;
        Ok(PumpResult {
            should_close: self.closed,
        })
    }

    pub fn handle_char(&mut self, code_unit: u32, lparam: isize) -> eyre::Result<bool> {
        trace!(
            code_unit,
            lparam,
            win32_input_mode = self.win32_input.enabled,
            suppressed_front = ?self.suppressed_chars.front().copied(),
            "handling character input"
        );
        if self.should_route_text_through_key_events()? {
            return Ok(false);
        }

        if !self.win32_input.enabled
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

        if self.win32_input.enabled {
            let Some(pending_key) = self.win32_input.pending_char_key else {
                return Ok(false);
            };

            self.write_win32_input_mode_char_event(pending_key, character, lparam)?;
            self.repaint.needs_repaint = true;
            return Ok(true);
        }

        if character == '\r' || character == '\t' || character == '\u{8}' {
            return Ok(false);
        }

        if character < ' ' {
            let control = u8::try_from(u32::from(character)).unwrap_or_default();
            self.write_input(&[control])?;
            self.repaint.needs_repaint = true;
            return Ok(true);
        }

        let mut bytes = [0_u8; 4];
        let encoded = character.encode_utf8(&mut bytes);
        self.write_input(encoded.as_bytes())?;
        self.repaint.needs_repaint = true;
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
        let Some(key_event) = mapped_key_event(vkey, lparam, mods) else {
            return Ok(false);
        };

        trace!(
            vkey,
            lparam,
            ?key_event.mapped_key,
            unshifted_codepoint = u32::from(key_event.unshifted_codepoint),
            ?mods,
            was_down,
            is_release,
            win32_input_mode = self.win32_input.enabled,
            "handling key event"
        );

        if is_release && !self.win32_input.enabled && !self.should_report_key_releases()? {
            return Ok(false);
        }

        let kitty_flags = self.current_kitty_keyboard_flags()?;
        if self.win32_input.enabled {
            return self.handle_win32_input_mode_key(key_event, is_release);
        }

        if kitty_flags.is_empty() {
            return self.handle_legacy_key_event(key_event, is_release);
        }

        self.handle_kitty_key_event(key_event, was_down, is_release)
    }

    fn handle_win32_input_mode_key(
        &mut self,
        key_event: PendingWin32CharKey,
        is_release: bool,
    ) -> eyre::Result<bool> {
        if is_release {
            self.write_win32_input_mode_key_event(Win32InputModeKeyEvent {
                key: key_event,
                unicode_char: '\0',
                repeat_count: 1,
                key_down: false,
            })?;
            if self
                .win32_input
                .pending_char_key
                .map(|pending| pending.vkey)
                == Some(key_event.vkey)
            {
                self.win32_input.pending_char_key = None;
            }
            self.repaint.needs_repaint = true;
            return Ok(true);
        }

        if should_route_key_through_char_input(
            key_event.mapped_key,
            key_event.unshifted_codepoint,
            true,
        ) {
            self.win32_input.pending_char_key = Some(key_event);
            return Ok(false);
        }

        self.write_win32_input_mode_key_event(Win32InputModeKeyEvent {
            key: key_event,
            unicode_char: legacy_key_event_character(
                key_event.mapped_key,
                key_event.unshifted_codepoint,
                key_event.mods,
            )
            .unwrap_or('\0'),
            repeat_count: lparam_repeat_count(key_event.lparam),
            key_down: true,
        })?;
        self.repaint.needs_repaint = true;
        Ok(true)
    }

    fn handle_legacy_key_event(
        &mut self,
        key_event: PendingWin32CharKey,
        is_release: bool,
    ) -> eyre::Result<bool> {
        if key_event.mapped_key == key::Key::Backspace {
            if is_release {
                debug!(
                    vkey = key_event.vkey,
                    "ignored legacy Backspace key release"
                );
                return Ok(false);
            }

            self.suppressed_chars
                .push_back(SuppressedChar::with_alternate(u32::from('\u{8}'), 0x7F));
            debug!(
                vkey = key_event.vkey,
                ?key_event.mods,
                suppressed_len = self.suppressed_chars.len(),
                "writing legacy Backspace byte and suppressing matching WM_CHAR"
            );
            self.write_input(&[0x7F])?;
            self.repaint.needs_repaint = true;
            return Ok(true);
        }

        if should_route_key_through_char_input(
            key_event.mapped_key,
            key_event.unshifted_codepoint,
            false,
        ) {
            return Ok(false);
        }

        let legacy_bytes =
            legacy_special_key_bytes(key_event.mapped_key, key_event.mods).unwrap_or_default();
        if legacy_bytes.is_empty() {
            return Ok(false);
        }

        self.write_input(&legacy_bytes)?;
        self.repaint.needs_repaint = true;
        Ok(true)
    }

    fn handle_kitty_key_event(
        &mut self,
        key_event: PendingWin32CharKey,
        was_down: bool,
        is_release: bool,
    ) -> eyre::Result<bool> {
        let action = if is_release {
            key::Action::Release
        } else if was_down {
            key::Action::Repeat
        } else {
            key::Action::Press
        };
        let mut response = Vec::with_capacity(16);
        let mut consumed_mods = key::Mods::empty();
        if key_event.unshifted_codepoint != '\0' && key_event.mods.contains(key::Mods::SHIFT) {
            consumed_mods |= key::Mods::SHIFT;
        }

        self.key_event
            .set_action(action)
            .set_key(key_event.mapped_key)
            .set_mods(key_event.mods)
            .set_consumed_mods(consumed_mods)
            .set_unshifted_codepoint(key_event.unshifted_codepoint)
            .set_utf8::<String>(None);

        self.key_encoder
            .set_options_from_terminal(&self.terminal)
            .encode_to_vec(&self.key_event, &mut response)
            .wrap_err("failed to encode special key event")?;

        if response.is_empty() {
            return Ok(false);
        }

        self.write_input(&response)?;
        self.repaint.needs_repaint = true;
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
        self.win32_input.enabled
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

    pub fn visible_display_state(&mut self) -> eyre::Result<TerminalDisplayState> {
        let snapshot = self
            .render_state
            .update(&self.terminal)
            .wrap_err("failed to update terminal render state")?;
        let colors = snapshot
            .colors()
            .wrap_err("failed to fetch terminal colors")?;
        let mut rows = RowIterator::new().wrap_err("failed to create row iterator")?;
        let mut cells = CellIterator::new().wrap_err("failed to create cell iterator")?;
        let cursor = build_terminal_cursor(&snapshot, &colors)?;
        let mut display = TerminalDisplayState {
            backgrounds: Vec::new(),
            glyphs: Vec::new(),
            cursor,
        };

        let mut row_index = 0_i32;
        let mut row_iter = rows
            .update(&snapshot)
            .wrap_err("failed to update row iterator")?;
        while let Some(row) = row_iter.next() {
            let mut column_index = 0_i32;
            let mut cell_iter = cells
                .update(row)
                .wrap_err("failed to update cell iterator")?;
            while let Some(cell) = cell_iter.next() {
                let style = cell.style().wrap_err("failed to read cell style")?;
                let graphemes = cell.graphemes().wrap_err("failed to read cell text")?;
                let foreground = cell.fg_color().wrap_err("failed to read cell foreground")?;
                let background = cell.bg_color().wrap_err("failed to read cell background")?;
                let (glyph_color, background_color) =
                    resolve_terminal_cell_colors(&colors, foreground, background, style.inverse);

                if let Some(color) = background_color {
                    display.backgrounds.push(TerminalDisplayBackground {
                        cell: TerminalCellPoint::new(column_index, row_index),
                        color,
                    });
                }

                if !graphemes.is_empty() {
                    for character in graphemes {
                        display.glyphs.push(TerminalDisplayGlyph {
                            cell: TerminalCellPoint::new(column_index, row_index),
                            character,
                            color: glyph_color,
                        });
                    }
                }
                column_index += 1;
            }
            row_index += 1;
        }

        Ok(display)
    }

    #[must_use]
    pub fn take_input_trace(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.input_trace)
    }

    fn write_input(&mut self, data: &[u8]) -> eyre::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|poison_error| eyre::eyre!("PTY writer mutex was poisoned: {poison_error}"))?;
        writer
            .write_all(data)
            .wrap_err("failed to write input to PTY")?;
        writer.flush().wrap_err("failed to flush PTY input")?;
        self.input_trace.push(data.to_vec());
        Ok(())
    }

    fn write_win32_input_mode_key_event(
        &mut self,
        event: Win32InputModeKeyEvent,
    ) -> eyre::Result<()> {
        let scancode = u32::from(lparam_scancode(event.key.lparam));
        let sequence = format!(
            "\x1b[{};{scancode};{};{};{};{}_",
            event.key.vkey,
            u32::from(event.unicode_char),
            u8::from(event.key_down),
            control_key_state(event.key.mods),
            event.repeat_count.max(1),
        );

        if event.key_down
            && !self.win32_input.enabled
            && let Some(character) =
                legacy_char_suppression(event.key.mapped_key, event.unicode_char)
        {
            self.suppressed_chars
                .push_back(SuppressedChar::single(u32::from(character)));
        }

        self.write_input(sequence.as_bytes())
    }

    fn write_win32_input_mode_char_event(
        &mut self,
        pending_key: PendingWin32CharKey,
        character: char,
        lparam: isize,
    ) -> eyre::Result<()> {
        self.write_win32_input_mode_key_event(Win32InputModeKeyEvent {
            key: pending_key,
            unicode_char: character,
            repeat_count: lparam_repeat_count(lparam),
            key_down: true,
        })
    }

    fn strip_win32_input_mode_sequence<'a>(&mut self, data: &'a [u8]) -> Cow<'a, [u8]> {
        let mut combined = std::mem::take(&mut self.win32_input_mode_buffer);
        combined.extend_from_slice(data);
        let mut output = Vec::with_capacity(combined.len());
        let mut index = 0;

        while index < combined.len() {
            if combined[index..].starts_with(WIN32_INPUT_MODE_ENABLE) {
                self.win32_input.enabled = true;
                self.win32_input.pending_char_key = None;
                index += WIN32_INPUT_MODE_ENABLE.len();
                continue;
            }

            if combined[index..].starts_with(WIN32_INPUT_MODE_DISABLE) {
                self.win32_input.enabled = false;
                self.win32_input.pending_char_key = None;
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

fn rgb_to_rgba(color: RgbColor) -> [f32; 4] {
    [
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
        1.0,
    ]
}

fn resolve_terminal_cell_colors(
    colors: &libghostty_vt::render::Colors,
    foreground: Option<RgbColor>,
    background: Option<RgbColor>,
    inverse: bool,
) -> ([f32; 4], Option<[f32; 4]>) {
    let mut foreground = foreground.unwrap_or(colors.foreground);
    let mut background = background.unwrap_or(colors.background);
    let mut draw_background = background != colors.background;

    if inverse {
        std::mem::swap(&mut foreground, &mut background);
        draw_background = true;
    }

    (
        rgb_to_rgba(foreground),
        draw_background.then(|| rgb_to_rgba(background)),
    )
}

fn build_terminal_cursor(
    snapshot: &libghostty_vt::render::Snapshot<'_, '_>,
    colors: &libghostty_vt::render::Colors,
) -> eyre::Result<Option<TerminalDisplayCursor>> {
    if !snapshot
        .cursor_visible()
        .wrap_err("failed to query cursor visibility")?
    {
        return Ok(None);
    }

    let Some(viewport) = snapshot
        .cursor_viewport()
        .wrap_err("failed to query cursor viewport")?
    else {
        return Ok(None);
    };
    let style = snapshot
        .cursor_visual_style()
        .wrap_err("failed to query cursor visual style")?;
    let cursor_color = snapshot
        .cursor_color()
        .wrap_err("failed to query cursor color")?
        .or(colors.cursor)
        .unwrap_or(colors.foreground);

    Ok(Some(TerminalDisplayCursor {
        cell: TerminalCellPoint::new(i32::from(viewport.x), i32::from(viewport.y)),
        color: rgb_to_rgba(cursor_color),
        style: map_cursor_style(style),
    }))
}

fn map_cursor_style(style: CursorVisualStyle) -> TerminalDisplayCursorStyle {
    match style {
        CursorVisualStyle::Bar => TerminalDisplayCursorStyle::Bar,
        CursorVisualStyle::Underline => TerminalDisplayCursorStyle::Underline,
        CursorVisualStyle::BlockHollow => TerminalDisplayCursorStyle::BlockHollow,
        _ => TerminalDisplayCursorStyle::Block,
    }
}

fn mapped_key_event(vkey: u32, lparam: isize, mods: key::Mods) -> Option<PendingWin32CharKey> {
    let (mapped_key, unshifted_codepoint) = map_virtual_key(vkey, lparam)?;
    Some(PendingWin32CharKey {
        vkey,
        lparam,
        mapped_key,
        unshifted_codepoint,
        mods,
    })
}

fn map_virtual_key(vkey: u32, lparam: isize) -> Option<(key::Key, char)> {
    map_printable_virtual_key(vkey).or_else(|| {
        map_modifier_virtual_key(vkey, lparam_is_extended(lparam), lparam_scancode(lparam))
            .or_else(|| map_navigation_virtual_key(vkey))
    })
}

fn map_printable_virtual_key(vkey: u32) -> Option<(key::Key, char)> {
    match vkey {
        0x20 => Some((key::Key::Space, ' ')),
        0x30 | 0x60 => Some((key::Key::Digit0, '0')),
        0x31 | 0x61 => Some((key::Key::Digit1, '1')),
        0x32 | 0x62 => Some((key::Key::Digit2, '2')),
        0x33 | 0x63 => Some((key::Key::Digit3, '3')),
        0x34 | 0x64 => Some((key::Key::Digit4, '4')),
        0x35 | 0x65 => Some((key::Key::Digit5, '5')),
        0x36 | 0x66 => Some((key::Key::Digit6, '6')),
        0x37 | 0x67 => Some((key::Key::Digit7, '7')),
        0x38 | 0x68 => Some((key::Key::Digit8, '8')),
        0x39 | 0x69 => Some((key::Key::Digit9, '9')),
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
        0x6A => Some((key::Key::Digit8, '*')),
        0x6B => Some((key::Key::Equal, '+')),
        0x6D | 0xBD => Some((key::Key::Minus, '-')),
        0x6E | 0xBE => Some((key::Key::Period, '.')),
        0x6F | 0xBF => Some((key::Key::Slash, '/')),
        0xBA => Some((key::Key::Semicolon, ';')),
        0xBB => Some((key::Key::Equal, '=')),
        0xBC => Some((key::Key::Comma, ',')),
        0xC0 => Some((key::Key::Backquote, '`')),
        0xDB => Some((key::Key::BracketLeft, '[')),
        0xDC => Some((key::Key::Backslash, '\\')),
        0xDD => Some((key::Key::BracketRight, ']')),
        0xDE => Some((key::Key::Quote, '\'')),
        _ => None,
    }
}

fn map_modifier_virtual_key(vkey: u32, extended: bool, scancode: u8) -> Option<(key::Key, char)> {
    match vkey {
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
        _ => None,
    }
}

fn map_navigation_virtual_key(vkey: u32) -> Option<(key::Key, char)> {
    match vkey {
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

fn lparam_low_u32(lparam: isize) -> u32 {
    let bytes = lparam.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn lparam_scancode(lparam: isize) -> u8 {
    u8::try_from((lparam_low_u32(lparam) >> 16) & 0xFF).unwrap_or_default()
}

fn lparam_is_extended(lparam: isize) -> bool {
    ((lparam_low_u32(lparam) >> 24) & 1) != 0
}

fn lparam_repeat_count(lparam: isize) -> u16 {
    u16::try_from(lparam_low_u32(lparam) & 0xFFFF)
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
        let extended = lparam_is_extended(lparam);
        let scancode = lparam_scancode(lparam);
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

fn key_state(vkey: i32) -> u16 {
    // Safety: `GetKeyState` reads the current thread keyboard state for a virtual key.
    let state = unsafe { GetKeyState(vkey) };
    u16::from_ne_bytes(state.to_ne_bytes())
}

fn key_is_pressed(vkey: i32) -> bool {
    (key_state(vkey) & 0x8000) != 0
}

fn key_is_toggled(vkey: i32) -> bool {
    (key_state(vkey) & 0x0001) != 0
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
    (matches!(
        mapped_key,
        key::Key::Backspace | key::Key::Tab | key::Key::Enter
    ) || unicode_char != '\0')
        .then_some(unicode_char)
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

#[cfg(test)]
mod tests {
    use super::{
        MIN_CODE_PANEL_HEIGHT, TerminalDisplayCursorStyle, TerminalLayout, map_cursor_style,
        map_virtual_key, resolve_terminal_cell_colors,
    };
    use libghostty_vt::render::Colors;
    use libghostty_vt::style::RgbColor;

    #[test]
    fn cell_layout_regions_do_not_overlap_and_leave_terminal_room() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
        };

        let sidecar = layout.sidecar_rect();
        let code = layout.code_panel_rect();
        let result = layout.result_panel_rect();
        let plus = layout.plus_button_rect();
        let terminal = layout.terminal_rect();

        assert!(sidecar.right() <= code.left());
        assert!(code.bottom() < result.top());
        assert!(result.bottom() < plus.top());
        assert_eq!(terminal.left(), code.left());
        assert_eq!(terminal.right(), code.right());
        assert_eq!(terminal.bottom(), code.bottom());
        assert!(code.height() >= MIN_CODE_PANEL_HEIGHT);
    }

    #[test]
    fn inverse_cells_swap_colors_and_force_background() {
        let colors = Colors {
            background: RgbColor {
                r: 10,
                g: 20,
                b: 30,
            },
            foreground: RgbColor {
                r: 240,
                g: 241,
                b: 242,
            },
            cursor: None,
            palette: [RgbColor { r: 0, g: 0, b: 0 }; 256],
        };

        let (foreground, background) =
            resolve_terminal_cell_colors(&colors, Some(RgbColor { r: 1, g: 2, b: 3 }), None, true);

        assert_eq!(foreground, [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0]);
        assert_eq!(
            background,
            Some([1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0, 1.0])
        );
    }

    #[test]
    fn cursor_style_mapping_matches_ghostty_values() {
        assert_eq!(
            map_cursor_style(libghostty_vt::render::CursorVisualStyle::Bar),
            TerminalDisplayCursorStyle::Bar
        );
        assert_eq!(
            map_cursor_style(libghostty_vt::render::CursorVisualStyle::Block),
            TerminalDisplayCursorStyle::Block
        );
        assert_eq!(
            map_cursor_style(libghostty_vt::render::CursorVisualStyle::Underline),
            TerminalDisplayCursorStyle::Underline
        );
        assert_eq!(
            map_cursor_style(libghostty_vt::render::CursorVisualStyle::BlockHollow),
            TerminalDisplayCursorStyle::BlockHollow
        );
    }

    #[test]
    fn drag_handle_stays_within_sidecar() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
        };

        let drag = layout.drag_handle_rect();
        let sidecar = layout.sidecar_rect();

        assert!(drag.left() >= sidecar.left());
        assert!(drag.right() <= sidecar.right());
        assert!(drag.top() >= sidecar.top());
        assert!(drag.bottom() <= sidecar.bottom());
    }

    #[test]
    fn map_virtual_key_maps_numpad_digits_to_text() {
        for (vkey, expected) in [
            (0x60, '0'),
            (0x61, '1'),
            (0x62, '2'),
            (0x63, '3'),
            (0x64, '4'),
            (0x65, '5'),
            (0x66, '6'),
            (0x67, '7'),
            (0x68, '8'),
            (0x69, '9'),
        ] {
            let (_, actual) = map_virtual_key(vkey, 0)
                .unwrap_or_else(|| panic!("expected numpad vkey {vkey:#X} to map"));
            assert_eq!(
                actual, expected,
                "unexpected char for numpad vkey {vkey:#X}"
            );
        }
    }

    #[test]
    fn map_virtual_key_maps_numpad_operators_to_text() {
        for (vkey, expected) in [
            (0x6A, '*'),
            (0x6B, '+'),
            (0x6D, '-'),
            (0x6E, '.'),
            (0x6F, '/'),
        ] {
            let (_, actual) = map_virtual_key(vkey, 0)
                .unwrap_or_else(|| panic!("expected numpad vkey {vkey:#X} to map"));
            assert_eq!(
                actual, expected,
                "unexpected char for numpad vkey {vkey:#X}"
            );
        }
    }
}
