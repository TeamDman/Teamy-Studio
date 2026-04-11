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
    Csi(String),
    Osc(String),
    OscEscape(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Facet)]
pub struct TeamyDisplayGlyph {
    pub row: usize,
    pub column: usize,
    pub character: char,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct TeamyDisplayState {
    pub cols: usize,
    pub rows: usize,
    pub visible_rows: Vec<TeamyDisplayRow>,
    pub cursor: TeamyDisplayCursor,
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
    visible_rows: Vec<Vec<char>>,
    scrollback_rows: Vec<Vec<char>>,
    viewport_offset: usize,
    cursor_col: usize,
    cursor_row: usize,
    pending_utf8: Vec<u8>,
    escape_state: EscapeState,
    trace_events: Vec<TeamyTraceEvent>,
    warned_unsupported_csi: Vec<(String, char)>,
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
            visible_rows: vec![vec![' '; cols]; rows],
            scrollback_rows: Vec::new(),
            viewport_offset: 0,
            cursor_col: 0,
            cursor_row: 0,
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

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = usize::from(cols.max(1));
        let rows = usize::from(rows.max(1));
        let was_bottom_anchored = self.is_viewport_bottom_anchored();

        if cols != self.cols {
            for row in &mut self.scrollback_rows {
                row.resize(cols, ' ');
            }
            for row in &mut self.visible_rows {
                row.resize(cols, ' ');
            }
            self.cols = cols;
            self.cursor_col = self.cursor_col.min(self.cols.saturating_sub(1));
        }

        if rows != self.rows {
            if rows > self.rows {
                self.visible_rows
                    .extend((0..(rows - self.rows)).map(|_| vec![' '; self.cols]));
            } else {
                let remove_count = self.rows - rows;
                for _ in 0..remove_count {
                    if let Some(first_visible) = self.visible_rows.first().cloned() {
                        self.scrollback_rows.push(first_visible);
                    }
                    if !self.visible_rows.is_empty() {
                        self.visible_rows.remove(0);
                    }
                }
                self.trim_scrollback();
            }

            self.rows = rows;
            self.cursor_row = self.cursor_row.min(self.rows.saturating_sub(1));
            if self.visible_rows.len() < self.rows {
                self.visible_rows.extend(
                    (0..(self.rows - self.visible_rows.len())).map(|_| vec![' '; self.cols]),
                );
            }
        }

        self.clamp_viewport_offset();
        if was_bottom_anchored {
            self.scroll_viewport(ScrollViewport::Bottom);
        }
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
            .unwrap_or_else(|| vec![' '; self.cols])
            .into_iter()
            .map(|character| character.to_string())
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
                        .filter_map(|(column_index, character)| {
                            (*character != ' ').then_some(TeamyDisplayGlyph {
                                row: row_index,
                                column: column_index,
                                character: *character,
                            })
                        })
                        .collect(),
                })
                .collect();

            let cursor_screen_row = self.scrollback_rows.len().saturating_add(self.cursor_row);
            let viewport_offset = usize::try_from(viewport.offset).unwrap_or(usize::MAX);

            TeamyDisplayState {
                cols: self.cols,
                rows: usize::try_from(viewport.visible).unwrap_or(self.rows),
                visible_rows,
                cursor: TeamyDisplayCursor {
                    row: cursor_screen_row.saturating_sub(viewport_offset),
                    column: self.cursor_col.min(self.cols.saturating_sub(1)),
                },
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
                            self.escape_state = EscapeState::Csi(String::new());
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
                    EscapeState::Csi(mut parameters) => {
                        if matches!(character, '0'..='9' | ';' | '?' | '>') {
                            parameters.push(character);
                            self.escape_state = EscapeState::Csi(parameters);
                            continue;
                        }

                        self.apply_csi(&parameters, character);
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

    fn apply_csi(&mut self, parameters: &str, final_byte: char) {
        let () = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("teamy_terminal_apply_csi").entered();

            self.record_event(
                "csi",
                Some(final_byte.to_string()),
                None,
                Some(parameters.to_owned()),
            );
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
                'h' | 'l' | 'm' | 't' => self.record_event(
                    "ignored-csi",
                    Some(final_byte.to_string()),
                    None,
                    Some(parameters.to_owned()),
                ),
                'c' => self.reply_device_attributes(parameters),
                'n' => self.reply_device_status_report(parameters),
                _ => self.warn_unsupported_csi(parameters, final_byte),
            }
        };
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
            self.warn_unsupported_csi(parameters, 'c');
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
            self.warn_unsupported_csi(parameters, 'n');
        }
    }

    fn warn_unsupported_csi(&mut self, parameters: &str, final_byte: char) {
        let key = (parameters.to_owned(), final_byte);
        if self.warned_unsupported_csi.contains(&key) {
            return;
        }

        self.warned_unsupported_csi.push(key);
        warn!(
            parameters,
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
                    row.fill(' ');
                }
            }
            1 => {
                for row in self.visible_rows.iter_mut().take(self.cursor_row) {
                    row.fill(' ');
                }
                if let Some(row) = self.visible_rows.get_mut(self.cursor_row) {
                    let end = self.cursor_col.min(self.cols.saturating_sub(1));
                    for cell in row.iter_mut().take(end.saturating_add(1)) {
                        *cell = ' ';
                    }
                }
            }
            2 | 3 => {
                for row in &mut self.visible_rows {
                    row.fill(' ');
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
                    *cell = ' ';
                }
            }
            1 => {
                let end = self.cursor_col.min(self.cols.saturating_sub(1));
                for cell in row.iter_mut().take(end.saturating_add(1)) {
                    *cell = ' ';
                }
            }
            2 => row.fill(' '),
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
            *cell = ' ';
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
            *cell = character;
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
            last_row.fill(' ');
        }

        self.clamp_viewport_offset();
        if was_bottom_anchored {
            self.scroll_viewport(ScrollViewport::Bottom);
        }
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

    fn viewport_rows(&self) -> Vec<&Vec<char>> {
        let metrics = self.viewport_metrics();
        let start = usize::try_from(metrics.offset).unwrap_or_default();
        let visible = usize::try_from(metrics.visible).unwrap_or(self.rows);
        (start..start.saturating_add(visible))
            .filter_map(|row_index| self.screen_row(row_index))
            .collect()
    }

    fn screen_row(&self, row_index: usize) -> Option<&Vec<char>> {
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

fn csi_likely_requires_terminal_response(parameters: &str, final_byte: char) -> bool {
    parameters.starts_with('?')
        || parameters.starts_with('>')
        || matches!(final_byte, 'c' | 'n' | 'u')
}

#[cfg(test)]
mod tests {
    use libghostty_vt::terminal::ScrollViewport;
    use std::sync::{Arc, Mutex};

    use super::TeamyTerminalEngine;

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
}
