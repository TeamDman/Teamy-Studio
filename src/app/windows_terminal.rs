use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
#[cfg(feature = "tracy")]
use tracing::debug_span;
use tracing::trace;

use eyre::Context;
use libghostty_vt::key;
use libghostty_vt::render::{CellIterator, CursorVisualStyle, RenderState, RowIterator};
use libghostty_vt::screen::RowSemanticPrompt;
use libghostty_vt::style::RgbColor;
use libghostty_vt::terminal::{Point, PointCoordinate, ScrollViewport};
use libghostty_vt::{Terminal, TerminalOptions};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tracing::{debug, info, info_span, instrument};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Console::{
    CAPSLOCK_ON, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, NUMLOCK_ON, RIGHT_ALT_PRESSED,
    RIGHT_CTRL_PRESSED, SHIFT_PRESSED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::paths::AppHome;

use super::spatial::{ClientRect, TerminalCellPoint};

pub const DRAG_STRIP_HEIGHT: i32 = 76;
pub const WINDOW_PADDING: i32 = 18;
pub const POLL_TIMER_ID: usize = 1;
pub const POLL_INTERVAL_MS: u32 = 16;
pub const TERMINAL_WORKER_WAKE_MESSAGE: u32 = WM_APP + 1;

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const MAX_SCROLLBACK: usize = 20_000;
const PTY_READ_BUFFER_BYTES: usize = 128 * 1_024;
const PTY_READ_CHANNEL_CAPACITY: usize = 8;
const TERMINAL_OUTPUT_SLICE_BYTES: usize = 256;
const TERMINAL_OUTPUT_QUEUE_SOFT_LIMIT_BYTES: usize = 64 * 1_024;
const TERMINAL_DISPLAY_PUBLISH_INTERVAL: Duration = Duration::from_millis(16);
const TERMINAL_WORKER_IDLE_TIMEOUT: Duration = Duration::from_millis(1);
const TERMINAL_WORKER_PUMP_TIME_BUDGET: Duration = Duration::from_millis(2);
const CELL_PANEL_GAP: i32 = 14;
const SIDECAR_WIDTH: i32 = 86;
const RESULT_PANEL_HEIGHT: i32 = 152;
const MIN_CODE_PANEL_HEIGHT: i32 = 180;
const PLUS_BUTTON_SIZE: i32 = 42;
const SIDECAR_BUTTON_SIZE: i32 = 34;
const SIDECAR_BUTTON_GAP: i32 = 12;
const TERMINAL_SCROLLBAR_WIDTH: i32 = 16;
const TERMINAL_SCROLLBAR_GAP: i32 = 8;
const WIN32_INPUT_MODE_ENABLE: &[u8] = b"\x1b[?9001h";
const WIN32_INPUT_MODE_DISABLE: &[u8] = b"\x1b[?9001l";
const CTRL_D_EOF: u8 = 0x04;
const CTRL_D_EXIT_COMMAND: &[u8] = b"exit\r";
const OSC_133_PREFIX: &[u8] = b"\x1b]133;";

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

pub struct PollPtyOutputResult {
    pub queued_output: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalDisplayScrollbar {
    pub total: u64,
    pub offset: u64,
    pub visible: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalDisplayState {
    pub backgrounds: Vec<TerminalDisplayBackground>,
    pub glyphs: Vec<TerminalDisplayGlyph>,
    pub cursor: Option<TerminalDisplayCursor>,
    pub scrollbar: Option<TerminalDisplayScrollbar>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalViewportMetrics {
    pub total: u64,
    pub offset: u64,
    pub visible: u64,
    pub scrollback: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalTextRow {
    row: i32,
    cells: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalSelectionMode {
    Linear,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSelection {
    anchor: TerminalCellPoint,
    focus: TerminalCellPoint,
    mode: TerminalSelectionMode,
}

impl TerminalSelection {
    #[must_use]
    pub fn new(
        anchor: TerminalCellPoint,
        focus: TerminalCellPoint,
        mode: TerminalSelectionMode,
    ) -> Self {
        Self {
            anchor,
            focus,
            mode,
        }
    }

    #[must_use]
    pub fn mode(self) -> TerminalSelectionMode {
        self.mode
    }

    #[must_use]
    pub fn contains(self, cell: TerminalCellPoint) -> bool {
        match self.mode {
            TerminalSelectionMode::Linear => {
                let (start, end) = ordered_linear_bounds(self.anchor, self.focus);
                linear_selection_contains(start, end, cell)
            }
            TerminalSelectionMode::Block => {
                let (left, top, right, bottom) = ordered_block_bounds(self.anchor, self.focus);
                (left..=right).contains(&cell.column()) && (top..=bottom).contains(&cell.row())
            }
        }
    }
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PromptInputState {
    #[default]
    Inactive,
    AwaitingPristine,
    AwaitingEdited,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SemanticPromptTracking {
    markers_observed: bool,
    at_shell_prompt: bool,
    input_state: PromptInputState,
}

pub struct TerminalSession {
    worker_tx: mpsc::Sender<TerminalWorkerRequest>,
    pending_updates: Arc<Mutex<VecDeque<TerminalWorkerUpdate>>>,
    wake_window: Arc<Mutex<Option<isize>>>,
    snapshot: TerminalSnapshot,
    worker_queued_output: bool,
    repaint_requested: bool,
    cached_display: TerminalDisplayState,
}

struct TerminalCore {
    terminal: Terminal<'static, 'static>,
    render_state: RenderState<'static>,
    key_encoder: key::Encoder<'static>,
    key_event: key::Event<'static>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send>,
    writer: Arc<Mutex<PtyWriter>>,
    reader: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    pending_output: VecDeque<u8>,
    cols: u16,
    rows: u16,
    repaint: RepaintState,
    input_trace: Vec<Vec<u8>>,
    suppressed_chars: VecDeque<SuppressedChar>,
    win32_input: Win32InputState,
    win32_input_mode_buffer: Vec<u8>,
    semantic_prompt_buffer: Vec<u8>,
    semantic_prompt: SemanticPromptTracking,
    cached_display: TerminalDisplayState,
    display_cache_dirty: bool,
    closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalSnapshot {
    cols: u16,
    rows: u16,
    pending_output_bytes: usize,
    closed: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum TerminalWorkerCommand {
    Resize(TerminalLayout),
    HandleChar {
        code_unit: u32,
        lparam: isize,
    },
    HandleKeyEvent {
        vkey: u32,
        lparam: isize,
        was_down: bool,
        is_release: bool,
        mods: key::Mods,
    },
    HandlePaste(String),
    SelectedText(TerminalSelection),
    VisibleText,
    ViewportMetrics,
    ViewportToScreenCell(TerminalCellPoint),
    ScrollViewportBy(isize),
    ScrollViewportToOffset(u64),
    VisibleDisplayStateWithSelection(Option<TerminalSelection>),
    CurrentKittyKeyboardFlags,
    Win32InputModeEnabled,
    TakeInputTrace,
    SemanticPromptState,
}

#[derive(Clone, Debug, PartialEq)]
enum TerminalWorkerUpdate {
    Snapshot(TerminalSnapshot),
    DisplayState(TerminalDisplayState),
    PtyOutputQueued,
    RepaintRequested,
    ChildExited,
}

struct TerminalWorkerRequest {
    command: TerminalWorkerCommand,
    reply_tx: mpsc::SyncSender<eyre::Result<TerminalWorkerResponse>>,
}

struct TerminalWorkerResponse {
    snapshot: TerminalSnapshot,
    payload: TerminalWorkerResponsePayload,
}

enum TerminalWorkerResponsePayload {
    Unit,
    Bool(bool),
    String(String),
    ViewportMetrics(TerminalViewportMetrics),
    ScreenCell(TerminalCellPoint),
    DisplayState(TerminalDisplayState),
    KittyKeyboardFlags(key::KittyKeyFlags),
    InputTrace(Vec<Vec<u8>>),
    SemanticPromptState((bool, bool, bool)),
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// behavior[impl window.appearance.code-panel.terminal-alignment]
    pub fn terminal_rect(self) -> ClientRect {
        self.terminal_viewport_rect()
    }

    #[must_use]
    pub fn terminal_viewport_rect(self) -> ClientRect {
        let code = self.code_panel_rect();
        let scrollbar = self.terminal_scrollbar_rect();
        ClientRect::new(
            code.left(),
            code.top(),
            (scrollbar.left() - TERMINAL_SCROLLBAR_GAP).max(code.left() + 1),
            code.bottom(),
        )
    }

    #[must_use]
    pub fn terminal_scrollbar_rect(self) -> ClientRect {
        let code = self.code_panel_rect();
        ClientRect::new(
            (code.right() - TERMINAL_SCROLLBAR_WIDTH).max(code.left() + 1),
            code.top(),
            code.right(),
            code.bottom(),
        )
    }

    #[must_use]
    pub fn grid_size(self) -> (u16, u16) {
        let rect = self.terminal_viewport_rect();
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
    pub fn new(app_home: &AppHome, working_dir: Option<&Path>) -> eyre::Result<Self> {
        let mut command = crate::shell_default::load_effective_command_builder(app_home)?;
        if let Some(working_dir) = working_dir {
            command.cwd(working_dir);
        }
        Self::new_with_command(command)
    }

    pub fn new_with_command(shell: CommandBuilder) -> eyre::Result<Self> {
        let (request_tx, request_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let pending_updates = Arc::new(Mutex::new(VecDeque::new()));
        let wake_window = Arc::new(Mutex::new(None));
        let pending_updates_for_bridge = Arc::clone(&pending_updates);
        let wake_window_for_bridge = Arc::clone(&wake_window);

        std::thread::Builder::new()
            .name("teamy-terminal-update-bridge".to_owned())
            .spawn(move || {
                while let Ok(update) = update_rx.recv() {
                    if let Ok(mut pending_updates) = pending_updates_for_bridge.lock() {
                        pending_updates.push_back(update);
                    }

                    let wake_target = wake_window_for_bridge
                        .lock()
                        .ok()
                        .and_then(|wake_window| *wake_window);
                    if let Some(raw_hwnd) = wake_target {
                        // Safety: this reconstructs the live window handle value previously stored by the UI thread and posts a message without dereferencing it.
                        let _ = unsafe {
                            PostMessageW(
                                Some(HWND(raw_hwnd as *mut core::ffi::c_void)),
                                TERMINAL_WORKER_WAKE_MESSAGE,
                                WPARAM(0),
                                LPARAM(0),
                            )
                        };
                    }
                }
            })
            .map_err(|error| {
                eyre::eyre!("failed to spawn terminal update bridge thread: {error}")
            })?;

        std::thread::Builder::new()
            .name("teamy-terminal-worker".to_owned())
            .spawn(move || {
                let startup_result = TerminalCore::new_with_command(shell).map(|core| {
                    let snapshot = core.snapshot();
                    (
                        TerminalWorkerRunner::new(core, request_rx, update_tx),
                        snapshot,
                    )
                });

                match startup_result {
                    Ok((mut runner, snapshot)) => {
                        let _ = startup_tx.send(Ok(snapshot));
                        runner.run();
                    }
                    Err(error) => {
                        let _ = startup_tx.send(Err(error));
                    }
                }
            })
            .map_err(|error| eyre::eyre!("failed to spawn terminal worker thread: {error}"))?;

        let snapshot = startup_rx.recv().map_err(|error| {
            eyre::eyre!("terminal worker failed to report startup state: {error}")
        })??;

        Ok(Self {
            worker_tx: request_tx,
            pending_updates,
            wake_window,
            snapshot,
            worker_queued_output: false,
            repaint_requested: false,
            cached_display: TerminalDisplayState::default(),
        })
    }

    pub fn set_wake_window(&mut self, hwnd: HWND) {
        if let Ok(mut wake_window) = self.wake_window.lock() {
            *wake_window = Some(hwnd.0 as isize);
        }
    }

    pub fn cols(&self) -> u16 {
        self.snapshot.cols
    }

    pub fn rows(&self) -> u16 {
        self.snapshot.rows
    }

    pub fn has_pending_output(&self) -> bool {
        self.snapshot.pending_output_bytes > 0
    }

    pub fn take_repaint_requested(&mut self) -> bool {
        self.drain_worker_updates();
        std::mem::take(&mut self.repaint_requested)
    }

    pub fn resize(&mut self, layout: TerminalLayout) -> eyre::Result<()> {
        let response = self.request(TerminalWorkerCommand::Resize(layout))?;
        match response.payload {
            TerminalWorkerResponsePayload::Unit => Ok(()),
            payload => Self::unexpected_response("Resize", payload),
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "keeps the existing TerminalSession API stable while the worker owns autonomous pumping"
    )]
    pub fn pump(&mut self) -> eyre::Result<PumpResult> {
        self.drain_worker_updates();
        Ok(PumpResult {
            should_close: self.snapshot.closed,
        })
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "keeps the existing TerminalSession API stable while the worker owns PTY polling"
    )]
    pub fn poll_pty_output(&mut self) -> eyre::Result<PollPtyOutputResult> {
        self.drain_worker_updates();
        let queued_output = std::mem::take(&mut self.worker_queued_output);
        Ok(PollPtyOutputResult {
            queued_output,
            should_close: self.snapshot.closed,
        })
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "keeps the existing TerminalSession API stable while the worker owns terminal mutation"
    )]
    pub fn pump_pending_output(&mut self) -> eyre::Result<PumpResult> {
        self.drain_worker_updates();
        Ok(PumpResult {
            should_close: self.snapshot.closed,
        })
    }

    pub fn handle_char(&mut self, code_unit: u32, lparam: isize) -> eyre::Result<bool> {
        let response = self.request(TerminalWorkerCommand::HandleChar { code_unit, lparam })?;
        match response.payload {
            TerminalWorkerResponsePayload::Bool(handled) => Ok(handled),
            payload => Self::unexpected_response("HandleChar", payload),
        }
    }

    pub fn handle_key_event(
        &mut self,
        vkey: u32,
        lparam: isize,
        was_down: bool,
        is_release: bool,
        mods: key::Mods,
    ) -> eyre::Result<bool> {
        let response = self.request(TerminalWorkerCommand::HandleKeyEvent {
            vkey,
            lparam,
            was_down,
            is_release,
            mods,
        })?;
        match response.payload {
            TerminalWorkerResponsePayload::Bool(handled) => Ok(handled),
            payload => Self::unexpected_response("HandleKeyEvent", payload),
        }
    }

    pub fn current_kitty_keyboard_flags(&self) -> eyre::Result<key::KittyKeyFlags> {
        let response = self.request_read_only(TerminalWorkerCommand::CurrentKittyKeyboardFlags)?;
        match response.payload {
            TerminalWorkerResponsePayload::KittyKeyboardFlags(flags) => Ok(flags),
            payload => Self::unexpected_response("CurrentKittyKeyboardFlags", payload),
        }
    }

    pub fn win32_input_mode_enabled(&self) -> bool {
        match self.request_read_only(TerminalWorkerCommand::Win32InputModeEnabled) {
            Ok(response) => match response.payload {
                TerminalWorkerResponsePayload::Bool(enabled) => enabled,
                _ => self.snapshot.closed,
            },
            Err(_) => self.snapshot.closed,
        }
    }

    pub fn handle_paste(&mut self, text: &str) -> eyre::Result<()> {
        let response = self.request(TerminalWorkerCommand::HandlePaste(text.to_owned()))?;
        match response.payload {
            TerminalWorkerResponsePayload::Unit => Ok(()),
            payload => Self::unexpected_response("HandlePaste", payload),
        }
    }

    pub fn selected_text(&mut self, selection: TerminalSelection) -> eyre::Result<String> {
        let response = self.request(TerminalWorkerCommand::SelectedText(selection))?;
        match response.payload {
            TerminalWorkerResponsePayload::String(text) => Ok(text),
            payload => Self::unexpected_response("SelectedText", payload),
        }
    }

    pub fn visible_text(&mut self) -> eyre::Result<String> {
        let response = self.request(TerminalWorkerCommand::VisibleText)?;
        match response.payload {
            TerminalWorkerResponsePayload::String(text) => Ok(text),
            payload => Self::unexpected_response("VisibleText", payload),
        }
    }

    pub fn viewport_metrics(&self) -> eyre::Result<TerminalViewportMetrics> {
        let response = self.request_read_only(TerminalWorkerCommand::ViewportMetrics)?;
        match response.payload {
            TerminalWorkerResponsePayload::ViewportMetrics(metrics) => Ok(metrics),
            payload => Self::unexpected_response("ViewportMetrics", payload),
        }
    }

    pub fn viewport_to_screen_cell(
        &self,
        cell: TerminalCellPoint,
    ) -> eyre::Result<TerminalCellPoint> {
        let response = self.request_read_only(TerminalWorkerCommand::ViewportToScreenCell(cell))?;
        match response.payload {
            TerminalWorkerResponsePayload::ScreenCell(screen_cell) => Ok(screen_cell),
            payload => Self::unexpected_response("ViewportToScreenCell", payload),
        }
    }

    pub fn scroll_viewport_by(&mut self, delta: isize) {
        let _ = self.request(TerminalWorkerCommand::ScrollViewportBy(delta));
    }

    pub fn scroll_viewport_to_offset(&mut self, offset: u64) -> eyre::Result<()> {
        let response = self.request(TerminalWorkerCommand::ScrollViewportToOffset(offset))?;
        match response.payload {
            TerminalWorkerResponsePayload::Unit => Ok(()),
            payload => Self::unexpected_response("ScrollViewportToOffset", payload),
        }
    }

    pub fn visible_display_state_with_selection(
        &mut self,
        selection: Option<TerminalSelection>,
    ) -> eyre::Result<TerminalDisplayState> {
        self.drain_worker_updates();
        if selection.is_none() {
            return Ok(self.cached_display.clone());
        }

        let response = self.request(TerminalWorkerCommand::VisibleDisplayStateWithSelection(
            selection,
        ))?;
        match response.payload {
            TerminalWorkerResponsePayload::DisplayState(display) => Ok(display),
            payload => Self::unexpected_response("VisibleDisplayStateWithSelection", payload),
        }
    }

    #[must_use]
    pub fn take_input_trace(&mut self) -> Vec<Vec<u8>> {
        match self.request(TerminalWorkerCommand::TakeInputTrace) {
            Ok(response) => match response.payload {
                TerminalWorkerResponsePayload::InputTrace(trace) => trace,
                _ => Vec::new(),
            },
            Err(_) => Vec::new(),
        }
    }

    #[must_use]
    pub fn semantic_prompt_state(&self) -> (bool, bool, bool) {
        match self.request_read_only(TerminalWorkerCommand::SemanticPromptState) {
            Ok(response) => match response.payload {
                TerminalWorkerResponsePayload::SemanticPromptState(state) => state,
                _ => (false, false, false),
            },
            Err(_) => (false, false, false),
        }
    }

    fn request(&mut self, command: TerminalWorkerCommand) -> eyre::Result<TerminalWorkerResponse> {
        self.drain_worker_updates();
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.worker_tx
            .send(TerminalWorkerRequest { command, reply_tx })
            .map_err(|error| eyre::eyre!("failed to send terminal worker request: {error}"))?;
        let response = reply_rx
            .recv()
            .map_err(|error| eyre::eyre!("terminal worker dropped response channel: {error}"))??;
        self.snapshot = response.snapshot;
        self.drain_worker_updates();
        Ok(response)
    }

    fn request_read_only(
        &self,
        command: TerminalWorkerCommand,
    ) -> eyre::Result<TerminalWorkerResponse> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.worker_tx
            .send(TerminalWorkerRequest { command, reply_tx })
            .map_err(|error| eyre::eyre!("failed to send terminal worker request: {error}"))?;
        reply_rx
            .recv()
            .map_err(|error| eyre::eyre!("terminal worker dropped response channel: {error}"))?
    }

    fn drain_worker_updates(&mut self) {
        let Ok(mut pending_updates) = self.pending_updates.lock() else {
            return;
        };

        while let Some(update) = pending_updates.pop_front() {
            match update {
                TerminalWorkerUpdate::Snapshot(snapshot) => {
                    self.snapshot = snapshot;
                }
                TerminalWorkerUpdate::DisplayState(display) => {
                    self.cached_display = display;
                }
                TerminalWorkerUpdate::PtyOutputQueued => {
                    self.worker_queued_output = true;
                }
                TerminalWorkerUpdate::ChildExited => {
                    self.snapshot.closed = true;
                }
                TerminalWorkerUpdate::RepaintRequested => {
                    self.repaint_requested = true;
                }
            }
        }
    }

    fn unexpected_response<T>(
        command_name: &str,
        _payload: TerminalWorkerResponsePayload,
    ) -> eyre::Result<T> {
        eyre::bail!("terminal worker returned an unexpected response for {command_name}")
    }
}

struct TerminalWorkerRunner {
    core: TerminalCore,
    request_rx: mpsc::Receiver<TerminalWorkerRequest>,
    update_tx: mpsc::Sender<TerminalWorkerUpdate>,
    last_snapshot: TerminalSnapshot,
    last_display_publish_at: Instant,
}

impl TerminalWorkerRunner {
    fn new(
        core: TerminalCore,
        request_rx: mpsc::Receiver<TerminalWorkerRequest>,
        update_tx: mpsc::Sender<TerminalWorkerUpdate>,
    ) -> Self {
        let last_snapshot = core.snapshot();
        Self {
            core,
            request_rx,
            update_tx,
            last_snapshot,
            last_display_publish_at: Instant::now(),
        }
    }

    fn run(&mut self) {
        let _ = self
            .update_tx
            .send(TerminalWorkerUpdate::Snapshot(self.last_snapshot));
        let _ = self.publish_display_state_if_due();

        loop {
            match self.request_rx.recv_timeout(TERMINAL_WORKER_IDLE_TIMEOUT) {
                Ok(request) => {
                    if self.handle_request(request).is_err() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if self.service_background_output().is_err() {
                let _ = self.update_tx.send(TerminalWorkerUpdate::ChildExited);
                break;
            }
        }
    }

    fn handle_request(&mut self, request: TerminalWorkerRequest) -> eyre::Result<()> {
        let payload = match request.command {
            TerminalWorkerCommand::Resize(layout) => {
                self.core.resize(layout)?;
                TerminalWorkerResponsePayload::Unit
            }
            TerminalWorkerCommand::HandleChar { code_unit, lparam } => {
                TerminalWorkerResponsePayload::Bool(self.core.handle_char(code_unit, lparam)?)
            }
            TerminalWorkerCommand::HandleKeyEvent {
                vkey,
                lparam,
                was_down,
                is_release,
                mods,
            } => TerminalWorkerResponsePayload::Bool(
                self.core
                    .handle_key_event(vkey, lparam, was_down, is_release, mods)?,
            ),
            TerminalWorkerCommand::HandlePaste(text) => {
                self.core.handle_paste(&text)?;
                TerminalWorkerResponsePayload::Unit
            }
            TerminalWorkerCommand::SelectedText(selection) => {
                TerminalWorkerResponsePayload::String(self.core.selected_text(selection)?)
            }
            TerminalWorkerCommand::VisibleText => {
                TerminalWorkerResponsePayload::String(self.core.visible_text()?)
            }
            TerminalWorkerCommand::ViewportMetrics => {
                TerminalWorkerResponsePayload::ViewportMetrics(self.core.viewport_metrics()?)
            }
            TerminalWorkerCommand::ViewportToScreenCell(cell) => {
                TerminalWorkerResponsePayload::ScreenCell(self.core.viewport_to_screen_cell(cell)?)
            }
            TerminalWorkerCommand::ScrollViewportBy(delta) => {
                self.core.scroll_viewport_by(delta);
                TerminalWorkerResponsePayload::Unit
            }
            TerminalWorkerCommand::ScrollViewportToOffset(offset) => {
                self.core.scroll_viewport_to_offset(offset)?;
                TerminalWorkerResponsePayload::Unit
            }
            TerminalWorkerCommand::VisibleDisplayStateWithSelection(selection) => {
                TerminalWorkerResponsePayload::DisplayState(
                    self.core.visible_display_state_with_selection(selection)?,
                )
            }
            TerminalWorkerCommand::CurrentKittyKeyboardFlags => {
                TerminalWorkerResponsePayload::KittyKeyboardFlags(
                    self.core.current_kitty_keyboard_flags()?,
                )
            }
            TerminalWorkerCommand::Win32InputModeEnabled => {
                TerminalWorkerResponsePayload::Bool(self.core.win32_input_mode_enabled())
            }
            TerminalWorkerCommand::TakeInputTrace => {
                TerminalWorkerResponsePayload::InputTrace(self.core.take_input_trace())
            }
            TerminalWorkerCommand::SemanticPromptState => {
                TerminalWorkerResponsePayload::SemanticPromptState(
                    self.core.semantic_prompt_state(),
                )
            }
        };

        self.publish_snapshot_if_changed()?;
        let response = TerminalWorkerResponse {
            snapshot: self.last_snapshot,
            payload,
        };
        let _ = request.reply_tx.send(Ok(response));
        Ok(())
    }

    fn service_background_output(&mut self) -> eyre::Result<()> {
        let poll_result = self.core.poll_pty_output()?;
        if poll_result.queued_output {
            let _ = self.update_tx.send(TerminalWorkerUpdate::PtyOutputQueued);
        }

        let pump_started_at = Instant::now();
        let mut processed_output = false;
        while self.core.has_pending_output() {
            if let Ok(request) = self.request_rx.try_recv() {
                if processed_output {
                    self.core.refresh_semantic_prompt_tracking()?;
                }
                self.handle_request(request)?;
                self.publish_snapshot_if_changed()?;
                self.publish_display_state_if_due()?;
                return Ok(());
            }

            let processed_output_bytes = self.core.pump_pending_output_slice();
            if processed_output_bytes == 0 {
                break;
            }
            processed_output = true;

            self.publish_snapshot_if_changed()?;
            self.publish_display_state_if_due()?;

            if pump_started_at.elapsed() >= TERMINAL_WORKER_PUMP_TIME_BUDGET {
                break;
            }
        }

        if processed_output {
            self.core.refresh_semantic_prompt_tracking()?;
        }

        self.core.refresh_child_exit_state()?;

        self.publish_snapshot_if_changed()?;
        self.publish_display_state_if_due()?;
        if self.last_snapshot.closed {
            let _ = self.update_tx.send(TerminalWorkerUpdate::ChildExited);
        }
        Ok(())
    }

    fn publish_display_state_if_due(&mut self) -> eyre::Result<()> {
        if !self.should_publish_display_state() {
            return Ok(());
        }

        let display = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("publish_cached_terminal_display_state").entered();
            self.core.cached_display_state()?.clone()
        };
        self.update_tx
            .send(TerminalWorkerUpdate::DisplayState(display))
            .map_err(|error| {
                eyre::eyre!("failed to publish terminal worker display state: {error}")
            })?;
        self.last_display_publish_at = Instant::now();
        let _ = self.update_tx.send(TerminalWorkerUpdate::RepaintRequested);
        Ok(())
    }

    fn should_publish_display_state(&self) -> bool {
        should_publish_terminal_display_state(
            self.core.display_cache_dirty,
            self.core.has_pending_output(),
            self.last_snapshot.closed,
            self.last_display_publish_at.elapsed(),
        )
    }

    fn publish_snapshot_if_changed(&mut self) -> eyre::Result<()> {
        let snapshot = self.core.snapshot();
        if snapshot != self.last_snapshot {
            self.last_snapshot = snapshot;
            self.update_tx
                .send(TerminalWorkerUpdate::Snapshot(snapshot))
                .map_err(|error| {
                    eyre::eyre!("failed to publish terminal worker snapshot: {error}")
                })?;
        }
        Ok(())
    }
}

impl TerminalCore {
    #[instrument(level = "info", skip_all)]
    pub fn new_with_command(shell: CommandBuilder) -> eyre::Result<Self> {
        let pty_system = native_pty_system();
        let initial_size = PtySize {
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = info_span!(
            "open_pseudoterminal",
            cols = DEFAULT_COLS,
            rows = DEFAULT_ROWS
        )
        .in_scope(|| {
            pty_system
                .openpty(initial_size)
                .map_err(|error| eyre::eyre!("failed to open pseudoterminal: {error}"))
        })?;

        let writer: Arc<Mutex<PtyWriter>> =
            Arc::new(Mutex::new(pair.master.take_writer().map_err(|error| {
                eyre::eyre!("failed to open PTY writer: {error}")
            })?));
        let writer_for_effect = Arc::clone(&writer);

        let mut terminal = info_span!("create_libghostty_terminal").in_scope(|| {
            Terminal::new(TerminalOptions {
                cols: DEFAULT_COLS,
                rows: DEFAULT_ROWS,
                max_scrollback: MAX_SCROLLBACK,
            })
            .wrap_err("failed to create libghostty terminal")
        })?;
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
        let child = info_span!("spawn_pty_child").in_scope(|| {
            pair.slave
                .spawn_command(shell)
                .map_err(|error| eyre::eyre!("failed to spawn shell inside PTY: {error}"))
        })?;
        drop(pair.slave);

        let mut cloned_reader = info_span!("clone_pty_reader").in_scope(|| {
            pair.master
                .try_clone_reader()
                .map_err(|error| eyre::eyre!("failed to clone PTY reader: {error}"))
        })?;
        let (reader_tx, reader_rx) = mpsc::sync_channel(PTY_READ_CHANNEL_CAPACITY);
        std::thread::spawn(move || {
            let mut buffer = vec![0_u8; PTY_READ_BUFFER_BYTES];
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
            pending_output: VecDeque::new(),
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
            semantic_prompt_buffer: Vec::new(),
            semantic_prompt: SemanticPromptTracking::default(),
            cached_display: TerminalDisplayState::default(),
            display_cache_dirty: true,
            closed: false,
        })
    }

    #[must_use]
    fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            cols: self.cols,
            rows: self.rows,
            pending_output_bytes: self.pending_output.len(),
            closed: self.closed,
        }
    }

    pub fn has_pending_output(&self) -> bool {
        !self.pending_output.is_empty()
    }

    #[instrument(level = "info", skip_all, fields(client_width = layout.client_width, client_height = layout.client_height))]
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
                pixel_width: u16::try_from(layout.terminal_viewport_rect().width().max(0))
                    .unwrap_or(u16::MAX),
                pixel_height: u16::try_from(layout.terminal_viewport_rect().height().max(0))
                    .unwrap_or(u16::MAX),
            })
            .map_err(|error| eyre::eyre!("failed to resize PTY: {error}"))?;

        self.cols = cols;
        self.rows = rows;
        self.repaint.needs_repaint = true;
        self.repaint.full_repaint_pending = true;
        self.invalidate_display_cache();
        Ok(())
    }

    #[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
    pub fn poll_pty_output(&mut self) -> eyre::Result<PollPtyOutputResult> {
        let mut queued_output = false;

        let () = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("drain_pty_reader_messages").entered();

            while let Ok(message) = self.reader.try_recv() {
                match message {
                    Ok(bytes) => {
                        let normalized_bytes = {
                            #[cfg(feature = "tracy")]
                            let _span = debug_span!("normalize_terminal_output_bytes").entered();
                            normalize_cursor_visibility_mode_sequence(&bytes)
                        };
                        let bytes = self.strip_win32_input_mode_sequence(normalized_bytes.as_ref());
                        let semantic_prompt_before_output = self.semantic_prompt;

                        let () = {
                            #[cfg(feature = "tracy")]
                            let _span = debug_span!("queue_pty_output_message").entered();

                            if should_close_from_echoed_ctrl_d(
                                semantic_prompt_before_output,
                                bytes.as_ref(),
                            ) {
                                let bytes = strip_echoed_ctrl_d(bytes.as_ref());
                                if !bytes.is_empty() {
                                    self.queue_terminal_output(bytes.as_ref());
                                    queued_output = true;
                                }
                                info!(
                                    semantic_prompt = ?self.semantic_prompt,
                                    "closing terminal after shell echoed Ctrl+D at the prompt"
                                );
                                self.closed = true;
                                break;
                            }

                            self.queue_terminal_output(bytes.as_ref());
                            queued_output = true;
                        };

                        if self.pending_output.len() >= TERMINAL_OUTPUT_QUEUE_SOFT_LIMIT_BYTES {
                            break;
                        }
                    }
                    Err(error) => {
                        let message = format!("\r\n[pty read error: {error}]\r\n");
                        let () = {
                            #[cfg(feature = "tracy")]
                            let _span = debug_span!("queue_pty_read_error_message").entered();
                            self.queue_terminal_output(message.as_bytes());
                            queued_output = true;
                            self.closed = true;
                        };
                    }
                }
            }
        };

        self.refresh_child_exit_state()?;

        Ok(PollPtyOutputResult {
            queued_output,
            should_close: self.closed,
        })
    }

    #[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
    #[expect(
        dead_code,
        reason = "keeps the pre-worker TerminalCore API available while the worker now drives output slices directly"
    )]
    pub fn pump_pending_output(&mut self) -> eyre::Result<PumpResult> {
        let output_processed = self.pump_pending_output_slice() > 0;

        if output_processed {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("refresh_semantic_prompt_tracking").entered();
            self.refresh_semantic_prompt_tracking()?;
        }

        self.refresh_child_exit_state()?;

        Ok(PumpResult {
            should_close: self.closed,
        })
    }

    fn pump_pending_output_slice(&mut self) -> usize {
        let processed_output_bytes = self.flush_pending_output();
        if processed_output_bytes > 0 {
            self.repaint.needs_repaint = true;
            self.invalidate_display_cache();
        }

        processed_output_bytes
    }

    fn refresh_child_exit_state(&mut self) -> eyre::Result<()> {
        #[cfg(feature = "tracy")]
        let _span = debug_span!("query_terminal_child_exit").entered();
        if self
            .child
            .try_wait()
            .wrap_err("failed to query shell status")?
            .is_some()
        {
            self.closed = true;
        }

        Ok(())
    }

    fn queue_terminal_output(&mut self, data: &[u8]) {
        self.pending_output.extend(data.iter().copied());
    }

    #[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
    fn flush_pending_output(&mut self) -> usize {
        if self.pending_output.is_empty() {
            return 0;
        }

        let slice_len = TERMINAL_OUTPUT_SLICE_BYTES.min(self.pending_output.len());
        let slice: Vec<u8> = self.pending_output.drain(..slice_len).collect();

        let () = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("process_terminal_output_chunk").entered();
            let () = {
                #[cfg(feature = "tracy")]
                let _span = debug_span!("observe_semantic_prompt_sequences").entered();
                self.observe_semantic_prompt_sequences(&slice);
            };
            let () = {
                #[cfg(feature = "tracy")]
                let _span = debug_span!("vt_write_terminal_output_slice").entered();
                self.terminal.vt_write(&slice);
            };
        };

        slice.len()
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

            self.mark_prompt_input_written();
            self.write_win32_input_mode_char_event(pending_key, character, lparam)?;
            self.repaint.needs_repaint = true;
            return Ok(true);
        }

        if character == '\r' || character == '\t' || character == '\u{8}' {
            return Ok(false);
        }

        if character < ' ' {
            let control = u8::try_from(u32::from(character)).unwrap_or_default();
            if control == CTRL_D_EOF && should_translate_ctrl_d_to_exit(self.semantic_prompt) {
                debug!(
                    semantic_prompt = ?self.semantic_prompt,
                    "translating Ctrl+D at shell prompt into `exit`"
                );
                self.write_input(CTRL_D_EXIT_COMMAND)?;
                self.repaint.needs_repaint = true;
                return Ok(true);
            }
            self.mark_prompt_input_written();
            self.write_input(&[control])?;
            self.repaint.needs_repaint = true;
            return Ok(true);
        }

        let mut bytes = [0_u8; 4];
        let encoded = character.encode_utf8(&mut bytes);
        self.mark_prompt_input_written();
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

        if !is_release && !was_down && should_translate_ctrl_d_key(key_event, self.semantic_prompt)
        {
            self.suppressed_chars
                .push_back(SuppressedChar::single(u32::from(CTRL_D_EOF)));
            debug!(
                semantic_prompt = ?self.semantic_prompt,
                "translating Ctrl+D key press at shell prompt into `exit`"
            );
            self.write_input(CTRL_D_EXIT_COMMAND)?;
            self.repaint.needs_repaint = true;
            return Ok(true);
        }

        if should_mark_prompt_input_written_for_key(key_event, was_down, is_release) {
            self.mark_prompt_input_written();
        }

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

    pub fn handle_paste(&mut self, text: &str) -> eyre::Result<()> {
        self.write_input(text.as_bytes())
    }

    pub fn selected_text(&mut self, selection: TerminalSelection) -> eyre::Result<String> {
        let rows = self.selected_cell_text_rows(selection)?;
        Ok(extract_selected_text(&rows, selection))
    }

    pub fn visible_text(&mut self) -> eyre::Result<String> {
        let rows = self.visible_cell_text_rows()?;
        Ok(rows
            .into_iter()
            .map(|row| row.cells.concat().trim_end_matches(' ').to_owned())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn viewport_metrics(&self) -> eyre::Result<TerminalViewportMetrics> {
        let scrollbar = self
            .terminal
            .scrollbar()
            .wrap_err("failed to query terminal scrollbar state")?;
        let scrollback_rows = self
            .terminal
            .scrollback_rows()
            .wrap_err("failed to query terminal scrollback row count")?;

        Ok(TerminalViewportMetrics {
            total: scrollbar.total,
            offset: scrollbar.offset,
            visible: scrollbar.len,
            scrollback: u64::try_from(scrollback_rows).unwrap_or(u64::MAX),
        })
    }

    pub fn viewport_to_screen_cell(
        &self,
        cell: TerminalCellPoint,
    ) -> eyre::Result<TerminalCellPoint> {
        let metrics = self.viewport_metrics()?;
        Ok(TerminalCellPoint::new(
            cell.column(),
            i32::try_from(metrics.offset).unwrap_or(i32::MAX) + cell.row(),
        ))
    }

    pub fn scroll_viewport_by(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }

        self.terminal.scroll_viewport(ScrollViewport::Delta(delta));
        self.repaint.needs_repaint = true;
        self.repaint.full_repaint_pending = true;
        self.invalidate_display_cache();
    }

    pub fn scroll_viewport_to_offset(&mut self, offset: u64) -> eyre::Result<()> {
        let viewport = self.viewport_metrics()?;
        let max_offset = viewport.total.saturating_sub(viewport.visible);
        let target_offset = offset.min(max_offset);
        if target_offset == viewport.offset {
            return Ok(());
        }

        if target_offset == 0 {
            self.terminal.scroll_viewport(ScrollViewport::Top);
        } else if target_offset == max_offset {
            self.terminal.scroll_viewport(ScrollViewport::Bottom);
        } else {
            let delta = i128::from(target_offset) - i128::from(viewport.offset);
            let delta = delta.clamp(isize::MIN as i128, isize::MAX as i128) as isize;
            self.terminal.scroll_viewport(ScrollViewport::Delta(delta));
        }

        self.repaint.needs_repaint = true;
        self.repaint.full_repaint_pending = true;
        self.invalidate_display_cache();
        Ok(())
    }

    #[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
    pub fn visible_display_state_with_selection(
        &mut self,
        selection: Option<TerminalSelection>,
    ) -> eyre::Result<TerminalDisplayState> {
        if selection.is_none() {
            return Ok(self.cached_display_state()?.clone());
        }

        self.build_display_state(selection)
    }

    fn invalidate_display_cache(&mut self) {
        self.display_cache_dirty = true;
    }

    fn cached_display_state(&mut self) -> eyre::Result<&TerminalDisplayState> {
        if self.display_cache_dirty {
            let display = self.build_display_state(None)?;
            self.cached_display = display;
            self.display_cache_dirty = false;
        }

        Ok(&self.cached_display)
    }

    #[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
    fn build_display_state(
        &mut self,
        selection: Option<TerminalSelection>,
    ) -> eyre::Result<TerminalDisplayState> {
        let viewport = self.viewport_metrics()?;
        let snapshot = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_terminal_render_state").entered();
            self.render_state
                .update(&self.terminal)
                .wrap_err("failed to update terminal render state")?
        };
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
            scrollbar: Some(TerminalDisplayScrollbar {
                total: viewport.total,
                offset: viewport.offset,
                visible: viewport.visible,
            }),
        };

        {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("collect_visible_terminal_cells").entered();
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
                    let viewport_cell = TerminalCellPoint::new(column_index, row_index);
                    let selection_cell = TerminalCellPoint::new(
                        column_index,
                        i32::try_from(viewport.offset).unwrap_or(i32::MAX) + row_index,
                    );
                    let selected =
                        selection.is_some_and(|selection| selection.contains(selection_cell));
                    let (glyph_color, background_color) = resolve_terminal_cell_colors(
                        &colors,
                        foreground,
                        background,
                        style.inverse ^ selected,
                    );

                    if let Some(color) = background_color {
                        display.backgrounds.push(TerminalDisplayBackground {
                            cell: viewport_cell,
                            color,
                        });
                    }

                    if !graphemes.is_empty() {
                        for character in graphemes {
                            display.glyphs.push(TerminalDisplayGlyph {
                                cell: viewport_cell,
                                character,
                                color: glyph_color,
                            });
                        }
                    }
                    column_index += 1;
                }
                row_index += 1;
            }
        }

        Ok(display)
    }

    #[must_use]
    pub fn take_input_trace(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.input_trace)
    }

    #[must_use]
    pub fn semantic_prompt_state(&self) -> (bool, bool, bool) {
        (
            self.semantic_prompt.markers_observed,
            self.semantic_prompt.at_shell_prompt,
            matches!(
                self.semantic_prompt.input_state,
                PromptInputState::AwaitingPristine | PromptInputState::AwaitingEdited
            ),
        )
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

    fn refresh_semantic_prompt_tracking(&mut self) -> eyre::Result<()> {
        let next = semantic_prompt_tracking(&mut self.render_state, &self.terminal)?;
        if !self.semantic_prompt.markers_observed && next.markers_observed {
            info!("detected OSC 133 semantic prompt markers from shell output");
        }
        if self.semantic_prompt.at_shell_prompt != next.at_shell_prompt {
            info!(
                at_shell_prompt = next.at_shell_prompt,
                "terminal shell prompt state changed"
            );
        }
        self.semantic_prompt.markers_observed =
            self.semantic_prompt.markers_observed || next.markers_observed;
        self.semantic_prompt.at_shell_prompt = next.at_shell_prompt;
        Ok(())
    }

    fn mark_prompt_input_written(&mut self) {
        if matches!(
            self.semantic_prompt.input_state,
            PromptInputState::AwaitingPristine
        ) {
            self.semantic_prompt.input_state = PromptInputState::AwaitingEdited;
        }
    }

    fn observe_semantic_prompt_sequences(&mut self, data: &[u8]) {
        let mut combined = std::mem::take(&mut self.semantic_prompt_buffer);
        combined.extend_from_slice(data);

        let mut index = 0;
        while index < combined.len() {
            let Some(relative_start) = combined[index..]
                .windows(OSC_133_PREFIX.len())
                .position(|window| window == OSC_133_PREFIX)
            else {
                break;
            };

            let start = index + relative_start;
            let payload_start = start + OSC_133_PREFIX.len();
            let Some((payload_end, terminator_len)) = osc_terminator(&combined[payload_start..])
            else {
                self.semantic_prompt_buffer
                    .extend_from_slice(&combined[start..]);
                return;
            };

            let payload = &combined[payload_start..payload_start + payload_end];
            self.apply_semantic_prompt_payload(payload);
            index = payload_start + payload_end + terminator_len;
        }

        if index < combined.len() {
            let trailing = &combined[index..];
            if let Some(partial_len) = partial_osc_133_prefix_len(trailing) {
                self.semantic_prompt_buffer
                    .extend_from_slice(&trailing[trailing.len() - partial_len..]);
            }
        }
    }

    fn apply_semantic_prompt_payload(&mut self, payload: &[u8]) {
        let Some(action) = payload.first().copied() else {
            return;
        };

        match action {
            b'B' | b'I' => {
                self.semantic_prompt.markers_observed = true;
                self.semantic_prompt.input_state = PromptInputState::AwaitingPristine;
                debug!(
                    action = %char::from(action),
                    "detected OSC 133 shell awaiting-input marker"
                );
            }
            b'A' | b'C' | b'D' | b'N' | b'P' => {
                self.semantic_prompt.markers_observed = true;
                self.semantic_prompt.input_state = PromptInputState::Inactive;
            }
            b'L' => {
                self.semantic_prompt.markers_observed = true;
            }
            _ => {}
        }
    }

    fn visible_cell_text_rows(&mut self) -> eyre::Result<Vec<TerminalTextRow>> {
        let viewport = self.viewport_metrics()?;
        let snapshot = self
            .render_state
            .update(&self.terminal)
            .wrap_err("failed to update terminal render state")?;
        let mut rows = RowIterator::new().wrap_err("failed to create row iterator")?;
        let mut cells = CellIterator::new().wrap_err("failed to create cell iterator")?;
        let mut text_rows = Vec::new();

        let mut row_iter = rows
            .update(&snapshot)
            .wrap_err("failed to update row iterator")?;
        let mut row_index = 0_i32;
        while let Some(row) = row_iter.next() {
            let mut row_cells = Vec::new();
            let mut cell_iter = cells
                .update(row)
                .wrap_err("failed to update cell iterator")?;
            while let Some(cell) = cell_iter.next() {
                let graphemes = cell.graphemes().wrap_err("failed to read cell text")?;
                if graphemes.is_empty() {
                    row_cells.push(" ".to_owned());
                } else {
                    row_cells.push(graphemes.iter().collect());
                }
            }
            text_rows.push(TerminalTextRow {
                row: i32::try_from(viewport.offset).unwrap_or(i32::MAX) + row_index,
                cells: row_cells,
            });
            row_index += 1;
        }

        Ok(text_rows)
    }

    fn selected_cell_text_rows(
        &self,
        selection: TerminalSelection,
    ) -> eyre::Result<Vec<TerminalTextRow>> {
        let total_rows = self
            .terminal
            .total_rows()
            .wrap_err("failed to query terminal total row count")?;
        if total_rows == 0 {
            return Ok(Vec::new());
        }

        let (start_row, end_row) = selection_row_bounds(selection);
        let max_row = i32::try_from(total_rows.saturating_sub(1)).unwrap_or(i32::MAX);
        let clamped_start = start_row.clamp(0, max_row);
        let clamped_end = end_row.clamp(0, max_row);
        let mut rows = Vec::new();

        for row in clamped_start..=clamped_end {
            rows.push(TerminalTextRow {
                row,
                cells: self.screen_row_cells(u32::try_from(row).unwrap_or_default())?,
            });
        }

        Ok(rows)
    }

    fn screen_row_cells(&self, row: u32) -> eyre::Result<Vec<String>> {
        let mut cells = Vec::with_capacity(usize::from(self.cols));
        for column in 0..self.cols {
            let grid_ref = self
                .terminal
                .grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))
                .wrap_err_with(|| {
                    format!("failed to resolve terminal screen point at column {column}, row {row}")
                })?;
            cells.push(read_grid_ref_text(&grid_ref)?);
        }

        Ok(cells)
    }
}

fn ordered_linear_bounds(
    anchor: TerminalCellPoint,
    focus: TerminalCellPoint,
) -> (TerminalCellPoint, TerminalCellPoint) {
    if (anchor.row(), anchor.column()) <= (focus.row(), focus.column()) {
        (anchor, focus)
    } else {
        (focus, anchor)
    }
}

fn semantic_prompt_tracking(
    render_state: &mut RenderState<'static>,
    terminal: &Terminal<'static, 'static>,
) -> eyre::Result<SemanticPromptTracking> {
    let snapshot = render_state
        .update(terminal)
        .wrap_err("failed to update terminal render state for semantic prompt tracking")?;
    let cursor_row = snapshot
        .cursor_viewport()
        .wrap_err("failed to query terminal cursor viewport for semantic prompt tracking")?
        .map(|cursor| cursor.y);

    let mut rows = RowIterator::new().wrap_err("failed to create row iterator")?;
    let mut row_iter = rows
        .update(&snapshot)
        .wrap_err("failed to update row iterator")?;

    let mut row_index = 0_u16;
    let mut markers_observed = false;
    let mut at_shell_prompt = false;

    while let Some(row) = row_iter.next() {
        let semantic_prompt = row
            .raw_row()
            .wrap_err("failed to query raw terminal row for semantic prompt tracking")?
            .semantic_prompt()
            .wrap_err("failed to query terminal row semantic prompt state")?;

        if semantic_prompt != RowSemanticPrompt::None {
            markers_observed = true;
            if cursor_row == Some(row_index) {
                at_shell_prompt = true;
            }
        }

        row_index = row_index.saturating_add(1);
    }

    Ok(SemanticPromptTracking {
        markers_observed,
        at_shell_prompt,
        input_state: PromptInputState::Inactive,
    })
}

fn should_close_from_echoed_ctrl_d(tracking: SemanticPromptTracking, data: &[u8]) -> bool {
    tracking.markers_observed
        && tracking.at_shell_prompt
        && matches!(tracking.input_state, PromptInputState::AwaitingPristine)
        && data.contains(&CTRL_D_EOF)
}

fn should_translate_ctrl_d_to_exit(tracking: SemanticPromptTracking) -> bool {
    tracking.markers_observed
        && tracking.at_shell_prompt
        && matches!(tracking.input_state, PromptInputState::AwaitingPristine)
}

/// behavior[impl window.interaction.input.ctrl-d-exits-current-shell-at-prompt]
fn should_translate_ctrl_d_key(
    key_event: PendingWin32CharKey,
    tracking: SemanticPromptTracking,
) -> bool {
    should_translate_ctrl_d_to_exit(tracking)
        && key_event.mapped_key == key::Key::D
        && key_event.mods.contains(key::Mods::CTRL)
}

fn should_mark_prompt_input_written_for_key(
    key_event: PendingWin32CharKey,
    was_down: bool,
    is_release: bool,
) -> bool {
    !was_down && !is_release && !is_modifier_key(key_event.mapped_key)
}

fn is_modifier_key(mapped_key: key::Key) -> bool {
    matches!(
        mapped_key,
        key::Key::ShiftLeft
            | key::Key::ShiftRight
            | key::Key::ControlLeft
            | key::Key::ControlRight
            | key::Key::AltLeft
            | key::Key::AltRight
    )
}

fn strip_echoed_ctrl_d(data: &[u8]) -> Cow<'_, [u8]> {
    if !data.contains(&CTRL_D_EOF) {
        return Cow::Borrowed(data);
    }

    Cow::Owned(
        data.iter()
            .copied()
            .filter(|byte| *byte != CTRL_D_EOF)
            .collect(),
    )
}

fn osc_terminator(data: &[u8]) -> Option<(usize, usize)> {
    if let Some(index) = data.iter().position(|byte| *byte == b'\x07') {
        return Some((index, 1));
    }

    data.windows(2)
        .position(|window| window == b"\x1b\\")
        .map(|index| (index, 2))
}

fn partial_osc_133_prefix_len(data: &[u8]) -> Option<usize> {
    let max_len = data.len().min(OSC_133_PREFIX.len().saturating_sub(1));
    (1..=max_len)
        .rev()
        .find(|len| OSC_133_PREFIX.starts_with(&data[data.len() - len..]))
}

fn ordered_block_bounds(
    anchor: TerminalCellPoint,
    focus: TerminalCellPoint,
) -> (i32, i32, i32, i32) {
    (
        anchor.column().min(focus.column()),
        anchor.row().min(focus.row()),
        anchor.column().max(focus.column()),
        anchor.row().max(focus.row()),
    )
}

fn linear_selection_contains(
    start: TerminalCellPoint,
    end: TerminalCellPoint,
    cell: TerminalCellPoint,
) -> bool {
    (cell.row(), cell.column()) >= (start.row(), start.column())
        && (cell.row(), cell.column()) <= (end.row(), end.column())
}

fn selection_row_bounds(selection: TerminalSelection) -> (i32, i32) {
    match selection.mode() {
        TerminalSelectionMode::Linear => {
            let (start, end) = ordered_linear_bounds(selection.anchor, selection.focus);
            (start.row(), end.row())
        }
        TerminalSelectionMode::Block => {
            let (_, top, _, bottom) = ordered_block_bounds(selection.anchor, selection.focus);
            (top, bottom)
        }
    }
}

fn extract_selected_text(rows: &[TerminalTextRow], selection: TerminalSelection) -> String {
    let mut selected_rows = Vec::new();

    for row in rows {
        let mut selected = String::new();
        for (column_index, cell_text) in row.cells.iter().enumerate() {
            let cell =
                TerminalCellPoint::new(i32::try_from(column_index).unwrap_or(i32::MAX), row.row);
            if selection.contains(cell) {
                selected.push_str(cell_text);
            }
        }

        if !selected.is_empty() {
            let normalized = if selection.mode() == TerminalSelectionMode::Linear {
                selected.trim_end_matches(' ').to_owned()
            } else {
                selected
            };
            selected_rows.push(normalized);
        }
    }

    selected_rows.join("\n")
}

fn read_grid_ref_text(grid_ref: &libghostty_vt::screen::GridRef<'_>) -> eyre::Result<String> {
    let mut small = ['\0'; 8];
    match grid_ref.graphemes(&mut small) {
        Ok(0) => Ok(" ".to_owned()),
        Ok(length) => Ok(small[..length].iter().collect()),
        Err(libghostty_vt::Error::OutOfSpace { required }) => {
            let mut buffer = vec!['\0'; required];
            let length = grid_ref
                .graphemes(&mut buffer)
                .wrap_err("failed to read terminal grapheme cluster into resized buffer")?;
            if length == 0 {
                Ok(" ".to_owned())
            } else {
                Ok(buffer[..length].iter().collect())
            }
        }
        Err(error) => Err(error).wrap_err("failed to read terminal cell grapheme cluster"),
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

/// behavior[impl window.appearance.terminal.selection.inverse]
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

/// behavior[impl window.appearance.terminal.cursor.visible]
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

/// behavior[impl window.interaction.input.numpad-numlock-text]
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

fn should_publish_terminal_display_state(
    display_cache_dirty: bool,
    has_pending_output: bool,
    child_closed: bool,
    elapsed_since_last_publish: Duration,
) -> bool {
    if !display_cache_dirty {
        return false;
    }

    if child_closed || !has_pending_output {
        return true;
    }

    elapsed_since_last_publish >= TERMINAL_DISPLAY_PUBLISH_INTERVAL
}

#[cfg(test)]
mod tests {
    use super::{
        MIN_CODE_PANEL_HEIGHT, PendingWin32CharKey, PromptInputState,
        SemanticPromptTracking, TERMINAL_DISPLAY_PUBLISH_INTERVAL,
        TerminalDisplayCursorStyle, TerminalLayout, TerminalSelection, TerminalSelectionMode,
        TerminalTextRow, extract_selected_text, map_cursor_style, map_virtual_key, osc_terminator,
        partial_osc_133_prefix_len, resolve_terminal_cell_colors, should_close_from_echoed_ctrl_d,
        should_mark_prompt_input_written_for_key, should_publish_terminal_display_state,
        should_translate_ctrl_d_key, should_translate_ctrl_d_to_exit, strip_echoed_ctrl_d,
    };
    use crate::app::spatial::TerminalCellPoint;
    use libghostty_vt::key;
    use libghostty_vt::render::Colors;
    use libghostty_vt::style::RgbColor;
    use std::time::Duration;

    // behavior[verify window.appearance.code-panel.terminal-alignment]
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
        let terminal = layout.terminal_viewport_rect();
        let scrollbar = layout.terminal_scrollbar_rect();

        assert!(sidecar.right() <= code.left());
        assert!(code.bottom() < result.top());
        assert!(result.bottom() < plus.top());
        assert_eq!(terminal.left(), code.left());
        assert_eq!(terminal.bottom(), code.bottom());
        assert!(terminal.right() < scrollbar.left());
        assert_eq!(scrollbar.right(), code.right());
        assert_eq!(scrollbar.bottom(), code.bottom());
        assert!(code.height() >= MIN_CODE_PANEL_HEIGHT);
    }

    // behavior[verify window.appearance.terminal.selection.inverse]
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
    fn display_publish_is_throttled_while_output_is_still_bursting() {
        assert!(!should_publish_terminal_display_state(
            true,
            true,
            false,
            TERMINAL_DISPLAY_PUBLISH_INTERVAL.saturating_sub(Duration::from_millis(1)),
        ));
    }

    #[test]
    fn display_publish_is_immediate_when_output_burst_finishes() {
        assert!(should_publish_terminal_display_state(
            true,
            false,
            false,
            Duration::ZERO,
        ));
    }

    #[test]
    fn display_publish_is_immediate_when_child_has_closed() {
        assert!(should_publish_terminal_display_state(
            true,
            true,
            true,
            Duration::ZERO,
        ));
    }

    // behavior[verify window.appearance.chrome]
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

    // behavior[verify window.interaction.input.numpad-numlock-text]
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

    // behavior[verify window.interaction.input.numpad-numlock-text]
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

    #[test]
    fn echoed_ctrl_d_only_closes_once_semantic_prompt_markers_report_shell_input() {
        assert!(!should_close_from_echoed_ctrl_d(
            SemanticPromptTracking::default(),
            &[0x04]
        ));
        assert!(!should_close_from_echoed_ctrl_d(
            SemanticPromptTracking {
                markers_observed: true,
                at_shell_prompt: false,
                input_state: PromptInputState::AwaitingPristine,
            },
            &[0x04]
        ));
        assert!(!should_close_from_echoed_ctrl_d(
            SemanticPromptTracking {
                markers_observed: true,
                at_shell_prompt: true,
                input_state: PromptInputState::Inactive,
            },
            b"not-eof"
        ));
        assert!(!should_close_from_echoed_ctrl_d(
            SemanticPromptTracking {
                markers_observed: true,
                at_shell_prompt: true,
                input_state: PromptInputState::Inactive,
            },
            &[0x04]
        ));
        assert!(should_close_from_echoed_ctrl_d(
            SemanticPromptTracking {
                markers_observed: true,
                at_shell_prompt: true,
                input_state: PromptInputState::AwaitingPristine,
            },
            &[0x04]
        ));
        assert!(should_close_from_echoed_ctrl_d(
            SemanticPromptTracking {
                markers_observed: true,
                at_shell_prompt: true,
                input_state: PromptInputState::AwaitingPristine,
            },
            b"\x1b]133;D;0\x07\x04"
        ));
    }

    #[test]
    fn strip_echoed_ctrl_d_removes_eof_byte_from_mixed_output() {
        assert_eq!(
            strip_echoed_ctrl_d(b"prefix\x04suffix").as_ref(),
            b"prefixsuffix"
        );
        assert_eq!(strip_echoed_ctrl_d(&[0x04]).as_ref(), b"");
        assert_eq!(
            strip_echoed_ctrl_d(b"plain output").as_ref(),
            b"plain output"
        );
    }

    // behavior[verify window.interaction.input.ctrl-d-exits-current-shell-at-prompt]
    #[test]
    fn ctrl_d_translation_targets_shell_prompt_rows() {
        assert!(!should_translate_ctrl_d_to_exit(
            SemanticPromptTracking::default()
        ));
        assert!(!should_translate_ctrl_d_to_exit(SemanticPromptTracking {
            markers_observed: true,
            at_shell_prompt: false,
            input_state: PromptInputState::AwaitingPristine,
        }));
        assert!(should_translate_ctrl_d_to_exit(SemanticPromptTracking {
            markers_observed: true,
            at_shell_prompt: true,
            input_state: PromptInputState::AwaitingPristine,
        }));
        assert!(!should_translate_ctrl_d_to_exit(SemanticPromptTracking {
            markers_observed: true,
            at_shell_prompt: true,
            input_state: PromptInputState::AwaitingEdited,
        }));
    }

    // behavior[verify window.interaction.input.ctrl-d-exits-current-shell-at-prompt]
    #[test]
    fn ctrl_d_key_translation_requires_ctrl_modified_d_at_shell_prompt() {
        let tracking = SemanticPromptTracking {
            markers_observed: true,
            at_shell_prompt: true,
            input_state: PromptInputState::AwaitingPristine,
        };
        let ctrl_d = PendingWin32CharKey {
            vkey: 0x44,
            lparam: 0,
            mapped_key: key::Key::D,
            unshifted_codepoint: 'd',
            mods: key::Mods::CTRL,
        };
        let plain_d = PendingWin32CharKey {
            mods: key::Mods::empty(),
            ..ctrl_d
        };

        assert!(should_translate_ctrl_d_key(ctrl_d, tracking));
        assert!(!should_translate_ctrl_d_key(plain_d, tracking));
        assert!(!should_translate_ctrl_d_key(
            ctrl_d,
            SemanticPromptTracking::default()
        ));
        assert!(!should_translate_ctrl_d_key(
            ctrl_d,
            SemanticPromptTracking {
                markers_observed: true,
                at_shell_prompt: true,
                input_state: PromptInputState::AwaitingEdited,
            }
        ));
    }

    #[test]
    fn prompt_input_write_tracking_ignores_modifier_keydowns() {
        let ctrl_key = PendingWin32CharKey {
            vkey: 0x11,
            lparam: 0,
            mapped_key: key::Key::ControlLeft,
            unshifted_codepoint: '\0',
            mods: key::Mods::CTRL,
        };
        let a_key = PendingWin32CharKey {
            vkey: 0x41,
            lparam: 0,
            mapped_key: key::Key::A,
            unshifted_codepoint: 'a',
            mods: key::Mods::empty(),
        };

        assert!(!should_mark_prompt_input_written_for_key(
            ctrl_key, false, false
        ));
        assert!(should_mark_prompt_input_written_for_key(
            a_key, false, false
        ));
        assert!(!should_mark_prompt_input_written_for_key(
            a_key, true, false
        ));
        assert!(!should_mark_prompt_input_written_for_key(
            a_key, false, true
        ));
    }

    #[test]
    fn osc_terminator_accepts_bel_and_st() {
        assert_eq!(osc_terminator(b"B\x07rest"), Some((1, 1)));
        assert_eq!(osc_terminator(b"B\x1b\\rest"), Some((1, 2)));
        assert_eq!(osc_terminator(b"B"), None);
    }

    #[test]
    fn partial_osc_133_prefix_len_tracks_split_prefixes() {
        assert_eq!(partial_osc_133_prefix_len(b"abc\x1b]13"), Some(4));
        assert_eq!(partial_osc_133_prefix_len(b"plain"), None);
    }

    // behavior[verify window.interaction.selection.linear]
    #[test]
    fn linear_selection_wraps_across_rows() {
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(2, 0),
            TerminalCellPoint::new(1, 1),
            TerminalSelectionMode::Linear,
        );

        assert!(selection.contains(TerminalCellPoint::new(2, 0)));
        assert!(selection.contains(TerminalCellPoint::new(3, 0)));
        assert!(selection.contains(TerminalCellPoint::new(0, 1)));
        assert!(selection.contains(TerminalCellPoint::new(1, 1)));
        assert!(!selection.contains(TerminalCellPoint::new(1, 0)));
        assert!(!selection.contains(TerminalCellPoint::new(2, 1)));
    }

    // behavior[verify window.interaction.selection.block-alt-drag]
    #[test]
    fn block_selection_uses_a_rectangle() {
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(3, 1),
            TerminalCellPoint::new(1, 3),
            TerminalSelectionMode::Block,
        );

        assert!(selection.contains(TerminalCellPoint::new(1, 1)));
        assert!(selection.contains(TerminalCellPoint::new(2, 2)));
        assert!(selection.contains(TerminalCellPoint::new(3, 3)));
        assert!(!selection.contains(TerminalCellPoint::new(0, 2)));
        assert!(!selection.contains(TerminalCellPoint::new(4, 2)));
    }

    // behavior[verify window.interaction.clipboard.right-click-copy-selection]
    #[test]
    fn extract_selected_text_wraps_linear_selection_by_row() {
        let rows = vec![
            TerminalTextRow {
                row: 0,
                cells: vec![
                    "a".to_owned(),
                    "b".to_owned(),
                    "c".to_owned(),
                    "d".to_owned(),
                ],
            },
            TerminalTextRow {
                row: 1,
                cells: vec![
                    "e".to_owned(),
                    "f".to_owned(),
                    "g".to_owned(),
                    "h".to_owned(),
                ],
            },
        ];
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(2, 0),
            TerminalCellPoint::new(1, 1),
            TerminalSelectionMode::Linear,
        );

        assert_eq!(extract_selected_text(&rows, selection), "cd\nef");
    }

    // behavior[verify window.interaction.clipboard.right-click-copy-selection]
    #[test]
    fn extract_selected_text_preserves_block_rows() {
        let rows = vec![
            TerminalTextRow {
                row: 0,
                cells: vec![
                    "a".to_owned(),
                    "b".to_owned(),
                    "c".to_owned(),
                    "d".to_owned(),
                ],
            },
            TerminalTextRow {
                row: 1,
                cells: vec![
                    "e".to_owned(),
                    "f".to_owned(),
                    "g".to_owned(),
                    "h".to_owned(),
                ],
            },
            TerminalTextRow {
                row: 2,
                cells: vec![
                    "i".to_owned(),
                    "j".to_owned(),
                    "k".to_owned(),
                    "l".to_owned(),
                ],
            },
        ];
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(1, 0),
            TerminalCellPoint::new(2, 2),
            TerminalSelectionMode::Block,
        );

        assert_eq!(extract_selected_text(&rows, selection), "bc\nfg\njk");
    }

    // behavior[verify window.interaction.clipboard.selection-preserves-scrolled-history]
    #[test]
    fn extract_selected_text_uses_absolute_row_coordinates() {
        let rows = vec![
            TerminalTextRow {
                row: 12,
                cells: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
            },
            TerminalTextRow {
                row: 13,
                cells: vec!["d".to_owned(), "e".to_owned(), "f".to_owned()],
            },
        ];
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(1, 12),
            TerminalCellPoint::new(1, 13),
            TerminalSelectionMode::Linear,
        );

        assert_eq!(extract_selected_text(&rows, selection), "bc\nde");
    }

    #[test]
    fn selection_checks_can_use_absolute_rows_without_moving_render_cells() {
        let render_cell = TerminalCellPoint::new(4, 2);
        let selection_cell = TerminalCellPoint::new(4, 18);
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(4, 18),
            TerminalCellPoint::new(4, 18),
            TerminalSelectionMode::Block,
        );

        assert_eq!(render_cell.row(), 2);
        assert!(selection.contains(selection_cell));
        assert!(!selection.contains(render_cell));
    }
}
