use super::spatial::{ClientPoint, ClientRect, TerminalCellPoint};
use super::windows_d3d12_renderer::{PanelEffect, RenderScene, push_glyph, push_panel};
use super::windows_terminal::TerminalSelection;
use windows::Win32::Foundation::RECT;

const DEFAULT_TEXT_COLOR: [f32; 4] = [0.96, 0.95, 0.90, 1.0];
const DEFAULT_SELECTION_FOREGROUND: [f32; 4] = [0.06, 0.07, 0.09, 1.0];
const DEFAULT_SELECTION_BACKGROUND: [f32; 4] = [0.42, 0.67, 0.98, 1.0];
const TAB_STOP_COLUMNS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextGridRows {
    rows: Vec<Vec<char>>,
    cols: usize,
}

pub fn build_text_grid_scene(
    text_rect: ClientRect,
    text: &str,
    cell_width: i32,
    cell_height: i32,
    selection: Option<TerminalSelection>,
) -> RenderScene {
    build_text_grid_scene_with_palette(
        text_rect,
        text,
        cell_width,
        cell_height,
        selection,
        DEFAULT_TEXT_COLOR,
        DEFAULT_SELECTION_FOREGROUND,
        DEFAULT_SELECTION_BACKGROUND,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "palette inputs are passed explicitly at call sites"
)]
pub fn build_text_grid_scene_with_palette(
    text_rect: ClientRect,
    text: &str,
    cell_width: i32,
    cell_height: i32,
    selection: Option<TerminalSelection>,
    text_color: [f32; 4],
    selection_foreground: [f32; 4],
    selection_background: [f32; 4],
) -> RenderScene {
    let rows = layout_text_grid_rows(
        text,
        text_rect.width(),
        text_rect.height(),
        cell_width,
        cell_height,
    );
    let mut scene = RenderScene {
        panels: Vec::new(),
        glyphs: Vec::new(),
        sprites: Vec::new(),
        overlay_panels: Vec::new(),
    };

    if rows.cols == 0 {
        return scene;
    }

    for (row_index, row) in rows.rows.iter().enumerate() {
        let row_index = i32::try_from(row_index).unwrap_or(i32::MAX);
        let visible_cols = row
            .len()
            .max(selection_row_width(selection, row_index, rows.cols));
        for column_index in 0..visible_cols.min(rows.cols) {
            let column_index = i32::try_from(column_index).unwrap_or(i32::MAX);
            let cell = TerminalCellPoint::new(column_index, row_index);
            let selected = selection.is_some_and(|selection| selection.contains(cell));
            let rect = cell_rect(text_rect, cell, cell_width, cell_height);
            if selected {
                push_panel(
                    &mut scene,
                    rect,
                    selection_background,
                    PanelEffect::TerminalFill,
                );
            }

            if let Some(character) = row.get(usize::try_from(column_index).unwrap_or_default())
                && *character != ' '
            {
                push_glyph(
                    &mut scene,
                    rect,
                    *character,
                    if selected {
                        selection_foreground
                    } else {
                        text_color
                    },
                );
            }
        }
    }

    scene
}

pub fn extract_selected_text(
    text_rect: ClientRect,
    text: &str,
    cell_width: i32,
    cell_height: i32,
    selection: TerminalSelection,
) -> String {
    let rows = layout_text_grid_rows(text, text_rect.width(), i32::MAX, cell_width, cell_height);
    let mut selected_rows = Vec::new();

    for (row_index, row) in rows.rows.iter().enumerate() {
        let row_index = i32::try_from(row_index).unwrap_or(i32::MAX);
        let mut row_buffer = String::new();
        let mut row_has_selection = false;
        let visible_cols =
            row.len()
                .max(selection_row_width(Some(selection), row_index, rows.cols));
        for column_index in 0..visible_cols.min(rows.cols) {
            let column_index = i32::try_from(column_index).unwrap_or(i32::MAX);
            let cell = TerminalCellPoint::new(column_index, row_index);
            if !selection.contains(cell) {
                continue;
            }

            row_has_selection = true;
            row_buffer.push(
                row.get(usize::try_from(column_index).unwrap_or_default())
                    .copied()
                    .unwrap_or(' '),
            );
        }

        if row_has_selection {
            selected_rows.push(row_buffer);
        }
    }

    selected_rows.join("\n")
}

pub fn cell_from_client_point(
    text_rect: ClientRect,
    point: ClientPoint,
    cell_width: i32,
    cell_height: i32,
    clamp_to_bounds: bool,
) -> Option<TerminalCellPoint> {
    let point = point.to_win32_point().ok()?;
    let rect = text_rect.to_win32_rect();
    if !clamp_to_bounds
        && (point.x < rect.left
            || point.x >= rect.right
            || point.y < rect.top
            || point.y >= rect.bottom)
    {
        return None;
    }

    let clamped_x = point.x.clamp(rect.left, rect.right.saturating_sub(1));
    let clamped_y = point.y.clamp(rect.top, rect.bottom.saturating_sub(1));
    let column = (clamped_x - rect.left) / cell_width.max(1);
    let row = (clamped_y - rect.top) / cell_height.max(1);
    Some(TerminalCellPoint::new(column, row))
}

fn layout_text_grid_rows(
    text: &str,
    width_px: i32,
    height_px: i32,
    cell_width: i32,
    cell_height: i32,
) -> TextGridRows {
    let cols = usize::try_from((width_px / cell_width.max(1)).max(0)).unwrap_or_default();
    let max_rows = if height_px == i32::MAX {
        usize::MAX
    } else {
        usize::try_from((height_px / cell_height.max(1)).max(0)).unwrap_or_default()
    };
    if cols == 0 || max_rows == 0 {
        return TextGridRows {
            rows: Vec::new(),
            cols,
        };
    }

    let mut rows = vec![Vec::new()];
    for character in text.chars() {
        if character == '\n' {
            if !push_text_grid_newline(&mut rows, max_rows) {
                break;
            }
            continue;
        }

        if character == '\t' {
            if !push_text_grid_tab(&mut rows, cols, max_rows) {
                break;
            }
            continue;
        }

        if !push_text_grid_character(&mut rows, cols, max_rows, character) {
            break;
        }
    }

    if rows.len() > max_rows {
        rows.truncate(max_rows);
    }

    TextGridRows { rows, cols }
}

fn push_text_grid_newline(rows: &mut Vec<Vec<char>>, max_rows: usize) -> bool {
    if rows.len() >= max_rows {
        return false;
    }

    rows.push(Vec::new());
    true
}

fn push_text_grid_tab(rows: &mut Vec<Vec<char>>, cols: usize, max_rows: usize) -> bool {
    let current_col = rows.last().map_or(0, Vec::len);
    let next_tab_stop = ((current_col / TAB_STOP_COLUMNS) + 1) * TAB_STOP_COLUMNS;
    let spaces_to_insert = next_tab_stop.saturating_sub(current_col).max(1);

    for _ in 0..spaces_to_insert {
        if !push_text_grid_character(rows, cols, max_rows, ' ') {
            return false;
        }
    }

    true
}

fn push_text_grid_character(
    rows: &mut Vec<Vec<char>>,
    cols: usize,
    max_rows: usize,
    character: char,
) -> bool {
    if rows.last().is_some_and(|row| row.len() >= cols) && !push_text_grid_newline(rows, max_rows) {
        return false;
    }

    if let Some(current_row) = rows.last_mut() {
        current_row.push(character);
        return true;
    }

    false
}

fn selection_row_width(selection: Option<TerminalSelection>, row: i32, cols: usize) -> usize {
    let Some(selection) = selection else {
        return 0;
    };

    (0..cols)
        .rev()
        .find(|column| {
            selection.contains(TerminalCellPoint::new(
                i32::try_from(*column).unwrap_or(i32::MAX),
                row,
            ))
        })
        .map_or(0, |column| column + 1)
}

fn cell_rect(
    text_rect: ClientRect,
    cell: TerminalCellPoint,
    cell_width: i32,
    cell_height: i32,
) -> RECT {
    RECT {
        left: text_rect.left() + (cell.column() * cell_width),
        top: text_rect.top() + (cell.row() * cell_height),
        right: text_rect.left() + ((cell.column() + 1) * cell_width),
        bottom: text_rect.top() + ((cell.row() + 1) * cell_height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::windows_terminal::TerminalSelectionMode;

    #[test]
    fn builds_wrapped_rows_using_available_columns() {
        let rows = layout_text_grid_rows("abcdef", 32, 48, 8, 16);

        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.rows[0].iter().collect::<String>(), "abcd");
        assert_eq!(rows.rows[1].iter().collect::<String>(), "ef");
    }

    #[test]
    fn expands_tabs_to_eight_column_stops() {
        let rows = layout_text_grid_rows("a\tb", 96, 32, 8, 16);

        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0].iter().collect::<String>(), "a       b");
    }

    #[test]
    fn expands_tabs_in_scene_diagnostic_reproduction() {
        let rows = layout_text_grid_rows(
            "window\tTeamy Studio\nbell-source\tWindows\n\nactions\n- Terminal\tavailable\n- Storage\tavailable\n- Audio\tavailable",
            480,
            160,
            8,
            16,
        );

        assert_eq!(
            rows.rows[0].iter().collect::<String>(),
            "window  Teamy Studio"
        );
        assert_eq!(
            rows.rows[1].iter().collect::<String>(),
            "bell-source     Windows"
        );
        assert_eq!(
            rows.rows[4].iter().collect::<String>(),
            "- Terminal      available"
        );
        assert_eq!(
            rows.rows[5].iter().collect::<String>(),
            "- Storage       available"
        );
        assert_eq!(rows.rows[6].iter().collect::<String>(), "- Audio available");
        assert!(
            rows.rows
                .iter()
                .flatten()
                .all(|character| *character != '\t')
        );
    }

    #[test]
    fn extracts_linear_selection_across_wrapped_rows() {
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(2, 0),
            TerminalCellPoint::new(1, 1),
            TerminalSelectionMode::Linear,
        );

        let extracted =
            extract_selected_text(ClientRect::new(0, 0, 32, 48), "abcdef", 8, 16, selection);

        assert_eq!(extracted, "cd\nef");
    }

    #[test]
    fn extracts_tabbed_selection_as_rendered_spaces() {
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(0, 0),
            TerminalCellPoint::new(8, 0),
            TerminalSelectionMode::Linear,
        );

        let extracted =
            extract_selected_text(ClientRect::new(0, 0, 96, 16), "a\tb", 8, 16, selection);

        assert_eq!(extracted, "a       b");
    }

    #[test]
    fn extracts_block_selection_with_spaces() {
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(1, 0),
            TerminalCellPoint::new(2, 1),
            TerminalSelectionMode::Block,
        );

        let extracted =
            extract_selected_text(ClientRect::new(0, 0, 32, 48), "ab\ncd", 8, 16, selection);

        assert_eq!(extracted, "b \nd ");
    }

    #[test]
    fn converts_client_points_into_grid_cells() {
        let cell = cell_from_client_point(
            ClientRect::new(10, 20, 42, 68),
            ClientPoint::new(26, 37),
            8,
            16,
            false,
        )
        .expect("point should resolve to a cell");

        assert_eq!(cell, TerminalCellPoint::new(2, 1));
    }
}
