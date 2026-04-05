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

type PtyWriter = Box<dyn Write + Send>;

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
    pub fn new() -> eyre::Result<Self> {
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

        let writer: Arc<Mutex<PtyWriter>> = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|error| eyre::eyre!("failed to open PTY writer: {error}"))?,
        ));
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

        info!("starting Teamy Studio shell");
        let shell = default_shell_command();
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
        Ok(())
    }

    pub fn pump(&mut self) -> eyre::Result<bool> {
        let mut changed = false;

        while let Ok(message) = self.reader.try_recv() {
            match message {
                Ok(bytes) => {
                    self.terminal.vt_write(&bytes);
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

        if self.child.try_wait().wrap_err("failed to query shell status")?.is_some() {
            self.closed = true;
        }

        self.needs_repaint |= changed;
        Ok(self.closed)
    }

    pub fn handle_char(&mut self, code_unit: u32) -> eyre::Result<bool> {
        let Some(character) = char::from_u32(code_unit) else {
            return Ok(false);
        };

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

    pub fn handle_keydown(
        &mut self,
        vkey: u32,
        was_down: bool,
        mods: key::Mods,
    ) -> eyre::Result<bool> {
        let Some(mapped_key) = map_virtual_key(vkey) else {
            return Ok(false);
        };

        let action = if was_down {
            key::Action::Repeat
        } else {
            key::Action::Press
        };
        let mut response = Vec::with_capacity(16);
        self.key_event
            .set_action(action)
            .set_key(mapped_key)
            .set_mods(mods)
            .set_consumed_mods(key::Mods::empty())
            .set_unshifted_codepoint('\0')
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

    pub fn paint(&mut self, hdc: HDC, layout: TerminalLayout, overlay_text: &str) -> eyre::Result<()> {
        paint_terminal_background(hdc, layout.client_width, layout.client_height)?;
        paint_drag_strip(hdc, layout.client_width, overlay_text)?;

        let snapshot = self
            .render_state
            .update(&self.terminal)
            .wrap_err("failed to update terminal render state")?;
        let colors = snapshot.colors().wrap_err("failed to fetch terminal colors")?;
        let mut rows = RowIterator::new().wrap_err("failed to create row iterator")?;
        let mut cells = CellIterator::new().wrap_err("failed to create cell iterator")?;
        let rect = layout.terminal_rect();

        let background_brush = unsafe { CreateSolidBrush(rgb_to_colorref(colors.background)) };
        if background_brush.0.is_null() {
            eyre::bail!("failed to create terminal background brush");
        }
        let _ = unsafe { FillRect(hdc, &rect, background_brush) };
        let _ = unsafe { DeleteObject(background_brush.into()) };

        let mut row_index = 0_i32;
        let mut row_iter = rows.update(&snapshot).wrap_err("failed to update row iterator")?;
        while let Some(row) = row_iter.next() {
            let y = rect.top + (row_index * layout.cell_height);
            let mut column_index = 0_i32;
            let mut cell_iter = cells.update(row).wrap_err("failed to update cell iterator")?;

            while let Some(cell) = cell_iter.next() {
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

            row.set_dirty(false)
                .wrap_err("failed to clear row dirty flag after paint")?;
            row_index += 1;
        }

        snapshot
            .set_dirty(libghostty_vt::render::Dirty::Clean)
            .wrap_err("failed to clear render dirty state")?;

        if snapshot.cursor_visible().unwrap_or(false) {
            if let Some(cursor) = snapshot.cursor_viewport().wrap_err("failed to fetch cursor")? {
                paint_cursor(hdc, layout, rect, cursor, colors.cursor.unwrap_or(colors.foreground))?;
            }
        }

        self.needs_repaint = false;
        Ok(())
    }

    fn write_input(&mut self, data: &[u8]) -> eyre::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| eyre::eyre!("PTY writer mutex was poisoned"))?;
        writer.write_all(data).wrap_err("failed to write input to PTY")?;
        writer.flush().wrap_err("failed to flush PTY input")?;
        Ok(())
    }
}

fn default_shell_command() -> CommandBuilder {
    #[cfg(windows)]
    {
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned());
        let mut command = CommandBuilder::new(shell);
        command.arg("/D");
        command
    }

    #[cfg(not(windows))]
    {
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()))
    }
}

fn map_virtual_key(vkey: u32) -> Option<key::Key> {
    match vkey {
        0x08 => Some(key::Key::Backspace),
        0x09 => Some(key::Key::Tab),
        0x0D => Some(key::Key::Enter),
        0x1B => Some(key::Key::Escape),
        0x21 => Some(key::Key::PageUp),
        0x22 => Some(key::Key::PageDown),
        0x23 => Some(key::Key::End),
        0x24 => Some(key::Key::Home),
        0x25 => Some(key::Key::ArrowLeft),
        0x26 => Some(key::Key::ArrowUp),
        0x27 => Some(key::Key::ArrowRight),
        0x28 => Some(key::Key::ArrowDown),
        0x2D => Some(key::Key::Insert),
        0x2E => Some(key::Key::Delete),
        _ => None,
    }
}

pub fn keyboard_mods() -> key::Mods {
    key::Mods::empty()
}

fn rgb_to_colorref(color: RgbColor) -> COLORREF {
    COLORREF(u32::from(color.r) | (u32::from(color.g) << 8) | (u32::from(color.b) << 16))
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