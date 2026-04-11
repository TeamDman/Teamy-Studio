use facet::Facet;
use libghostty_vt::terminal::ScrollViewport;
use std::borrow::Cow;
#[cfg(feature = "tracy")]
use tracing::debug_span;
use tracing::warn;

type PtyWriteEffect = Box<dyn FnMut(&[u8]) + Send>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum EscapeState {
    Ground,
    Escape,
    Csi {
        parameters: String,
        intermediates: String,
    },
    Osc(String),
    OscEscape(String),
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Facet)]
pub enum TeamyColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb {
        r: u8,
        g: u8,
        b: u8,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Facet)]
pub struct TeamyCellStyle {
    pub foreground: TeamyColor,
    pub background: TeamyColor,
    pub inverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TeamyCell {
    character: char,
    style: TeamyCellStyle,
}

impl TeamyCell {
    fn blank() -> Self {
        Self {
            character: ' ',
            style: TeamyCellStyle::default(),
        }
    }

    fn blank_with_style(style: TeamyCellStyle) -> Self {
        Self {
            character: ' ',
            style,
        }
    }
}

#[derive(Clone, Debug)]
struct TeamySavedScreen {
    visible_rows: Vec<Vec<TeamyCell>>,
    scrollback_rows: Vec<Vec<TeamyCell>>,
    viewport_offset: usize,
    cursor_col: usize,
    cursor_row: usize,
    current_style: TeamyCellStyle,
    cursor_style: TeamyCursorStyle,
    cursor_visible: bool,
}

#[derive(Clone, Copy, Debug)]
struct TeamySavedCursor {
    col: usize,
    row: usize,
    style: TeamyCursorStyle,
    visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Facet)]
pub struct TeamyDisplayGlyph {
    pub row: usize,
    pub column: usize,
    pub character: char,
    pub foreground: TeamyColor,
    pub background: TeamyColor,
    pub inverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Facet)]
pub struct TeamyDisplayCell {
    pub row: usize,
    pub column: usize,
    pub character: char,
    pub foreground: TeamyColor,
    pub background: TeamyColor,
    pub inverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct TeamyTraceEvent {
    pub action: String,
    pub row: usize,
    pub column: usize,
    pub text: Option<String>,
    pub count: Option<usize>,
    pub parameters: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct TeamyDisplayRow {
    pub row: usize,
    pub glyphs: Vec<TeamyDisplayGlyph>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Facet)]
pub struct TeamyDisplayCursor {
    pub row: usize,
    pub column: usize,
    pub style: TeamyCursorStyle,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Facet)]
pub enum TeamyCursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct TeamyDisplayState {
    pub cols: usize,
    pub rows: usize,
    pub visible_rows: Vec<TeamyDisplayRow>,
    pub cursor: TeamyDisplayCursor,
    pub cursor_visible: bool,
    pub total_rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct TeamyTraceSnapshot {
    pub events: Vec<TeamyTraceEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamyViewportMetrics {
    pub total: u64,
    pub offset: u64,
    pub visible: u64,
    pub scrollback: usize,
}

#[expect(
    missing_debug_implementations,
    reason = "contains a callback field that is intentionally not debug-printable"
)]
pub struct TeamyTerminalEngine {
    cols: usize,
    rows: usize,
    max_scrollback: usize,
    visible_rows: Vec<Vec<TeamyCell>>,
    scrollback_rows: Vec<Vec<TeamyCell>>,
    viewport_offset: usize,
    cursor_col: usize,
    cursor_row: usize,
    current_style: TeamyCellStyle,
    cursor_style: TeamyCursorStyle,
    cursor_visible: bool,
    alternate_screen: Option<TeamySavedScreen>,
    saved_cursor: Option<TeamySavedCursor>,
    pending_utf8: Vec<u8>,
    escape_state: EscapeState,
    trace_events: Vec<TeamyTraceEvent>,
    warned_unsupported_csi: Vec<(String, String, char)>,
    pty_write_effect: Option<PtyWriteEffect>,
}

impl TeamyTerminalEngine {
    #[must_use]
    pub fn new(cols: u16, rows: u16, max_scrollback: usize) -> Self {
        let cols = usize::from(cols.max(1));
        let rows = usize::from(rows.max(1));
        Self {
            cols,
            rows,
            max_scrollback,
            visible_rows: vec![vec![TeamyCell::blank(); cols]; rows],
            scrollback_rows: Vec::new(),
            viewport_offset: 0,
            cursor_col: 0,
            cursor_row: 0,
            current_style: TeamyCellStyle::default(),
            cursor_style: TeamyCursorStyle::Block,
            cursor_visible: true,
            alternate_screen: None,
            saved_cursor: None,
            pending_utf8: Vec::new(),
            escape_state: EscapeState::Ground,
            trace_events: Vec::new(),
            warned_unsupported_csi: Vec::new(),
            pty_write_effect: None,
        }
    }

    pub fn on_pty_write<F>(&mut self, effect: F)
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        self.pty_write_effect = Some(Box::new(effect));
    }

    pub fn vt_write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        let () = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("teamy_terminal_vt_write").entered();

            self.pending_utf8.extend_from_slice(bytes);
            loop {
                match std::str::from_utf8(&self.pending_utf8) {
                    Ok(valid) => {
                        let text = valid.to_owned();
                        self.pending_utf8.clear();
                        self.process_text(&text);
                        break;
                    }
                    Err(error) => {
                        let valid_up_to = error.valid_up_to();
                        if valid_up_to > 0 {
                            let Ok(text) = std::str::from_utf8(&self.pending_utf8[..valid_up_to])
                                .map(str::to_owned)
                            else {
                                break;
                            };
                            self.pending_utf8.drain(..valid_up_to);
                            self.process_text(&text);
                            continue;
                        }

                        if let Some(error_len) = error.error_len() {
                            self.write_character('\u{FFFD}');
                            self.pending_utf8.drain(..error_len);
                            continue;
                        }

                        break;
                    }
                }
            }
        };
    }

    #[must_use]
    pub fn visible_text(&self) -> String {
        let mut lines = self
            .viewport_rows()
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.character)
                    .collect::<String>()
                    .trim_end_matches(' ')
                    .to_owned()
            })
            .collect::<Vec<_>>();

        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }

        lines.join("\n")
    }

    #[must_use]
    pub fn total_rows(&self) -> usize {
        self.scrollback_rows.len() + self.visible_rows.len()
    }

    #[must_use]
    pub fn viewport_metrics(&self) -> TeamyViewportMetrics {
        let total = self.total_rows();
        let visible = self.rows.min(total.max(1));
        let max_offset = total.saturating_sub(visible);
        let offset = self.viewport_offset.min(max_offset);

        TeamyViewportMetrics {
            total: u64::try_from(total).unwrap_or(u64::MAX),
            offset: u64::try_from(offset).unwrap_or(u64::MAX),
            visible: u64::try_from(visible).unwrap_or(u64::MAX),
            scrollback: self.scrollback_rows.len(),
        }
    }

    #[must_use]
    pub fn cursor_screen_position(&self) -> TeamyDisplayCursor {
        TeamyDisplayCursor {
            row: self.scrollback_rows.len().saturating_add(self.cursor_row),
            column: self.cursor_col.min(self.cols.saturating_sub(1)),
            style: self.cursor_style,
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = usize::from(cols.max(1));
        let rows = usize::from(rows.max(1));
        let was_bottom_anchored = self.is_viewport_bottom_anchored();

        if cols != self.cols {
            for row in &mut self.scrollback_rows {
                row.resize(cols, TeamyCell::blank());
            }
            for row in &mut self.visible_rows {
                row.resize(cols, TeamyCell::blank());
            }
            self.cols = cols;
            self.cursor_col = self.cursor_col.min(self.cols.saturating_sub(1));
        }

        if rows != self.rows {
            if rows > self.rows {
                self.visible_rows
                    .extend((0..(rows - self.rows)).map(|_| vec![TeamyCell::blank(); self.cols]));
            } else {
                let rows_to_scroll = self.cursor_row.saturating_add(1).saturating_sub(rows);
                for _ in 0..rows_to_scroll {
                    if let Some(first_visible) = self.visible_rows.first().cloned() {
                        self.scrollback_rows.push(first_visible);
                    }
                    if !self.visible_rows.is_empty() {
                        self.visible_rows.remove(0);
                    }
                }
                self.trim_scrollback();
                self.cursor_row = self.cursor_row.saturating_sub(rows_to_scroll);

                if self.visible_rows.len() > rows {
                    self.visible_rows.truncate(rows);
                }
            }

            self.rows = rows;
            self.cursor_row = self.cursor_row.min(self.rows.saturating_sub(1));
            if self.visible_rows.len() < self.rows {
                self.visible_rows.extend(
                    (0..(self.rows - self.visible_rows.len()))
                        .map(|_| vec![TeamyCell::blank(); self.cols]),
                );
            }
        }

        self.clamp_viewport_offset();
        if was_bottom_anchored {
            self.scroll_viewport(ScrollViewport::Bottom);
        }
    }

    pub fn scroll_active_cursor_into_view(&mut self) {
        let cursor = self.cursor_screen_position();
        let viewport = self.viewport_metrics();
        let visible = usize::try_from(viewport.visible)
            .unwrap_or(self.rows)
            .max(1);
        let offset = usize::try_from(viewport.offset).unwrap_or_default();
        let viewport_end = offset.saturating_add(visible);

        if cursor.row < offset {
            self.viewport_offset = cursor.row;
        } else if cursor.row >= viewport_end {
            self.viewport_offset = cursor.row.saturating_add(1).saturating_sub(visible);
        }

        self.clamp_viewport_offset();
    }

    pub fn scroll_viewport(&mut self, viewport: ScrollViewport) {
        let metrics = self.viewport_metrics();
        let max_offset =
            usize::try_from(metrics.total.saturating_sub(metrics.visible)).unwrap_or(usize::MAX);

        self.viewport_offset = match viewport {
            ScrollViewport::Top => 0,
            ScrollViewport::Bottom => max_offset,
            ScrollViewport::Delta(delta) => {
                let current = i128::try_from(self.viewport_offset).unwrap_or(i128::MAX);
                let next = current + delta as i128;
                let max_offset_i128 = i128::try_from(max_offset).unwrap_or(i128::MAX);
                usize::try_from(next.clamp(0, max_offset_i128)).unwrap_or(max_offset)
            }
        };
    }

    #[must_use]
    pub fn screen_row_cells(&self, row: u32) -> Vec<String> {
        let row_index = usize::try_from(row).unwrap_or(usize::MAX);
        self.screen_row(row_index)
            .cloned()
            .unwrap_or_else(|| vec![TeamyCell::blank(); self.cols])
            .into_iter()
            .map(|cell| cell.character.to_string())
            .collect()
    }

    #[must_use]
    pub fn screen_row_display_cells(&self, row: u32) -> Vec<TeamyDisplayCell> {
        let row_index = usize::try_from(row).unwrap_or(usize::MAX);
        self.screen_row(row_index)
            .cloned()
            .unwrap_or_else(|| vec![TeamyCell::blank(); self.cols])
            .into_iter()
            .enumerate()
            .map(|(column, cell)| TeamyDisplayCell {
                row: row_index,
                column,
                character: cell.character,
                foreground: cell.style.foreground,
                background: cell.style.background,
                inverse: cell.style.inverse,
            })
            .collect()
    }

    #[must_use]
    pub fn trace_snapshot(&self) -> TeamyTraceSnapshot {
        TeamyTraceSnapshot {
            events: self.trace_events.clone(),
        }
    }

    #[must_use]
    pub fn display_state(&self) -> TeamyDisplayState {
        {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("teamy_terminal_display_state").entered();

            let viewport = self.viewport_metrics();
            let visible_rows = self
                .viewport_rows()
                .iter()
                .enumerate()
                .map(|(row_index, row)| TeamyDisplayRow {
                    row: row_index,
                    glyphs: row
                        .iter()
                        .enumerate()
                        .filter_map(|(column_index, cell)| {
                            (cell.character != ' ').then_some(TeamyDisplayGlyph {
                                row: row_index,
                                column: column_index,
                                character: cell.character,
                                foreground: cell.style.foreground,
                                background: cell.style.background,
                                inverse: cell.style.inverse,
                            })
                        })
                        .collect(),
                })
                .collect();

            let cursor_screen = self.cursor_screen_position();
            let viewport_offset = usize::try_from(viewport.offset).unwrap_or(usize::MAX);

            TeamyDisplayState {
                cols: self.cols,
                rows: usize::try_from(viewport.visible).unwrap_or(self.rows),
                visible_rows,
                cursor: TeamyDisplayCursor {
                    row: cursor_screen.row.saturating_sub(viewport_offset),
                    column: cursor_screen.column,
                    style: cursor_screen.style,
                },
                cursor_visible: self.cursor_visible,
                total_rows: self.total_rows(),
            }
        }
    }

    fn process_text(&mut self, text: &str) {
        let () = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("teamy_terminal_process_text").entered();

            for character in text.chars() {
                match std::mem::replace(&mut self.escape_state, EscapeState::Ground) {
                    EscapeState::Ground => match character {
                        '\u{1b}' => {
                            self.record_event("escape", None, None, None);
                            self.escape_state = EscapeState::Escape;
                        }
                        '\r' => {
                            self.record_event("carriage-return", None, None, None);
                            self.cursor_col = 0;
                        }
                        '\n' => {
                            self.record_event("line-feed", None, None, None);
                            self.advance_line_feed();
                        }
                        '\t' => {
                            let next_tab_stop = ((self.cursor_col / 8) + 1) * 8;
                            self.record_event(
                                "tab",
                                None,
                                Some(next_tab_stop.saturating_sub(self.cursor_col)),
                                None,
                            );
                            while self.cursor_col < next_tab_stop {
                                self.write_character(' ');
                            }
                        }
                        character if character < ' ' => {}
                        character => self.write_character(character),
                    },
                    EscapeState::Escape => match character {
                        '[' => {
                            self.record_event("csi-start", None, None, None);
                            self.escape_state = EscapeState::Csi {
                                parameters: String::new(),
                                intermediates: String::new(),
                            };
                        }
                        ']' => {
                            self.record_event("osc-start", None, None, None);
                            self.escape_state = EscapeState::Osc(String::new());
                        }
                        _ => {
                            self.record_event(
                                "escape-cancel",
                                Some(character.to_string()),
                                None,
                                None,
                            );
                        }
                    },
                    EscapeState::Csi {
                        mut parameters,
                        mut intermediates,
                    } => {
                        if intermediates.is_empty()
                            && matches!(character, '0'..='9' | ';' | '?' | '>')
                        {
                            parameters.push(character);
                            self.escape_state = EscapeState::Csi {
                                parameters,
                                intermediates,
                            };
                            continue;
                        }

                        if (' '..='/').contains(&character) {
                            intermediates.push(character);
                            self.escape_state = EscapeState::Csi {
                                parameters,
                                intermediates,
                            };
                            continue;
                        }

                        self.apply_csi(&parameters, &intermediates, character);
                    }
                    EscapeState::Osc(mut payload) => match character {
                        '\u{07}' => self.apply_osc(&payload),
                        '\u{1b}' => self.escape_state = EscapeState::OscEscape(payload),
                        _ => {
                            payload.push(character);
                            self.escape_state = EscapeState::Osc(payload);
                        }
                    },
                    EscapeState::OscEscape(mut payload) => {
                        if character == '\\' {
                            self.apply_osc(&payload);
                        } else {
                            payload.push('\u{1b}');
                            payload.push(character);
                            self.escape_state = EscapeState::Osc(payload);
                        }
                    }
                }
            }
        };
    }

    fn apply_osc(&mut self, payload: &str) {
        self.record_event("osc", Some(payload.to_owned()), None, None);
    }

    fn apply_csi(&mut self, parameters: &str, intermediates: &str, final_byte: char) {
        let () = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("teamy_terminal_apply_csi").entered();

            self.record_event(
                "csi",
                Some(final_byte.to_string()),
                None,
                Some(if intermediates.is_empty() {
                    parameters.to_owned()
                } else {
                    format!("{parameters}|{intermediates}")
                }),
            );

            if intermediates == " " && final_byte == 'q' {
                self.apply_cursor_style(parameters);
                return;
            }

            match final_byte {
                'C' => {
                    let count = parameters
                        .split(';')
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    self.cursor_right(count.max(1));
                }
                'D' => {
                    let count = parameters
                        .split(';')
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    self.cursor_left(count.max(1));
                }
                'G' => {
                    let column = parameters
                        .split(';')
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    self.cursor_horizontal_absolute(column.max(1));
                }
                'H' | 'f' => {
                    let mut fields = parameters.split(';');
                    let row = fields
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    let column = fields
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    self.cursor_position(row.max(1), column.max(1));
                }
                'J' => {
                    let mode = parameters
                        .split(';')
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(0);
                    self.erase_in_display(mode);
                }
                'K' => {
                    let mode = parameters
                        .split(';')
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(0);
                    self.erase_in_line(mode);
                }
                'P' => {
                    let count = parameters
                        .split(';')
                        .next()
                        .filter(|value| !value.is_empty())
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    self.delete_character(count.max(1));
                }
                'h' => self.apply_mode(parameters, true),
                'l' => self.apply_mode(parameters, false),
                't' => self.record_event(
                    "ignored-csi",
                    Some(final_byte.to_string()),
                    None,
                    Some(parameters.to_owned()),
                ),
                'm' => self.apply_sgr(parameters),
                'c' => self.reply_device_attributes(parameters),
                'n' => self.reply_device_status_report(parameters),
                _ => self.warn_unsupported_csi(parameters, intermediates, final_byte),
            }
        };
    }

    fn apply_mode(&mut self, parameters: &str, enabled: bool) {
        self.record_event(
            if enabled { "set-mode" } else { "reset-mode" },
            None,
            None,
            Some(parameters.to_owned()),
        );

        match parameters {
            "?25" => self.cursor_visible = enabled,
            "?1047" | "?1049" => {
                if enabled {
                    if parameters == "?1049" {
                        self.save_cursor_state();
                    }
                    self.enter_alternate_screen();
                } else {
                    self.exit_alternate_screen();
                    if parameters == "?1049" {
                        self.restore_cursor_state();
                    }
                }
            }
            "?1048" => {
                if enabled {
                    self.save_cursor_state();
                } else {
                    self.restore_cursor_state();
                }
            }
            _ => self.record_event(
                "ignored-csi",
                Some(if enabled {
                    "h".to_owned()
                } else {
                    "l".to_owned()
                }),
                None,
                Some(parameters.to_owned()),
            ),
        }
    }

    fn apply_cursor_style(&mut self, parameters: &str) {
        self.record_event("cursor-style", None, None, Some(parameters.to_owned()));
        self.cursor_style = match parameters.parse::<u8>().unwrap_or(0) {
            3 | 4 => TeamyCursorStyle::Underline,
            5 | 6 => TeamyCursorStyle::Bar,
            _ => TeamyCursorStyle::Block,
        };
    }

    fn apply_sgr(&mut self, parameters: &str) {
        self.record_event("sgr", None, None, Some(parameters.to_owned()));

        let mut fields = if parameters.is_empty() {
            vec![0]
        } else {
            parameters
                .split(';')
                .map(|field| field.parse::<u16>().unwrap_or(0))
                .collect::<Vec<_>>()
        }
        .into_iter();

        while let Some(code) = fields.next() {
            match code {
                0 => self.current_style = TeamyCellStyle::default(),
                7 => self.current_style.inverse = true,
                27 => self.current_style.inverse = false,
                30..=37 => self.current_style.foreground = TeamyColor::Indexed((code - 30) as u8),
                39 => self.current_style.foreground = TeamyColor::Default,
                40..=47 => self.current_style.background = TeamyColor::Indexed((code - 40) as u8),
                49 => self.current_style.background = TeamyColor::Default,
                90..=97 => {
                    self.current_style.foreground = TeamyColor::Indexed((code - 90 + 8) as u8)
                }
                100..=107 => {
                    self.current_style.background = TeamyColor::Indexed((code - 100 + 8) as u8)
                }
                38 => {
                    if let Some(color) = parse_extended_sgr_color(&mut fields) {
                        self.current_style.foreground = color;
                    }
                }
                48 => {
                    if let Some(color) = parse_extended_sgr_color(&mut fields) {
                        self.current_style.background = color;
                    }
                }
                _ => {}
            }
        }
    }

    fn reply_device_attributes(&mut self, parameters: &str) {
        let response = match parameters {
            "" | "0" => Some(Cow::Borrowed(b"\x1b[?62;c".as_slice())),
            ">" | ">0" => Some(Cow::Borrowed(b"\x1b[>0;0;0c".as_slice())),
            _ => None,
        };

        if let Some(response) = response {
            self.record_event(
                "reply-device-attributes",
                Some(String::from_utf8_lossy(response.as_ref()).into_owned()),
                Some(response.len()),
                Some(parameters.to_owned()),
            );
            self.emit_pty_write(response.as_ref());
        } else {
            self.warn_unsupported_csi(parameters, "", 'c');
        }
    }

    fn reply_device_status_report(&mut self, parameters: &str) {
        let row = self.cursor_row.saturating_add(1);
        let column = self
            .cursor_col
            .min(self.cols.saturating_sub(1))
            .saturating_add(1);
        let response = match parameters {
            "6" => Some(format!("\x1b[{row};{column}R")),
            "?6" => Some(format!("\x1b[?{row};{column}R")),
            _ => None,
        };

        if let Some(response) = response {
            self.record_event(
                "reply-device-status-report",
                Some(response.clone()),
                Some(response.len()),
                Some(parameters.to_owned()),
            );
            self.emit_pty_write(response.as_bytes());
        } else {
            self.warn_unsupported_csi(parameters, "", 'n');
        }
    }

    fn warn_unsupported_csi(&mut self, parameters: &str, intermediates: &str, final_byte: char) {
        let key = (parameters.to_owned(), intermediates.to_owned(), final_byte);
        if self.warned_unsupported_csi.contains(&key) {
            return;
        }

        self.warned_unsupported_csi.push(key);
        warn!(
            parameters,
            intermediates,
            final_byte = %final_byte,
            requires_terminal_response =
                csi_likely_requires_terminal_response(parameters, final_byte),
            "teamy terminal received unsupported CSI sequence"
        );
    }

    fn emit_pty_write(&mut self, bytes: &[u8]) {
        let Some(effect) = self.pty_write_effect.as_mut() else {
            return;
        };

        effect(bytes);
    }

    fn cursor_left(&mut self, count: usize) {
        self.record_event("cursor-left", None, Some(count), None);
        self.cursor_col = self.cursor_col.saturating_sub(count);
    }

    fn cursor_right(&mut self, count: usize) {
        self.record_event("cursor-right", None, Some(count), None);
        self.cursor_col = self.cursor_col.saturating_add(count).min(self.cols);
    }

    fn cursor_horizontal_absolute(&mut self, column: usize) {
        self.record_event("cursor-horizontal-absolute", None, Some(column), None);
        self.cursor_col = column.saturating_sub(1).min(self.cols.saturating_sub(1));
    }

    fn cursor_position(&mut self, row: usize, column: usize) {
        self.record_event(
            "cursor-position",
            None,
            None,
            Some(format!("{row};{column}")),
        );
        self.cursor_row = row.saturating_sub(1).min(self.rows.saturating_sub(1));
        self.cursor_col = column.saturating_sub(1).min(self.cols.saturating_sub(1));
    }

    fn erase_in_display(&mut self, mode: usize) {
        self.record_event("erase-in-display", None, Some(mode), None);
        match mode {
            0 => {
                self.erase_in_line(0);
                for row in self
                    .visible_rows
                    .iter_mut()
                    .skip(self.cursor_row.saturating_add(1))
                {
                    row.fill(TeamyCell::blank_with_style(self.current_style));
                }
            }
            1 => {
                for row in self.visible_rows.iter_mut().take(self.cursor_row) {
                    row.fill(TeamyCell::blank_with_style(self.current_style));
                }
                if let Some(row) = self.visible_rows.get_mut(self.cursor_row) {
                    let end = self.cursor_col.min(self.cols.saturating_sub(1));
                    for cell in row.iter_mut().take(end.saturating_add(1)) {
                        *cell = TeamyCell::blank_with_style(self.current_style);
                    }
                }
            }
            2 | 3 => {
                for row in &mut self.visible_rows {
                    row.fill(TeamyCell::blank_with_style(self.current_style));
                }
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: usize) {
        self.record_event("erase-in-line", None, Some(mode), None);
        let Some(row) = self.visible_rows.get_mut(self.cursor_row) else {
            return;
        };

        match mode {
            0 => {
                for cell in row.iter_mut().skip(self.cursor_col.min(self.cols)) {
                    *cell = TeamyCell::blank_with_style(self.current_style);
                }
            }
            1 => {
                let end = self.cursor_col.min(self.cols.saturating_sub(1));
                for cell in row.iter_mut().take(end.saturating_add(1)) {
                    *cell = TeamyCell::blank_with_style(self.current_style);
                }
            }
            2 => row.fill(TeamyCell::blank_with_style(self.current_style)),
            _ => {}
        }
    }

    fn delete_character(&mut self, count: usize) {
        self.record_event("delete-character", None, Some(count), None);
        let Some(row) = self.visible_rows.get_mut(self.cursor_row) else {
            return;
        };

        let start = self.cursor_col.min(self.cols);
        let count = count.min(self.cols.saturating_sub(start));
        if count == 0 {
            return;
        }

        let tail_start = start.saturating_add(count);
        let tail_len = self.cols.saturating_sub(tail_start);
        if tail_len > 0 {
            row.copy_within(tail_start..tail_start + tail_len, start);
        }
        for cell in row.iter_mut().skip(self.cols.saturating_sub(count)) {
            *cell = TeamyCell::blank_with_style(self.current_style);
        }
    }

    fn write_character(&mut self, character: char) {
        if self.cursor_col >= self.cols {
            self.wrap_line();
        }

        self.record_event("write-character", Some(character.to_string()), None, None);

        if let Some(row) = self.visible_rows.get_mut(self.cursor_row)
            && let Some(cell) = row.get_mut(self.cursor_col)
        {
            *cell = TeamyCell {
                character,
                style: self.current_style,
            };
        }

        self.cursor_col += 1;
    }

    fn wrap_line(&mut self) {
        self.record_event("wrap-line", None, None, None);
        self.cursor_col = 0;
        self.advance_row();
    }

    fn advance_line_feed(&mut self) {
        self.advance_row();
        self.cursor_col = self.cursor_col.min(self.cols.saturating_sub(1));
    }

    fn advance_row(&mut self) {
        let was_bottom_anchored = self.is_viewport_bottom_anchored();
        if self.cursor_row + 1 < self.rows {
            self.cursor_row += 1;
            return;
        }

        self.record_event("scroll-row", None, None, None);

        if let Some(first_visible) = self.visible_rows.first().cloned() {
            self.scrollback_rows.push(first_visible);
            self.trim_scrollback();
        }
        self.visible_rows.rotate_left(1);
        if let Some(last_row) = self.visible_rows.last_mut() {
            last_row.fill(TeamyCell::blank_with_style(self.current_style));
        }

        self.clamp_viewport_offset();
        if was_bottom_anchored {
            self.scroll_viewport(ScrollViewport::Bottom);
        }
    }

    fn save_cursor_state(&mut self) {
        self.saved_cursor = Some(TeamySavedCursor {
            col: self.cursor_col,
            row: self.cursor_row,
            style: self.cursor_style,
            visible: self.cursor_visible,
        });
    }

    fn restore_cursor_state(&mut self) {
        let Some(saved_cursor) = self.saved_cursor.take() else {
            return;
        };

        self.cursor_col = saved_cursor.col.min(self.cols.saturating_sub(1));
        self.cursor_row = saved_cursor.row.min(self.rows.saturating_sub(1));
        self.cursor_style = saved_cursor.style;
        self.cursor_visible = saved_cursor.visible;
    }

    fn enter_alternate_screen(&mut self) {
        if self.alternate_screen.is_some() {
            return;
        }

        self.alternate_screen = Some(TeamySavedScreen {
            visible_rows: self.visible_rows.clone(),
            scrollback_rows: self.scrollback_rows.clone(),
            viewport_offset: self.viewport_offset,
            cursor_col: self.cursor_col,
            cursor_row: self.cursor_row,
            current_style: self.current_style,
            cursor_style: self.cursor_style,
            cursor_visible: self.cursor_visible,
        });

        self.visible_rows = vec![vec![TeamyCell::blank(); self.cols]; self.rows];
        self.scrollback_rows.clear();
        self.viewport_offset = 0;
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.current_style = TeamyCellStyle::default();
        self.cursor_style = TeamyCursorStyle::Block;
        self.cursor_visible = true;
    }

    fn exit_alternate_screen(&mut self) {
        let Some(saved_screen) = self.alternate_screen.take() else {
            return;
        };

        self.visible_rows = saved_screen.visible_rows;
        self.scrollback_rows = saved_screen.scrollback_rows;
        self.viewport_offset = saved_screen.viewport_offset;
        self.cursor_col = saved_screen.cursor_col;
        self.cursor_row = saved_screen.cursor_row;
        self.current_style = saved_screen.current_style;
        self.cursor_style = saved_screen.cursor_style;
        self.cursor_visible = saved_screen.cursor_visible;
        self.clamp_viewport_offset();
    }

    fn record_event(
        &mut self,
        action: &str,
        text: Option<String>,
        count: Option<usize>,
        parameters: Option<String>,
    ) {
        self.trace_events.push(TeamyTraceEvent {
            action: action.to_owned(),
            row: self.cursor_row,
            column: self.cursor_col,
            text,
            count,
            parameters,
        });
    }

    fn viewport_rows(&self) -> Vec<&Vec<TeamyCell>> {
        let metrics = self.viewport_metrics();
        let start = usize::try_from(metrics.offset).unwrap_or_default();
        let visible = usize::try_from(metrics.visible).unwrap_or(self.rows);
        (start..start.saturating_add(visible))
            .filter_map(|row_index| self.screen_row(row_index))
            .collect()
    }

    fn screen_row(&self, row_index: usize) -> Option<&Vec<TeamyCell>> {
        if row_index < self.scrollback_rows.len() {
            self.scrollback_rows.get(row_index)
        } else {
            self.visible_rows
                .get(row_index.saturating_sub(self.scrollback_rows.len()))
        }
    }

    fn trim_scrollback(&mut self) {
        if self.scrollback_rows.len() > self.max_scrollback {
            let overflow = self.scrollback_rows.len() - self.max_scrollback;
            self.scrollback_rows.drain(..overflow);
        }
    }

    fn clamp_viewport_offset(&mut self) {
        let total = self.total_rows();
        let visible = self.rows.min(total.max(1));
        let max_offset = total.saturating_sub(visible);
        self.viewport_offset = self.viewport_offset.min(max_offset);
    }

    fn is_viewport_bottom_anchored(&self) -> bool {
        let metrics = self.viewport_metrics();
        usize::try_from(metrics.offset.saturating_add(metrics.visible)).unwrap_or(usize::MAX)
            >= self.total_rows()
    }
}

fn parse_extended_sgr_color(parameters: &mut impl Iterator<Item = u16>) -> Option<TeamyColor> {
    match parameters.next()? {
        2 => {
            let r = u8::try_from(parameters.next()?).ok()?;
            let g = u8::try_from(parameters.next()?).ok()?;
            let b = u8::try_from(parameters.next()?).ok()?;
            Some(TeamyColor::Rgb { r, g, b })
        }
        5 => Some(TeamyColor::Indexed(u8::try_from(parameters.next()?).ok()?)),
        _ => None,
    }
}

fn csi_likely_requires_terminal_response(parameters: &str, final_byte: char) -> bool {
    parameters.starts_with('?')
        || parameters.starts_with('>')
        || matches!(final_byte, 'c' | 'n' | 'u')
}

#[cfg(test)]
mod tests {
    use libghostty_vt::terminal::ScrollViewport;
    use std::sync::{Arc, Mutex};

    use super::{TeamyColor, TeamyCursorStyle, TeamyTerminalEngine};

    fn capture_pty_writes(engine: &mut TeamyTerminalEngine) -> Arc<Mutex<Vec<Vec<u8>>>> {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let writes_for_effect = Arc::clone(&writes);
        engine.on_pty_write(move |bytes| {
            if let Ok(mut writes) = writes_for_effect.lock() {
                writes.push(bytes.to_vec());
            }
        });
        writes
    }

    #[test]
    fn visible_text_keeps_simple_lines() {
        let mut engine = TeamyTerminalEngine::new(20, 4, 64);
        engine.vt_write(b"hello\r\nworld\r\n");

        assert_eq!(engine.visible_text(), "hello\nworld");
    }

    #[test]
    fn wrapping_moves_to_next_row() {
        let mut engine = TeamyTerminalEngine::new(5, 4, 64);
        engine.vt_write(b"abcdef");

        assert_eq!(engine.visible_text(), "abcde\nf");
    }

    #[test]
    fn carriage_return_overwrites_from_line_start() {
        let mut engine = TeamyTerminalEngine::new(5, 4, 64);
        engine.vt_write(b"abcde\rZ");

        assert_eq!(engine.visible_text(), "Zbcde");
    }

    #[test]
    fn scrollback_keeps_bounded_history() {
        let mut engine = TeamyTerminalEngine::new(4, 2, 2);
        engine.vt_write(b"one\r\ntwo\r\nthree\r\nfour\r\n");

        assert_eq!(engine.visible_text(), "four");
        assert_eq!(engine.total_rows(), 4);
    }

    #[test]
    fn partial_utf8_is_buffered_until_complete() {
        let mut engine = TeamyTerminalEngine::new(8, 2, 8);
        engine.vt_write(&[0xF0, 0x9F]);
        engine.vt_write(&[0xA6, 0x80]);

        assert_eq!(engine.visible_text(), "🦀");
    }

    #[test]
    fn tabs_advance_to_eight_column_stops() {
        let mut engine = TeamyTerminalEngine::new(12, 2, 8);
        engine.vt_write(b"a\tb");

        assert_eq!(engine.visible_text(), "a       b");
    }

    #[test]
    fn bare_line_feed_preserves_cursor_column() {
        let mut engine = TeamyTerminalEngine::new(6, 3, 8);
        engine.vt_write(b"1\n2\n3");

        assert_eq!(engine.visible_text(), "1\n 2\n  3");
    }

    #[test]
    fn csi_erase_entire_line_clears_before_redraw() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[2K\rZ");

        assert_eq!(engine.visible_text(), "Z");
    }

    #[test]
    fn csi_sequence_split_across_writes_is_buffered() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[");
        engine.vt_write(b"2K\rZ");

        assert_eq!(engine.visible_text(), "Z");
    }

    #[test]
    fn csi_cursor_left_moves_back_within_row() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[2DZ");

        assert_eq!(engine.visible_text(), "aZc");
    }

    #[test]
    fn csi_cursor_left_defaults_to_one_cell() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[DZ");

        assert_eq!(engine.visible_text(), "abZ");
    }

    #[test]
    fn csi_cursor_horizontal_absolute_moves_to_requested_column() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[2GZ");

        assert_eq!(engine.visible_text(), "aZc");
    }

    #[test]
    fn csi_cursor_horizontal_absolute_defaults_to_first_column() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[GZ");

        assert_eq!(engine.visible_text(), "Zbc");
    }

    #[test]
    fn csi_cursor_position_moves_to_requested_row_and_column() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[2;3HZ");

        assert_eq!(engine.visible_text(), "abc\n  Z");
    }

    #[test]
    fn csi_erase_display_clears_visible_screen() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\r\nxyz\x1b[H\x1b[2J");

        assert_eq!(engine.visible_text(), "");
    }

    #[test]
    fn raw_prompt_marker_text_without_escape_stays_visible() {
        let mut engine = TeamyTerminalEngine::new(32, 3, 8);
        engine.vt_write(b"133;D;0133;A133;B~");

        assert_eq!(engine.visible_text(), "133;D;0133;A133;B~");
    }

    #[test]
    fn osc_133_prompt_markers_do_not_render_into_visible_text() {
        let mut engine = TeamyTerminalEngine::new(32, 3, 8);
        engine.vt_write(b"\x1b]133;D;0\x07\x1b]133;A\x07\x1b]133;B\x07~");

        assert_eq!(engine.visible_text(), "~");
    }

    #[test]
    fn osc_title_sequence_does_not_render_into_visible_text() {
        let mut engine = TeamyTerminalEngine::new(32, 3, 8);
        engine.vt_write(b"\x1b]0;C:\\Program Files\\PowerShell\\7\\pwsh.EXE\x07~");

        assert_eq!(engine.visible_text(), "~");
    }

    #[test]
    fn osc_sequence_split_across_writes_is_buffered_until_terminated() {
        let mut engine = TeamyTerminalEngine::new(32, 3, 8);
        engine.vt_write(b"\x1b]133;A");
        engine.vt_write(b"\x07\x1b]133;B");
        engine.vt_write(b"\x1b\\~");

        assert_eq!(engine.visible_text(), "~");
    }

    #[test]
    fn combined_csi_redraw_sequence_rewrites_current_line() {
        let mut engine = TeamyTerminalEngine::new(16, 3, 8);
        engine.vt_write(b"value: old\x1b[8G\x1b[Knew");

        assert_eq!(engine.visible_text(), "value: new");
    }

    #[test]
    fn csi_delete_character_shifts_remaining_text_left() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abcXYZ\x1b[3D\x1b[P");

        assert_eq!(engine.visible_text(), "abcYZ");
    }

    #[test]
    fn csi_delete_character_defaults_to_one_cell() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abcd\x1b[2D\x1b[PZ");

        assert_eq!(engine.visible_text(), "abZ");
    }

    #[test]
    fn csi_cursor_right_moves_forward_within_row() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[2CZ");

        assert_eq!(engine.visible_text(), "abc  Z");
    }

    #[test]
    fn csi_cursor_right_defaults_to_one_cell() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"abc\x1b[CZ");

        assert_eq!(engine.visible_text(), "abc Z");
    }

    #[test]
    fn csi_cursor_position_report_emits_terminal_reply() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        let writes = capture_pty_writes(&mut engine);

        engine.vt_write(b"abc\x1b[6n");

        let writes = writes.lock().expect("writes mutex should not be poisoned");
        assert_eq!(writes.as_slice(), [b"\x1b[1;4R".to_vec()]);
    }

    #[test]
    fn csi_private_cursor_position_report_emits_private_reply() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        let writes = capture_pty_writes(&mut engine);

        engine.vt_write(b"abc\x1b[?6n");

        let writes = writes.lock().expect("writes mutex should not be poisoned");
        assert_eq!(writes.as_slice(), [b"\x1b[?1;4R".to_vec()]);
    }

    #[test]
    fn csi_primary_device_attributes_emit_reply() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        let writes = capture_pty_writes(&mut engine);

        engine.vt_write(b"\x1b[c");

        let writes = writes.lock().expect("writes mutex should not be poisoned");
        assert_eq!(writes.as_slice(), [b"\x1b[?62;c".to_vec()]);
    }

    #[test]
    fn csi_secondary_device_attributes_emit_reply() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        let writes = capture_pty_writes(&mut engine);

        engine.vt_write(b"\x1b[>c");

        let writes = writes.lock().expect("writes mutex should not be poisoned");
        assert_eq!(writes.as_slice(), [b"\x1b[>0;0;0c".to_vec()]);
    }

    #[test]
    fn display_state_exports_glyph_positions_and_cursor() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 8);
        engine.vt_write(b"ab\nZ");

        let display = engine.display_state();

        assert_eq!(display.total_rows, 3);
        assert_eq!(display.cursor.row, 1);
        assert_eq!(display.cursor.column, 3);
        assert_eq!(display.visible_rows[0].glyphs.len(), 2);
        assert_eq!(display.visible_rows[1].glyphs.len(), 1);
        assert_eq!(display.visible_rows[1].glyphs[0].column, 2);
        assert_eq!(display.visible_rows[1].glyphs[0].character, 'Z');
        assert_eq!(
            display.visible_rows[1].glyphs[0].foreground,
            TeamyColor::Default
        );
        assert_eq!(display.cursor.style, TeamyCursorStyle::Block);
        assert!(display.cursor_visible);
    }

    #[test]
    fn sgr_truecolor_and_indexed_colors_are_exported() {
        let mut engine = TeamyTerminalEngine::new(16, 3, 8);
        engine.vt_write(b"\x1b[38;2;12;34;56mA\x1b[48;5;196mB\x1b[0mC");

        let display = engine.display_state();

        assert_eq!(
            display.visible_rows[0].glyphs[0].foreground,
            TeamyColor::Rgb {
                r: 12,
                g: 34,
                b: 56,
            }
        );
        assert_eq!(
            display.visible_rows[0].glyphs[1].background,
            TeamyColor::Indexed(196)
        );
        assert_eq!(
            display.visible_rows[0].glyphs[2].foreground,
            TeamyColor::Default
        );
    }

    #[test]
    fn screen_row_display_cells_keep_backgrounds_for_space_cells() {
        let mut engine = TeamyTerminalEngine::new(8, 2, 8);
        engine.vt_write(b"\x1b[48;2;1;2;3m \x1b[0m");

        let cells = engine.screen_row_display_cells(0);

        assert_eq!(cells[0].background, TeamyColor::Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(cells[0].character, ' ');
    }

    #[test]
    fn csi_cursor_style_sequence_with_space_intermediate_updates_cursor_style() {
        let mut engine = TeamyTerminalEngine::new(8, 2, 8);
        engine.vt_write(b"\x1b[6 q");

        assert_eq!(engine.display_state().cursor.style, TeamyCursorStyle::Bar);
    }

    #[test]
    fn alternate_screen_restore_recovers_normal_screen() {
        let mut engine = TeamyTerminalEngine::new(16, 4, 16);
        engine.vt_write(b"shell\r\nprompt\r\n");

        engine.vt_write(b"\x1b[?1049hhello from alt");
        assert!(engine.visible_text().contains("hello from alt"));

        engine.vt_write(b"\x1b[?1049l");

        assert_eq!(engine.visible_text(), "shell\nprompt");
    }

    #[test]
    fn private_cursor_visibility_mode_hides_cursor_until_restored() {
        let mut engine = TeamyTerminalEngine::new(8, 2, 8);
        engine.vt_write(b"\x1b[?25l");
        assert!(!engine.display_state().cursor_visible);

        engine.vt_write(b"\x1b[?25h");
        assert!(engine.display_state().cursor_visible);
    }

    #[test]
    fn trace_snapshot_records_csi_redraw_operations() {
        let mut engine = TeamyTerminalEngine::new(16, 3, 8);
        engine.vt_write(b"value: old\x1b[8G\x1b[Knew");

        let trace = engine.trace_snapshot();
        assert!(
            trace
                .events
                .iter()
                .any(|event| event.action == "cursor-horizontal-absolute")
        );
        assert!(
            trace
                .events
                .iter()
                .any(|event| event.action == "erase-in-line")
        );
        assert!(trace.events.iter().any(|event| {
            event.action == "write-character" && event.text.as_deref() == Some("n")
        }));
    }

    #[test]
    fn viewport_scrolling_exposes_scrollback_rows() {
        let mut engine = TeamyTerminalEngine::new(6, 2, 8);
        engine.vt_write(b"one\r\ntwo\r\nthree\r\n");

        engine.scroll_viewport(ScrollViewport::Top);

        let display = engine.display_state();
        assert_eq!(display.visible_rows[0].glyphs[0].character, 'o');
        assert_eq!(display.visible_rows[1].glyphs[0].character, 't');
    }

    #[test]
    fn resize_preserves_bottom_anchor_for_live_viewport() {
        let mut engine = TeamyTerminalEngine::new(6, 2, 8);
        engine.vt_write(b"one\r\ntwo\r\nthree\r\n");

        engine.resize(6, 3);

        let display = engine.display_state();
        assert_eq!(display.rows, 3);
        assert!(
            display
                .visible_rows
                .iter()
                .any(|row| { row.glyphs.iter().any(|glyph| glyph.character == 't') })
        );
        assert!(
            display
                .visible_rows
                .iter()
                .any(|row| { row.glyphs.iter().any(|glyph| glyph.character == 'h') })
        );
    }

    #[test]
    fn resize_preserves_cursor_screen_row_when_blank_rows_are_below_cursor() {
        let mut engine = TeamyTerminalEngine::new(8, 5, 16);
        engine.vt_write(b"~\r\n> ");

        let before = engine.cursor_screen_position();
        engine.resize(8, 2);
        let after_shrink = engine.cursor_screen_position();
        engine.resize(8, 5);
        let after_restore = engine.cursor_screen_position();

        assert_eq!(before.row, 1);
        assert_eq!(before.column, 2);
        assert_eq!(after_shrink, before);
        assert_eq!(after_restore, before);
        assert_eq!(engine.visible_text(), "~\n>");
    }

    #[test]
    fn scroll_active_cursor_into_view_reanchors_scrolled_viewport() {
        let mut engine = TeamyTerminalEngine::new(8, 3, 32);
        engine.vt_write(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n");
        engine.scroll_viewport(ScrollViewport::Delta(-2));

        engine.scroll_active_cursor_into_view();

        let metrics = engine.viewport_metrics();
        assert_eq!(metrics.offset + metrics.visible, metrics.total);
    }
}
