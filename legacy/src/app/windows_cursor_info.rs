mod pixel_size;

use std::io::{BufWriter, Write, stderr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{convert::Infallible, mem};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use eyre::Context;
use image::{Rgba, RgbaImage};
use pixel_size::PixelSize;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW,
    GetObjectW, HDC, HMONITOR, MONITORINFO, RGBQUAD, ReleaseDC, SRCCOPY, SelectObject, StretchBlt,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RIGHT, VK_UP};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSOR_SUPPRESSED, CURSORINFO, CURSORINFO_FLAGS, EnumWindows, GetClassNameW,
    GetCursorInfo, GetIconInfo, GetSystemMetrics, GetWindowRect, HCURSOR, ICONINFO,
    IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP, IDC_IBEAM, IDC_NO, IDC_SIZEALL,
    IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_UPARROW, IDC_WAIT, IsWindowVisible,
    LoadCursorW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::core::BOOL;

use super::spatial::{ScreenPoint, ScreenRect, TerminalCellPoint};
use super::windows_terminal::{
    PollPtyOutputResult, PumpResult, TerminalDisplayBackground, TerminalDisplayGlyph,
    TerminalDisplayRow, TerminalDisplayScrollbar, TerminalDisplayState, TerminalLayout,
    TerminalPerformanceSnapshot, TerminalViewportMetrics,
};

const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MIN_SCALE: i32 = 1;
const TEAMY_TERMINAL_WINDOW_CLASS_NAME: &str = "TeamyStudioTerminalWindow";
const TEAMY_SCENE_WINDOW_CLASS_NAME: &str = "TeamyStudioSceneWindow";
const TEAMY_BENCHMARK_WINDOW_CLASS_NAME: &str = "TeamyStudioTerminalBenchmarkWindow";
const TOOLTIP_WINDOW_CLASS_NAME: &str = "tooltips_class32";
const PRIMARY_TASKBAR_WINDOW_CLASS_NAME: &str = "Shell_TrayWnd";
const SECONDARY_TASKBAR_WINDOW_CLASS_NAME: &str = "Shell_SecondaryTrayWnd";
const DESKTOP_WINDOW_CLASS_NAMES: [&str; 2] = ["Progman", "WorkerW"];
const SIDEBAR_WIDTH: u16 = 34;
const LEGEND_STATIC_ROWS: u16 = 7;
const CURSOR_INFO_ENTER_TERMINAL_UI: &str =
    "\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h";
const CURSOR_INFO_EXIT_TERMINAL_UI: &str =
    "\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1049l";
type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

#[derive(Debug)]
struct SharedWriter<W> {
    inner: Arc<Mutex<W>>,
}

impl<W> SharedWriter<W> {
    fn new(writer: W) -> Self {
        Self {
            inner: Arc::new(Mutex::new(writer)),
        }
    }
}

impl<W> Clone for SharedWriter<W> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<W: Write> Write for SharedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut writer = self.inner.lock().map_err(|error| {
            std::io::Error::other(format!(
                "cursor-info output writer mutex was poisoned: {error}"
            ))
        })?;
        writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut writer = self.inner.lock().map_err(|error| {
            std::io::Error::other(format!(
                "cursor-info output writer mutex was poisoned: {error}"
            ))
        })?;
        writer.flush()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorInfoRenderMode {
    Mask,
    Desktop,
    Overlay,
}

impl CursorInfoRenderMode {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Mask => Self::Desktop,
            Self::Desktop => Self::Overlay,
            Self::Overlay => Self::Mask,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mask => "mask",
            Self::Desktop => "desktop",
            Self::Overlay => "overlay",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorInfoPixelSize {
    Full,
    HalfHeight,
}

impl From<CursorInfoPixelSize> for PixelSize {
    fn from(value: CursorInfoPixelSize) -> Self {
        match value {
            CursorInfoPixelSize::Full => Self::Full,
            CursorInfoPixelSize::HalfHeight => Self::HalfHeight,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorInfoConfig {
    pub initial_mode: CursorInfoRenderMode,
    pub scale: i32,
    pub pixel_size: CursorInfoPixelSize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CursorGeometry {
    hotspot: ScreenPoint,
    rect: ScreenRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservedWindowKind {
    Teamy,
    Taskbar,
    Foreign,
    Tooltip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObservedMonitor {
    rect: ScreenRect,
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ObservedWindow {
    rect: ScreenRect,
    kind: ObservedWindowKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SemanticClass {
    OutsideAllMonitors,
    DesktopMonitor(usize),
    ForeignWindow,
    Taskbar,
    TeamyWindow,
    Tooltip,
    CursorMask,
    CursorHotspot,
}

#[derive(Clone, Debug, PartialEq)]
struct CursorInfoSnapshot {
    cursor: CursorGeometry,
    cursor_name: &'static str,
    virtual_bounds: ScreenRect,
    current_monitor_bounds: ScreenRect,
    current_monitor_index: Option<usize>,
    monitors: Vec<ObservedMonitor>,
    windows: Vec<ObservedWindow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorVisibility {
    Visible,
    Hidden,
    Suppressed,
}

struct CursorInfoState {
    mode: CursorInfoRenderMode,
    scale: i32,
    pixel_size: PixelSize,
    viewport_center: ScreenPoint,
    follow_cursor: bool,
    drag_anchor: Option<(u16, u16)>,
    last_canvas_area: Rect,
    last_frame_started_at: Instant,
}

impl CursorInfoState {
    fn new(config: CursorInfoConfig, cursor_hotspot: ScreenPoint) -> Self {
        Self {
            mode: config.initial_mode,
            scale: config.scale.max(MIN_SCALE),
            pixel_size: config.pixel_size.into(),
            viewport_center: cursor_hotspot,
            follow_cursor: true,
            drag_anchor: None,
            last_canvas_area: Rect::default(),
            last_frame_started_at: Instant::now(),
        }
    }

    fn zoom_in(&mut self) {
        self.scale = (self.scale - 1).max(MIN_SCALE);
    }

    fn zoom_out(&mut self) {
        self.scale = self.scale.saturating_add(1).max(MIN_SCALE);
    }

    fn pan_by_cells(&mut self, horizontal_cells: i32, vertical_cells: i32) {
        let (pixels_per_cell_x, pixels_per_cell_y) = self.pixel_size.pixels_per_cell();
        self.viewport_center = ScreenPoint::new(
            self.viewport_center.x_px()
                - (horizontal_cells * self.scale * i32::from(pixels_per_cell_x)),
            self.viewport_center.y_px()
                - (vertical_cells * self.scale * i32::from(pixels_per_cell_y)),
        );
        self.follow_cursor = false;
    }
}

struct TerminalRestoreGuard<W: Write + Send + 'static> {
    original_hook: Arc<Mutex<Option<PanicHook>>>,
    restore_writer: SharedWriter<W>,
}

impl<W: Write + Send + 'static> TerminalRestoreGuard<W> {
    fn enter(writer: SharedWriter<W>) -> eyre::Result<Self> {
        let original_hook = Arc::new(Mutex::new(Some(std::panic::take_hook())));
        let hook_for_panic = Arc::clone(&original_hook);
        let writer_for_panic = writer.clone();
        std::panic::set_hook(Box::new(move |info| {
            let mut writer = writer_for_panic.clone();
            let _ = restore_terminal_state(&mut writer);
            if let Ok(guard) = hook_for_panic.lock()
                && let Some(hook) = guard.as_ref()
            {
                hook(info);
            }
        }));

        enable_raw_mode().wrap_err("failed to enable raw mode for cursor-info")?;
        let mut writer_handle = writer.clone();
        write_terminal_ui_sequence(&mut writer_handle, CURSOR_INFO_ENTER_TERMINAL_UI)
            .wrap_err("failed to enter alternate screen for cursor-info")?;
        Ok(Self {
            original_hook,
            restore_writer: writer,
        })
    }
}

impl<W: Write + Send + 'static> Drop for TerminalRestoreGuard<W> {
    fn drop(&mut self) {
        let mut writer = self.restore_writer.clone();
        let _ = restore_terminal_state(&mut writer);
        if let Ok(mut guard) = self.original_hook.lock()
            && let Some(hook) = guard.take()
        {
            std::panic::set_hook(hook);
        }
    }
}

pub fn run(config: CursorInfoConfig) -> eyre::Result<()> {
    run_with_crossterm_writer(stderr(), config)
}

fn run_with_crossterm_writer<W: Write + Send + 'static>(
    writer: W,
    config: CursorInfoConfig,
) -> eyre::Result<()> {
    let shared_writer = SharedWriter::new(writer);
    let _restore_guard = TerminalRestoreGuard::enter(shared_writer.clone())?;
    let backend = CrosstermBackend::new(BufWriter::new(shared_writer));
    let mut terminal =
        Terminal::new(backend).wrap_err("failed to create cursor-info terminal backend")?;
    terminal
        .clear()
        .wrap_err("failed to clear cursor-info terminal")?;

    let initial_snapshot = capture_snapshot()?;
    let mut state = CursorInfoState::new(config, initial_snapshot.cursor.hotspot);
    run_event_loop(&mut terminal, &mut state)
}

fn restore_terminal_state<W: Write>(writer: &mut W) -> eyre::Result<()> {
    disable_raw_mode().wrap_err("failed to disable raw mode")?;
    write_terminal_ui_sequence(writer, CURSOR_INFO_EXIT_TERMINAL_UI)
        .wrap_err("failed to restore alternate screen")?;
    Ok(())
}

fn write_terminal_ui_sequence<W: Write>(writer: &mut W, sequence: &str) -> eyre::Result<()> {
    writer
        .write_all(sequence.as_bytes())
        .wrap_err("failed to write terminal control sequence")?;
    writer
        .flush()
        .wrap_err("failed to flush terminal control sequence")?;
    Ok(())
}

fn run_event_loop<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<BufWriter<SharedWriter<W>>>>,
    state: &mut CursorInfoState,
) -> eyre::Result<()> {
    loop {
        let snapshot = capture_snapshot()?;
        if state.follow_cursor {
            state.viewport_center = snapshot.cursor.hotspot;
        }

        render_cursor_info_frame(terminal, state, &snapshot)
            .wrap_err("failed to draw cursor-info frame")?;

        if event::poll(FRAME_POLL_INTERVAL).wrap_err("failed to poll cursor-info input")? {
            match event::read().wrap_err("failed to read cursor-info input")? {
                Event::Key(key)
                    if key.kind == KeyEventKind::Press
                        && handle_key_event(state, key, snapshot.cursor.hotspot) =>
                {
                    break;
                }
                Event::Mouse(mouse) => handle_mouse_event(state, mouse),
                _ => {}
            }
        }
    }

    Ok(())
}

fn render_cursor_info_frame<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut CursorInfoState,
    snapshot: &CursorInfoSnapshot,
) -> Result<(), B::Error> {
    let terminal_area = terminal.size()?;
    let [canvas_outer, _sidebar_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDEBAR_WIDTH)])
            .areas(terminal_area.into());
    state.last_canvas_area = Block::bordered().title("Viewport").inner(canvas_outer);

    let frame_data = build_frame_data(state, snapshot)
        .expect("cursor-info frame data should build before drawing the frame");

    terminal.draw(|frame| {
        draw_cursor_info_frame(frame, state, snapshot, &frame_data);
    })?;
    state.last_frame_started_at = Instant::now();
    Ok(())
}

fn draw_cursor_info_frame(
    frame: &mut Frame<'_>,
    state: &mut CursorInfoState,
    snapshot: &CursorInfoSnapshot,
    frame_data: &CursorInfoFrameData,
) {
    let area = frame.area();
    let [canvas_outer, sidebar_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDEBAR_WIDTH)]).areas(area);
    let canvas_block = Block::bordered().title("Viewport");
    let canvas_area = canvas_block.inner(canvas_outer);
    canvas_block.render(canvas_outer, frame.buffer_mut());
    state.last_canvas_area = canvas_area;

    let legend_height = legend_height(snapshot);
    let [legend_area, info_area] =
        Layout::vertical([Constraint::Length(legend_height), Constraint::Fill(1)])
            .areas(sidebar_area);

    render_canvas(
        frame.buffer_mut(),
        canvas_area,
        state.pixel_size,
        frame_data,
    );
    render_legend(frame.buffer_mut(), legend_area, snapshot);
    render_info(frame.buffer_mut(), info_area, state, snapshot);
}

fn test_backend_to_terminal_display(
    backend: &TestBackend,
    terminal_area: Rect,
) -> TerminalDisplayState {
    const TEAMY_FOREGROUND: [f32; 4] = [0.93, 0.95, 0.98, 1.0];
    const TEAMY_BACKGROUND: [f32; 4] = [0.06, 0.07, 0.09, 1.0];

    let buffer = backend.buffer();
    let width = usize::from(terminal_area.width);
    let height = usize::from(terminal_area.height);
    let mut rows = Vec::with_capacity(height);
    for row_index in 0..height {
        let mut display_row = TerminalDisplayRow {
            row: i32::try_from(row_index).unwrap_or(i32::MAX),
            backgrounds: Vec::new(),
            glyphs: Vec::new(),
        };

        for column_index in 0..width {
            let cell = &buffer.content[row_index * width + column_index];
            let viewport_cell = TerminalCellPoint::new(
                i32::try_from(column_index).unwrap_or(i32::MAX),
                i32::try_from(row_index).unwrap_or(i32::MAX),
            );
            let (glyph_color, background) = ratatui_cell_colors(
                cell.fg,
                cell.bg,
                cell.modifier,
                TEAMY_FOREGROUND,
                TEAMY_BACKGROUND,
            );
            if let Some(background) = background {
                display_row.backgrounds.push(TerminalDisplayBackground {
                    cell: viewport_cell,
                    color: background,
                });
            }

            if !cell.skip {
                let character = cell.symbol().chars().next().unwrap_or(' ');
                if character != ' ' && !cell.modifier.contains(Modifier::HIDDEN) {
                    display_row.glyphs.push(TerminalDisplayGlyph {
                        cell: viewport_cell,
                        character,
                        color: glyph_color,
                    });
                }
            }
        }
        rows.push(display_row);
    }

    TerminalDisplayState {
        dirty_rows: (0..rows.len()).collect(),
        rows,
        cursor: None,
        scrollbar: Some(TerminalDisplayScrollbar {
            total: u64::from(terminal_area.height),
            offset: 0,
            visible: u64::from(terminal_area.height),
        }),
    }
}

fn ratatui_cell_colors(
    foreground: Color,
    background: Color,
    modifier: Modifier,
    default_foreground: [f32; 4],
    default_background: [f32; 4],
) -> ([f32; 4], Option<[f32; 4]>) {
    let mut foreground = ratatui_color_to_rgba(foreground, default_foreground);
    let mut background_rgba = ratatui_color_to_rgba(background, default_background);
    let mut draw_background = background != Color::Reset;

    if modifier.contains(Modifier::REVERSED) {
        std::mem::swap(&mut foreground, &mut background_rgba);
        draw_background = true;
    }

    (foreground, draw_background.then_some(background_rgba))
}

fn ratatui_color_to_rgba(color: Color, default_color: [f32; 4]) -> [f32; 4] {
    match color {
        Color::Reset => default_color,
        Color::Black => xterm_palette_color(0),
        Color::Red => xterm_palette_color(1),
        Color::Green => xterm_palette_color(2),
        Color::Yellow => xterm_palette_color(3),
        Color::Blue => xterm_palette_color(4),
        Color::Magenta => xterm_palette_color(5),
        Color::Cyan => xterm_palette_color(6),
        Color::Gray => xterm_palette_color(7),
        Color::DarkGray => xterm_palette_color(8),
        Color::LightRed => xterm_palette_color(9),
        Color::LightGreen => xterm_palette_color(10),
        Color::LightYellow => xterm_palette_color(11),
        Color::LightBlue => xterm_palette_color(12),
        Color::LightMagenta => xterm_palette_color(13),
        Color::LightCyan => xterm_palette_color(14),
        Color::White => xterm_palette_color(15),
        Color::Rgb(r, g, b) => [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        ],
        Color::Indexed(index) => xterm_palette_color(index),
    }
}

fn xterm_palette_color(index: u8) -> [f32; 4] {
    let (r, g, b) = match index {
        0 => (0, 0, 0),
        1 => (205, 49, 49),
        2 => (13, 188, 121),
        3 => (229, 229, 16),
        4 => (36, 114, 200),
        5 => (188, 63, 188),
        6 => (17, 168, 205),
        7 => (229, 229, 229),
        8 => (102, 102, 102),
        9 => (241, 76, 76),
        10 => (35, 209, 139),
        11 => (245, 245, 67),
        12 => (59, 142, 234),
        13 => (214, 112, 214),
        14 => (41, 184, 219),
        15 => (255, 255, 255),
        16..=231 => {
            let index = index - 16;
            let red = index / 36;
            let green = (index % 36) / 6;
            let blue = index % 6;
            let component = |value: u8| if value == 0 { 0 } else { (value * 40) + 55 };
            (component(red), component(green), component(blue))
        }
        232..=255 => {
            let gray = 8 + ((index - 232) * 10);
            (gray, gray, gray)
        }
    };

    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    ]
}

fn test_backend_visible_text(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let width = usize::from(buffer.area.width);
    let height = usize::from(buffer.area.height);
    let mut rows = Vec::with_capacity(height);
    for row_index in 0..height {
        let start = row_index * width;
        let end = start + width;
        let mut row = String::new();
        for cell in &buffer.content[start..end] {
            row.push_str(cell.symbol());
        }
        rows.push(row.trim_end().to_owned());
    }
    rows.join("\n")
}

fn legend_height(snapshot: &CursorInfoSnapshot) -> u16 {
    snapshot
        .monitors
        .len()
        .try_into()
        .unwrap_or(u16::MAX)
        .saturating_add(LEGEND_STATIC_ROWS)
        .saturating_add(2)
}

fn handle_key_event(
    state: &mut CursorInfoState,
    key: KeyEvent,
    cursor_hotspot: ScreenPoint,
) -> bool {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => true,
        KeyCode::Char('x') => {
            state.mode = state.mode.next();
            false
        }
        KeyCode::Char('f') => {
            state.follow_cursor = !state.follow_cursor;
            if state.follow_cursor {
                state.viewport_center = cursor_hotspot;
            }
            false
        }
        KeyCode::Char('+' | '=') => {
            state.zoom_in();
            false
        }
        KeyCode::Char('-') => {
            state.zoom_out();
            false
        }
        KeyCode::Left => {
            state.pan_by_cells(4, 0);
            false
        }
        KeyCode::Right => {
            state.pan_by_cells(-4, 0);
            false
        }
        KeyCode::Up => {
            state.pan_by_cells(0, 4);
            false
        }
        KeyCode::Down => {
            state.pan_by_cells(0, -4);
            false
        }
        _ => false,
    }
}

fn handle_mouse_event(state: &mut CursorInfoState, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Right) => {
            state.drag_anchor = Some((mouse.column, mouse.row));
            state.follow_cursor = false;
        }
        MouseEventKind::Drag(MouseButton::Right) => {
            if let Some((previous_column, previous_row)) = state.drag_anchor {
                let delta_columns = i32::from(mouse.column) - i32::from(previous_column);
                let delta_rows = i32::from(mouse.row) - i32::from(previous_row);
                if delta_columns != 0 || delta_rows != 0 {
                    state.pan_by_cells(delta_columns, delta_rows);
                    state.drag_anchor = Some((mouse.column, mouse.row));
                }
            }
        }
        MouseEventKind::Up(MouseButton::Right) => state.drag_anchor = None,
        MouseEventKind::ScrollUp => state.zoom_in(),
        MouseEventKind::ScrollDown => state.zoom_out(),
        _ => {}
    }
}

struct CursorInfoFrameData {
    logical_width: u32,
    logical_height: u32,
    colors: RgbaImage,
}

pub(crate) struct CursorInfoVirtualSession {
    terminal: Terminal<TestBackend>,
    state: CursorInfoState,
    display: Arc<TerminalDisplayState>,
    visible_text: String,
    should_close: bool,
    repaint_requested: bool,
    performance: TerminalPerformanceSnapshot,
}

impl CursorInfoVirtualSession {
    pub(crate) fn new(config: CursorInfoConfig) -> eyre::Result<Self> {
        let initial_snapshot = capture_snapshot()?;
        let state = CursorInfoState::new(config, initial_snapshot.cursor.hotspot);
        let terminal = unwrap_infallible(Terminal::new(TestBackend::new(80, 24)));
        let mut session = Self {
            terminal,
            state,
            display: Arc::new(TerminalDisplayState::default()),
            visible_text: String::new(),
            should_close: false,
            repaint_requested: true,
            performance: TerminalPerformanceSnapshot::default(),
        };
        session.render_snapshot(&initial_snapshot);
        Ok(session)
    }

    pub(crate) fn rows(&self) -> u16 {
        unwrap_infallible(self.terminal.size()).height
    }

    pub(crate) fn cached_display_state(&self) -> Arc<TerminalDisplayState> {
        Arc::clone(&self.display)
    }

    pub(crate) fn take_repaint_requested(&mut self) -> bool {
        mem::take(&mut self.repaint_requested)
    }

    pub(crate) fn resize(&mut self, layout: TerminalLayout) {
        let (cols, rows) = layout.grid_size();
        let size = unwrap_infallible(self.terminal.size());
        if size.width == cols && size.height == rows {
            return;
        }

        self.terminal.backend_mut().resize(cols, rows);
        self.repaint_requested = true;
    }

    pub(crate) fn pump(&mut self) -> PumpResult {
        PumpResult {
            should_close: self.should_close,
        }
    }

    pub(crate) fn poll_output(&mut self) -> eyre::Result<PollPtyOutputResult> {
        let snapshot = capture_snapshot()?;
        let queued_output = self.render_snapshot(&snapshot);
        Ok(PollPtyOutputResult {
            queued_output,
            should_close: self.should_close,
        })
    }

    pub(crate) fn handle_char(&mut self, code_unit: u32) -> bool {
        let Some(character) = char::from_u32(code_unit) else {
            return false;
        };

        let key = KeyEvent::from(KeyCode::Char(character));
        let cursor_hotspot = self.state.viewport_center;
        let close = handle_key_event(&mut self.state, key, cursor_hotspot);
        self.should_close |= close;
        self.repaint_requested = true;
        true
    }

    pub(crate) fn handle_key_event(&mut self, vkey: u32, is_release: bool) -> bool {
        if is_release {
            return false;
        }

        let key_code = match vkey {
            code if code == u32::from(VK_ESCAPE.0) => Some(KeyCode::Esc),
            code if code == u32::from(VK_LEFT.0) => Some(KeyCode::Left),
            code if code == u32::from(VK_RIGHT.0) => Some(KeyCode::Right),
            code if code == u32::from(VK_UP.0) => Some(KeyCode::Up),
            code if code == u32::from(VK_DOWN.0) => Some(KeyCode::Down),
            _ => None,
        };
        let Some(key_code) = key_code else {
            return false;
        };

        let cursor_hotspot = self.state.viewport_center;
        let close = handle_key_event(&mut self.state, KeyEvent::from(key_code), cursor_hotspot);
        self.should_close |= close;
        self.repaint_requested = true;
        true
    }

    pub(crate) fn handle_mouse_wheel(&mut self, scroll_up: bool) -> bool {
        if scroll_up {
            self.state.zoom_in();
        } else {
            self.state.zoom_out();
        }
        self.repaint_requested = true;
        true
    }

    pub(crate) fn visible_text(&self) -> String {
        self.visible_text.clone()
    }

    pub(crate) fn viewport_metrics(&self) -> TerminalViewportMetrics {
        let visible = u64::from(self.rows().max(1));
        TerminalViewportMetrics {
            total: visible,
            offset: 0,
            visible,
            scrollback: 0,
        }
    }

    fn render_snapshot(&mut self, snapshot: &CursorInfoSnapshot) -> bool {
        if self.state.follow_cursor {
            self.state.viewport_center = snapshot.cursor.hotspot;
        }

        let size = unwrap_infallible(self.terminal.size());
        let terminal_area = Rect::new(0, 0, size.width, size.height);
        let frame_data = build_frame_data(&self.state, snapshot)
            .expect("cursor-info virtual session should build frame data");
        unwrap_infallible(self.terminal.draw(|frame| {
            draw_cursor_info_frame(frame, &mut self.state, snapshot, &frame_data);
        }));
        self.state.last_frame_started_at = Instant::now();

        let next_display = Arc::new(test_backend_to_terminal_display(
            self.terminal.backend(),
            terminal_area,
        ));
        let changed = self.display.as_ref() != next_display.as_ref();
        if changed {
            self.performance.display_publications =
                self.performance.display_publications.saturating_add(1);
            self.performance.dirty_rows_published = self
                .performance
                .dirty_rows_published
                .saturating_add(u64::try_from(next_display.rows.len()).unwrap_or(u64::MAX));
            self.performance.max_dirty_rows_published = self
                .performance
                .max_dirty_rows_published
                .max(next_display.rows.len());
            self.display = next_display;
            self.visible_text = test_backend_visible_text(self.terminal.backend());
            self.repaint_requested = true;
        }
        changed
    }
}

fn unwrap_infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

fn build_frame_data(
    state: &CursorInfoState,
    snapshot: &CursorInfoSnapshot,
) -> eyre::Result<CursorInfoFrameData> {
    let canvas_width = u32::from(state.last_canvas_area.width.max(1));
    let canvas_height = u32::from(state.last_canvas_area.height.max(1));
    let (pixels_per_cell_x, pixels_per_cell_y) = state.pixel_size.pixels_per_cell();
    let logical_width = canvas_width.saturating_mul(u32::from(pixels_per_cell_x));
    let logical_height = canvas_height.saturating_mul(u32::from(pixels_per_cell_y));
    let viewport = viewport_rect(
        state.viewport_center,
        logical_width,
        logical_height,
        state.scale,
    );

    let desktop = if matches!(
        state.mode,
        CursorInfoRenderMode::Desktop | CursorInfoRenderMode::Overlay
    ) {
        Some(capture_desktop_region(
            viewport,
            logical_width,
            logical_height,
        )?)
    } else {
        None
    };

    let mut colors = RgbaImage::new(logical_width, logical_height);
    for y in 0..logical_height {
        for x in 0..logical_width {
            let desktop_x = viewport.left()
                + i32::try_from(x).unwrap_or_default() * state.scale
                + (state.scale / 2);
            let desktop_y = viewport.top()
                + i32::try_from(y).unwrap_or_default() * state.scale
                + (state.scale / 2);
            let point = ScreenPoint::new(desktop_x, desktop_y);
            let semantic = semantic_class_for_point(snapshot, point, state.scale);
            let base = desktop
                .as_ref()
                .map_or(Rgba([0, 0, 0, 255]), |image| *image.get_pixel(x, y));
            let final_color = match state.mode {
                CursorInfoRenderMode::Mask => semantic_class_color(semantic),
                CursorInfoRenderMode::Desktop => base,
                CursorInfoRenderMode::Overlay => {
                    blend_rgba(base, semantic_class_color(semantic), 55)
                }
            };
            colors.put_pixel(x, y, final_color);
        }
    }

    Ok(CursorInfoFrameData {
        logical_width,
        logical_height,
        colors,
    })
}

fn render_legend(buf: &mut Buffer, area: Rect, snapshot: &CursorInfoSnapshot) {
    let mut lines = vec![
        legend_line(SemanticClass::CursorHotspot, "cursor hotspot"),
        legend_line(SemanticClass::CursorMask, "cursor mask"),
        legend_line(SemanticClass::Tooltip, "tooltip"),
        legend_line(SemanticClass::TeamyWindow, "Teamy window"),
        legend_line(SemanticClass::Taskbar, "taskbar"),
        legend_line(SemanticClass::ForeignWindow, "other window"),
    ];

    for monitor in &snapshot.monitors {
        lines.push(legend_line(
            SemanticClass::DesktopMonitor(monitor.index),
            &format!("desktop monitor {}", monitor.index + 1),
        ));
    }

    lines.push(legend_line(
        SemanticClass::OutsideAllMonitors,
        "outside all monitors",
    ));

    Paragraph::new(lines)
        .block(Block::bordered().title("Legend"))
        .render(area, buf);
}

fn legend_line(class: SemanticClass, label: &str) -> Line<'static> {
    let swatch = Span::styled(
        "  ",
        Style::default().bg(ratatui_color(semantic_class_color(class))),
    );
    let text = Span::raw(format!(" {label}"));
    Line::from(vec![swatch, text])
}

fn render_info(
    buf: &mut Buffer,
    area: Rect,
    state: &CursorInfoState,
    snapshot: &CursorInfoSnapshot,
) {
    let client_cursor_text = snapshot
        .windows
        .iter()
        .find(|window| {
            window.kind == ObservedWindowKind::Teamy
                && window.rect.contains(snapshot.cursor.hotspot)
        })
        .map_or_else(
            || "client-cursor=(n/a)".to_owned(),
            |window| {
                format!(
                    "client-cursor=({}, {})",
                    snapshot.cursor.hotspot.x_px() - window.rect.left(),
                    snapshot.cursor.hotspot.y_px() - window.rect.top()
                )
            },
        );

    let text = vec![
        Line::from(Span::raw(format!("mode={}", state.mode.label())).cyan()),
        Line::from(vec![
            Span::raw(format!("scale={} ", state.scale)).yellow(),
            Span::raw(format!("pixel={}", state.pixel_size.label())).green(),
        ]),
        Line::from(Span::raw(format!("cursor={}", snapshot.cursor_name)).magenta()),
        Line::from(format!(
            "cursor-pos=({}, {})",
            snapshot.cursor.hotspot.x_px(),
            snapshot.cursor.hotspot.y_px(),
        )),
        Line::from(client_cursor_text),
        Line::from(format!(
            "frame-age-ms={}",
            state.last_frame_started_at.elapsed().as_millis()
        )),
    ];
    Paragraph::new(text)
        .block(Block::bordered().title("Info"))
        .render(area, buf);
}

fn render_canvas(
    buf: &mut Buffer,
    area: Rect,
    pixel_size: PixelSize,
    frame_data: &CursorInfoFrameData,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (_, pixels_per_cell_y) = pixel_size.pixels_per_cell();
    for row in 0..area.height {
        for column in 0..area.width {
            let logical_x = u32::from(column);
            let logical_y = u32::from(row) * u32::from(pixels_per_cell_y);
            let top = *frame_data.colors.get_pixel(
                logical_x.min(frame_data.logical_width.saturating_sub(1)),
                logical_y.min(frame_data.logical_height.saturating_sub(1)),
            );
            let bottom = *frame_data.colors.get_pixel(
                logical_x.min(frame_data.logical_width.saturating_sub(1)),
                (logical_y + 1).min(frame_data.logical_height.saturating_sub(1)),
            );
            let cell = &mut buf[(area.x + column, area.y + row)];
            if top == bottom {
                cell.set_symbol(" ")
                    .set_bg(ratatui_color(top))
                    .set_fg(Color::Reset);
            } else {
                cell.set_symbol("▀")
                    .set_fg(ratatui_color(top))
                    .set_bg(ratatui_color(bottom));
            }
        }
    }
}

fn capture_snapshot() -> eyre::Result<CursorInfoSnapshot> {
    let (cursor, cursor_name) = query_cursor_geometry()?;
    let monitors = enumerate_monitors()?;
    let current_monitor_index = monitors
        .iter()
        .find(|monitor| monitor.rect.contains(cursor.hotspot))
        .map(|monitor| monitor.index);
    let current_monitor_bounds = current_monitor_index
        .and_then(|index| monitors.iter().find(|monitor| monitor.index == index))
        .map_or_else(virtual_screen_rect, |monitor| monitor.rect);

    Ok(CursorInfoSnapshot {
        current_monitor_bounds,
        current_monitor_index,
        cursor,
        cursor_name,
        monitors,
        virtual_bounds: virtual_screen_rect(),
        windows: enumerate_visible_windows()?,
    })
}

fn query_cursor_geometry() -> eyre::Result<(CursorGeometry, &'static str)> {
    let mut cursor_info = CURSORINFO {
        cbSize: u32::try_from(std::mem::size_of::<CURSORINFO>())
            .expect("CURSORINFO size must fit in u32"),
        ..Default::default()
    };
    // Safety: `cursor_info` is valid writable storage for the current cursor snapshot.
    unsafe { GetCursorInfo(&raw mut cursor_info) }.wrap_err("failed to query cursor info")?;

    let hotspot = ScreenPoint::from_win32_point(cursor_info.ptScreenPos);
    let visibility = cursor_visibility(cursor_info.flags);
    let cursor_name = cursor_name(cursor_info.hCursor, visibility);
    if !should_query_cursor_icon(cursor_info.flags, cursor_info.hCursor) {
        return Ok((
            CursorGeometry {
                hotspot,
                rect: ScreenRect::new(
                    hotspot.x_px(),
                    hotspot.y_px(),
                    hotspot.x_px(),
                    hotspot.y_px(),
                ),
            },
            cursor_name,
        ));
    }

    let mut icon_info = ICONINFO::default();
    // Safety: the live cursor handle can be queried for icon metadata during this call.
    let icon_info_result = unsafe { GetIconInfo(cursor_info.hCursor.into(), &raw mut icon_info) };
    if icon_info_result.is_err() {
        return Ok((
            CursorGeometry {
                hotspot,
                rect: ScreenRect::new(
                    hotspot.x_px(),
                    hotspot.y_px(),
                    hotspot.x_px(),
                    hotspot.y_px(),
                ),
            },
            cursor_name,
        ));
    }

    let bitmap_handle = if icon_info.hbmColor.is_invalid() {
        icon_info.hbmMask
    } else {
        icon_info.hbmColor
    };
    let mut bitmap = BITMAP::default();
    // Safety: `bitmap` is valid writable storage for the selected bitmap metadata.
    unsafe {
        GetObjectW(
            bitmap_handle.into(),
            i32::try_from(std::mem::size_of::<BITMAP>()).expect("BITMAP size must fit in i32"),
            Some((&raw mut bitmap).cast()),
        )
    };

    let mut height = bitmap.bmHeight;
    if icon_info.hbmColor.is_invalid() {
        height /= 2;
    }
    let left = cursor_info.ptScreenPos.x - i32::try_from(icon_info.xHotspot).unwrap_or_default();
    let top = cursor_info.ptScreenPos.y - i32::try_from(icon_info.yHotspot).unwrap_or_default();

    // Safety: GetIconInfo returned bitmap handles that must be released after use.
    unsafe {
        let _ = DeleteObject(icon_info.hbmMask.into());
    }
    if !icon_info.hbmColor.is_invalid() {
        // Safety: the optional color bitmap handle came from GetIconInfo and must be released.
        unsafe {
            let _ = DeleteObject(icon_info.hbmColor.into());
        }
    }

    Ok((
        CursorGeometry {
            hotspot,
            rect: ScreenRect::new(left, top, left + bitmap.bmWidth, top + height),
        },
        cursor_name,
    ))
}

const fn cursor_visibility(flags: CURSORINFO_FLAGS) -> CursorVisibility {
    if (flags.0 & CURSOR_SUPPRESSED.0) != 0 {
        CursorVisibility::Suppressed
    } else if (flags.0 & CURSOR_SHOWING.0) != 0 {
        CursorVisibility::Visible
    } else {
        CursorVisibility::Hidden
    }
}

fn should_query_cursor_icon(flags: CURSORINFO_FLAGS, cursor: HCURSOR) -> bool {
    matches!(cursor_visibility(flags), CursorVisibility::Visible) && !cursor.is_invalid()
}

fn cursor_name(cursor: HCURSOR, visibility: CursorVisibility) -> &'static str {
    match visibility {
        CursorVisibility::Hidden => "hidden",
        CursorVisibility::Suppressed => "suppressed",
        CursorVisibility::Visible => standard_cursor_name(cursor).unwrap_or("custom"),
    }
}

fn standard_cursor_name(cursor: HCURSOR) -> Option<&'static str> {
    [
        (IDC_ARROW, "arrow"),
        (IDC_IBEAM, "ibeam"),
        (IDC_WAIT, "wait"),
        (IDC_APPSTARTING, "appstarting"),
        (IDC_CROSS, "cross"),
        (IDC_HAND, "hand"),
        (IDC_HELP, "help"),
        (IDC_NO, "no"),
        (IDC_SIZEALL, "sizeall"),
        (IDC_SIZENESW, "sizenesw"),
        (IDC_SIZENS, "sizens"),
        (IDC_SIZENWSE, "sizenwse"),
        (IDC_SIZEWE, "sizewe"),
        (IDC_UPARROW, "uparrow"),
    ]
    .into_iter()
    .find_map(|(identifier, label)| {
        (load_standard_cursor(identifier) == Some(cursor)).then_some(label)
    })
}

fn load_standard_cursor(identifier: windows::core::PCWSTR) -> Option<HCURSOR> {
    // Safety: loading shared system cursor resources by identifier is a read-only OS query.
    unsafe { LoadCursorW(None, identifier) }
        .ok()
        .filter(|cursor| !cursor.is_invalid())
}

fn virtual_screen_rect() -> ScreenRect {
    let left = system_metric(SM_XVIRTUALSCREEN);
    let top = system_metric(SM_YVIRTUALSCREEN);
    let width = system_metric(SM_CXVIRTUALSCREEN);
    let height = system_metric(SM_CYVIRTUALSCREEN);
    ScreenRect::new(left, top, left + width, top + height)
}

fn system_metric(index: windows::Win32::UI::WindowsAndMessaging::SYSTEM_METRICS_INDEX) -> i32 {
    // Safety: querying a system metric by constant index is a read-only OS call.
    unsafe { GetSystemMetrics(index) }
}

fn enumerate_monitors() -> eyre::Result<Vec<ObservedMonitor>> {
    unsafe extern "system" fn enumerate(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        // Safety: EnumDisplayMonitors passes back the same collector pointer for the callback duration.
        let monitors = unsafe { &mut *(lparam.0 as *mut Vec<ScreenRect>) };
        let mut info = MONITORINFO {
            cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>())
                .expect("MONITORINFO size must fit in u32"),
            ..Default::default()
        };

        // Safety: `info` is valid writable storage for the monitor metadata query.
        if unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
            monitors.push(ScreenRect::from_win32_rect(info.rcMonitor));
        }

        BOOL(1)
    }

    let mut monitor_rects: Vec<ScreenRect> = Vec::new();
    // Safety: the callback only appends into `monitor_rects`, which stays alive for the full call.
    let succeeded = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enumerate),
            LPARAM((&raw mut monitor_rects).cast::<()>() as isize),
        )
    };
    if !succeeded.as_bool() {
        eyre::bail!("failed to enumerate monitors for cursor-info")
    }

    monitor_rects.sort_by_key(|rect| (rect.left(), rect.top()));
    Ok(monitor_rects
        .into_iter()
        .enumerate()
        .map(|(index, rect)| ObservedMonitor { rect, index })
        .collect())
}

fn enumerate_visible_windows() -> eyre::Result<Vec<ObservedWindow>> {
    unsafe extern "system" fn enumerate(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // Safety: EnumWindows passes back the same collector pointer for the callback duration.
        let windows = unsafe { &mut *(lparam.0 as *mut Vec<ObservedWindow>) };
        // Safety: `hwnd` is a live top-level window handle under enumeration.
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return BOOL(1);
        }

        let mut rect = RECT::default();
        // Safety: `rect` is valid writable storage for the enumerated window bounds.
        if unsafe { GetWindowRect(hwnd, &raw mut rect) }.is_err() {
            return BOOL(1);
        }
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return BOOL(1);
        }

        let mut class_name_buffer = [0_u16; 256];
        // Safety: `class_name_buffer` is writable storage for the class name query.
        let class_len = unsafe { GetClassNameW(hwnd, &mut class_name_buffer) };
        let class_name = String::from_utf16_lossy(
            &class_name_buffer[..usize::try_from(class_len.max(0)).unwrap_or_default()],
        );

        if DESKTOP_WINDOW_CLASS_NAMES.contains(&class_name.as_str()) {
            return BOOL(1);
        }

        let kind = if class_name == TOOLTIP_WINDOW_CLASS_NAME {
            ObservedWindowKind::Tooltip
        } else if matches!(
            class_name.as_str(),
            PRIMARY_TASKBAR_WINDOW_CLASS_NAME | SECONDARY_TASKBAR_WINDOW_CLASS_NAME
        ) {
            ObservedWindowKind::Taskbar
        } else if matches!(
            class_name.as_str(),
            TEAMY_TERMINAL_WINDOW_CLASS_NAME
                | TEAMY_SCENE_WINDOW_CLASS_NAME
                | TEAMY_BENCHMARK_WINDOW_CLASS_NAME
        ) {
            ObservedWindowKind::Teamy
        } else {
            ObservedWindowKind::Foreign
        };

        windows.push(ObservedWindow {
            rect: ScreenRect::from_win32_rect(rect),
            kind,
        });
        BOOL(1)
    }

    let mut windows = Vec::new();
    // Safety: the callback only appends into `windows`, which stays alive for the full call.
    unsafe {
        EnumWindows(
            Some(enumerate),
            LPARAM((&raw mut windows).cast::<()>() as isize),
        )
    }
    .wrap_err("failed to enumerate windows for cursor-info")?;
    Ok(windows)
}

fn viewport_rect(
    center: ScreenPoint,
    logical_width: u32,
    logical_height: u32,
    scale: i32,
) -> ScreenRect {
    let width = i32::try_from(logical_width)
        .unwrap_or(i32::MAX)
        .saturating_mul(scale.max(MIN_SCALE));
    let height = i32::try_from(logical_height)
        .unwrap_or(i32::MAX)
        .saturating_mul(scale.max(MIN_SCALE));
    let left = center.x_px() - (width / 2);
    let top = center.y_px() - (height / 2);
    ScreenRect::new(left, top, left + width, top + height)
}

fn capture_desktop_region(
    region: ScreenRect,
    output_width: u32,
    output_height: u32,
) -> eyre::Result<RgbaImage> {
    if output_width == 0 || output_height == 0 {
        return Ok(RgbaImage::new(output_width, output_height));
    }

    // Safety: acquiring the desktop DC is required for screen capture and does not transfer ownership.
    let screen_dc = unsafe { GetDC(None) };
    if screen_dc.is_invalid() {
        eyre::bail!("failed to acquire screen device context")
    }
    // Safety: creating a compatible memory DC from a valid screen DC is the standard capture path.
    let memory_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
    if memory_dc.is_invalid() {
        // Safety: release the borrowed screen DC before returning.
        unsafe { ReleaseDC(None, screen_dc) };
        eyre::bail!("failed to create compatible memory device context")
    }

    // Safety: allocating a bitmap compatible with the screen DC is required for the capture target.
    let bitmap = unsafe {
        CreateCompatibleBitmap(
            screen_dc,
            i32::try_from(output_width).unwrap_or(i32::MAX),
            i32::try_from(output_height).unwrap_or(i32::MAX),
        )
    };
    if bitmap.is_invalid() {
        // Safety: the memory DC was created in this function and must be released on failure.
        unsafe {
            let _ = DeleteDC(memory_dc);
        };
        // Safety: release the borrowed screen DC before returning.
        unsafe { ReleaseDC(None, screen_dc) };
        eyre::bail!("failed to create capture bitmap")
    }

    // Safety: selecting the destination bitmap into the compatible DC prepares it for capture.
    unsafe { SelectObject(memory_dc, bitmap.into()) };
    // Safety: both DCs and the requested region dimensions are valid for this desktop blit.
    unsafe {
        StretchBlt(
            memory_dc,
            0,
            0,
            i32::try_from(output_width).unwrap_or(i32::MAX),
            i32::try_from(output_height).unwrap_or(i32::MAX),
            Some(screen_dc),
            region.left(),
            region.top(),
            region.width(),
            region.height(),
            SRCCOPY,
        )
    }
    .ok()
    .wrap_err("failed to capture desktop region")?;

    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>())
                .expect("BITMAPINFOHEADER size must fit in u32"),
            biWidth: i32::try_from(output_width).unwrap_or(i32::MAX),
            biHeight: -i32::try_from(output_height).unwrap_or(i32::MAX),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD::default(); 1],
    };
    let mut data =
        vec![
            0_u8;
            usize::try_from(output_width.saturating_mul(output_height).saturating_mul(4))
                .unwrap_or_default()
        ];
    // Safety: the bitmap, output buffer, and BITMAPINFO all remain valid for the entire transfer.
    let received = unsafe {
        GetDIBits(
            memory_dc,
            bitmap,
            0,
            output_height,
            Some(data.as_mut_ptr().cast()),
            &raw mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };

    // Safety: the capture bitmap was allocated in this function and must be deleted after use.
    unsafe {
        let _ = DeleteObject(bitmap.into());
    };
    // Safety: the compatible memory DC was created in this function and must be deleted after use.
    unsafe {
        let _ = DeleteDC(memory_dc);
    };
    // Safety: release the borrowed screen DC once the capture is complete.
    unsafe { ReleaseDC(None, screen_dc) };

    if received == 0 {
        eyre::bail!("GetDIBits returned no desktop capture data")
    }

    bgra_to_rgba(&mut data);
    RgbaImage::from_vec(output_width, output_height, data)
        .ok_or_else(|| eyre::eyre!("desktop capture returned invalid image data"))
}

fn bgra_to_rgba(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
}

fn semantic_class_for_point(
    snapshot: &CursorInfoSnapshot,
    point: ScreenPoint,
    sample_scale: i32,
) -> SemanticClass {
    let Some(monitor) = monitor_for_point(snapshot, point) else {
        return SemanticClass::OutsideAllMonitors;
    };

    if is_hotspot_marker(point, snapshot.cursor.hotspot, sample_scale) {
        return SemanticClass::CursorHotspot;
    }
    if snapshot.cursor.rect.contains(point) {
        return SemanticClass::CursorMask;
    }

    if let Some(window_kind) = topmost_window_kind_at_point(snapshot, point) {
        return match window_kind {
            ObservedWindowKind::Tooltip => SemanticClass::Tooltip,
            ObservedWindowKind::Teamy => SemanticClass::TeamyWindow,
            ObservedWindowKind::Taskbar => SemanticClass::Taskbar,
            ObservedWindowKind::Foreign => SemanticClass::ForeignWindow,
        };
    }

    SemanticClass::DesktopMonitor(monitor.index)
}

fn monitor_for_point(snapshot: &CursorInfoSnapshot, point: ScreenPoint) -> Option<ObservedMonitor> {
    snapshot
        .monitors
        .iter()
        .copied()
        .find(|monitor| monitor.rect.contains(point))
}

fn is_hotspot_marker(point: ScreenPoint, hotspot: ScreenPoint, sample_scale: i32) -> bool {
    let radius = (sample_scale.max(1) / 2).max(1);
    (point.x_px() - hotspot.x_px()).abs() <= radius
        && (point.y_px() - hotspot.y_px()).abs() <= radius
}

fn topmost_window_kind_at_point(
    snapshot: &CursorInfoSnapshot,
    point: ScreenPoint,
) -> Option<ObservedWindowKind> {
    snapshot
        .windows
        .iter()
        .filter(|window| window.rect.contains(point))
        .min_by_key(|window| {
            (
                window_kind_priority(window.kind),
                i64::from(window.rect.width()) * i64::from(window.rect.height()),
            )
        })
        .map(|window| window.kind)
}

const fn window_kind_priority(kind: ObservedWindowKind) -> u8 {
    match kind {
        ObservedWindowKind::Tooltip => 0,
        ObservedWindowKind::Teamy => 1,
        ObservedWindowKind::Taskbar => 2,
        ObservedWindowKind::Foreign => 3,
    }
}

fn semantic_class_color(class: SemanticClass) -> Rgba<u8> {
    match class {
        SemanticClass::OutsideAllMonitors => Rgba([24, 24, 24, 255]),
        SemanticClass::DesktopMonitor(index) => monitor_color(index),
        SemanticClass::ForeignWindow => Rgba([48, 160, 108, 255]),
        SemanticClass::Taskbar => Rgba([214, 120, 32, 255]),
        SemanticClass::TeamyWindow => Rgba([64, 128, 255, 255]),
        SemanticClass::Tooltip => Rgba([245, 245, 245, 255]),
        SemanticClass::CursorMask => Rgba([255, 230, 64, 255]),
        SemanticClass::CursorHotspot => Rgba([255, 0, 0, 255]),
    }
}

fn monitor_color(index: usize) -> Rgba<u8> {
    const COLORS: [Rgba<u8>; 6] = [
        Rgba([54, 196, 120, 255]),
        Rgba([122, 88, 214, 255]),
        Rgba([32, 154, 214, 255]),
        Rgba([190, 96, 48, 255]),
        Rgba([196, 76, 140, 255]),
        Rgba([118, 138, 34, 255]),
    ];
    COLORS[index % COLORS.len()]
}

fn blend_rgba(base: Rgba<u8>, overlay: Rgba<u8>, overlay_alpha_percent: u16) -> Rgba<u8> {
    let blend_channel = |base: u8, overlay: u8| {
        let base_weight = 100_u16.saturating_sub(overlay_alpha_percent);
        let blended =
            (u16::from(base) * base_weight + u16::from(overlay) * overlay_alpha_percent + 50) / 100;
        u8::try_from(blended).expect("blended color channel must remain within u8 range")
    };
    Rgba([
        blend_channel(base[0], overlay[0]),
        blend_channel(base[1], overlay[1]),
        blend_channel(base[2], overlay[2]),
        255,
    ])
}

fn ratatui_color(pixel: Rgba<u8>) -> Color {
    Color::Rgb(pixel[0], pixel[1], pixel[2])
}

trait ScreenPointPixels {
    fn x_px(self) -> i32;
    fn y_px(self) -> i32;
}

impl ScreenPointPixels for ScreenPoint {
    fn x_px(self) -> i32 {
        self.to_win32_point()
            .expect("screen point should remain integral in cursor-info")
            .x
    }

    fn y_px(self) -> i32 {
        self.to_win32_point()
            .expect("screen point should remain integral in cursor-info")
            .y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_mode_cycles_in_expected_order() {
        assert_eq!(
            CursorInfoRenderMode::Mask.next(),
            CursorInfoRenderMode::Desktop
        );
        assert_eq!(
            CursorInfoRenderMode::Desktop.next(),
            CursorInfoRenderMode::Overlay
        );
        assert_eq!(
            CursorInfoRenderMode::Overlay.next(),
            CursorInfoRenderMode::Mask
        );
    }

    #[test]
    fn blend_rgba_interpolates_channels() {
        let blended = blend_rgba(Rgba([0, 0, 0, 255]), Rgba([200, 100, 50, 255]), 50);
        assert_eq!(blended, Rgba([100, 50, 25, 255]));
    }

    #[test]
    fn semantic_class_prefers_teamy_over_foreign_window_overlap() {
        let snapshot = CursorInfoSnapshot {
            cursor: CursorGeometry {
                hotspot: ScreenPoint::new(20, 20),
                rect: ScreenRect::new(18, 18, 22, 22),
            },
            cursor_name: "arrow",
            virtual_bounds: ScreenRect::new(0, 0, 100, 100),
            current_monitor_bounds: ScreenRect::new(0, 0, 100, 100),
            current_monitor_index: Some(0),
            monitors: vec![ObservedMonitor {
                rect: ScreenRect::new(0, 0, 100, 100),
                index: 0,
            }],
            windows: vec![
                ObservedWindow {
                    rect: ScreenRect::new(0, 0, 100, 100),
                    kind: ObservedWindowKind::Foreign,
                },
                ObservedWindow {
                    rect: ScreenRect::new(10, 10, 60, 60),
                    kind: ObservedWindowKind::Teamy,
                },
            ],
        };

        assert_eq!(
            semantic_class_for_point(&snapshot, ScreenPoint::new(30, 30), 1),
            SemanticClass::TeamyWindow
        );
    }

    #[test]
    fn semantic_class_uses_monitor_desktop_for_uncovered_point() {
        let snapshot = CursorInfoSnapshot {
            cursor: CursorGeometry {
                hotspot: ScreenPoint::new(10, 10),
                rect: ScreenRect::new(8, 8, 12, 12),
            },
            cursor_name: "arrow",
            virtual_bounds: ScreenRect::new(0, 0, 200, 100),
            current_monitor_bounds: ScreenRect::new(0, 0, 100, 100),
            current_monitor_index: Some(0),
            monitors: vec![
                ObservedMonitor {
                    rect: ScreenRect::new(0, 0, 100, 100),
                    index: 0,
                },
                ObservedMonitor {
                    rect: ScreenRect::new(100, 0, 200, 100),
                    index: 1,
                },
            ],
            windows: Vec::new(),
        };

        assert_eq!(
            semantic_class_for_point(&snapshot, ScreenPoint::new(40, 40), 1),
            SemanticClass::DesktopMonitor(0)
        );
        assert_eq!(
            semantic_class_for_point(&snapshot, ScreenPoint::new(140, 40), 1),
            SemanticClass::DesktopMonitor(1)
        );
    }

    #[test]
    fn semantic_class_marks_hotspot_for_nearby_sample() {
        let snapshot = CursorInfoSnapshot {
            cursor: CursorGeometry {
                hotspot: ScreenPoint::new(50, 50),
                rect: ScreenRect::new(48, 48, 52, 52),
            },
            cursor_name: "arrow",
            virtual_bounds: ScreenRect::new(0, 0, 100, 100),
            current_monitor_bounds: ScreenRect::new(0, 0, 100, 100),
            current_monitor_index: Some(0),
            monitors: vec![ObservedMonitor {
                rect: ScreenRect::new(0, 0, 100, 100),
                index: 0,
            }],
            windows: Vec::new(),
        };

        assert_eq!(
            semantic_class_for_point(&snapshot, ScreenPoint::new(52, 52), 6),
            SemanticClass::CursorHotspot
        );
    }

    #[test]
    fn hidden_cursor_state_skips_icon_query() {
        assert!(!should_query_cursor_icon(
            CURSORINFO_FLAGS(0),
            HCURSOR::default()
        ));
        assert!(!should_query_cursor_icon(
            CURSOR_SUPPRESSED,
            HCURSOR::default()
        ));
    }

    #[test]
    fn visible_cursor_requires_valid_handle_for_icon_query() {
        assert!(!should_query_cursor_icon(
            CURSOR_SHOWING,
            HCURSOR::default()
        ));
    }
}
