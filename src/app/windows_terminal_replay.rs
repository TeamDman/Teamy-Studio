use std::fs;
use std::path::Path;
use std::time::Instant;

use eyre::Context;
use facet::Facet;
#[cfg(feature = "ghostty")]
use libghostty_vt::TerminalOptions;
#[cfg(feature = "ghostty")]
use libghostty_vt::render::{CellIterator, RowIterator};

use crate::app::teamy_terminal_engine::{
    TeamyCursorStyle, TeamyDisplayCursor, TeamyDisplayState, TeamyTerminalEngine,
    TeamyTraceSnapshot,
};

#[cfg(feature = "ghostty")]
use super::windows_terminal_engine::GhosttyTerminalEngine;

#[derive(Debug, Facet)]
struct TerminalReplayFixture {
    cols: u16,
    rows: u16,
    max_scrollback: usize,
    chunks: Vec<TerminalReplayChunk>,
}

#[derive(Debug, Facet)]
struct TerminalReplayChunk {
    text: String,
    repeat: Option<usize>,
}

#[derive(Debug, Facet)]
pub struct TerminalReplayReport {
    fixture_path: String,
    artifact_output_path: Option<String>,
    samples: usize,
    median_apply_ms: f64,
    median_vt_write_calls: u64,
    median_bytes_applied: u64,
    final_screen: String,
    teamy_median_apply_ms: f64,
    teamy_median_vt_write_calls: u64,
    teamy_median_bytes_applied: u64,
    teamy_final_screen: String,
    teamy_matches_ghostty: bool,
    teamy_display_rows: usize,
    teamy_display_glyphs: usize,
    teamy_display: TeamyDisplayState,
    teamy_trace: TeamyTraceSnapshot,
}

#[derive(Clone, Debug)]
struct TerminalReplaySample {
    apply_ms: f64,
    vt_write_calls: u64,
    bytes_applied: u64,
    final_screen: String,
    display_rows: usize,
    display_glyphs: usize,
    display: TeamyDisplayState,
    trace: TeamyTraceSnapshot,
}

pub fn run_terminal_replay_self_test(
    fixture_path: &Path,
    artifact_output: Option<&Path>,
    samples: usize,
) -> eyre::Result<TerminalReplayReport> {
    #[cfg(not(feature = "ghostty"))]
    {
        let _ = fixture_path;
        let _ = artifact_output;
        let _ = samples;
        eyre::bail!("terminal replay self-test requires the `ghostty` feature")
    }

    #[cfg(feature = "ghostty")]
    {
        let fixture_text = fs::read_to_string(fixture_path).wrap_err_with(|| {
            format!("failed to read replay fixture {}", fixture_path.display())
        })?;
        let fixture: TerminalReplayFixture =
            facet_json::from_str(&fixture_text).wrap_err_with(|| {
                format!("failed to parse replay fixture {}", fixture_path.display())
            })?;

        let sample_count = samples.max(1);
        let mut ghostty_sample_results = Vec::with_capacity(sample_count);
        let mut teamy_sample_results = Vec::with_capacity(sample_count);
        for _ in 0..sample_count {
            ghostty_sample_results.push(run_ghostty_terminal_replay_sample(&fixture)?);
            teamy_sample_results.push(run_teamy_terminal_replay_sample(&fixture));
        }

        let final_sample = ghostty_sample_results
            .last()
            .ok_or_else(|| eyre::eyre!("terminal replay did not produce any samples"))?;
        let final_teamy_sample = teamy_sample_results
            .last()
            .ok_or_else(|| eyre::eyre!("teamy terminal replay did not produce any samples"))?;
        let report = TerminalReplayReport {
            fixture_path: fixture_path.display().to_string(),
            artifact_output_path: artifact_output.map(|path| path.display().to_string()),
            samples: sample_count,
            median_apply_ms: median_f64(&ghostty_sample_results, |sample| sample.apply_ms),
            median_vt_write_calls: median_u64(&ghostty_sample_results, |sample| {
                sample.vt_write_calls
            }),
            median_bytes_applied: median_u64(&ghostty_sample_results, |sample| {
                sample.bytes_applied
            }),
            final_screen: final_sample.final_screen.clone(),
            teamy_median_apply_ms: median_f64(&teamy_sample_results, |sample| sample.apply_ms),
            teamy_median_vt_write_calls: median_u64(&teamy_sample_results, |sample| {
                sample.vt_write_calls
            }),
            teamy_median_bytes_applied: median_u64(&teamy_sample_results, |sample| {
                sample.bytes_applied
            }),
            teamy_final_screen: final_teamy_sample.final_screen.clone(),
            teamy_matches_ghostty: final_teamy_sample.final_screen == final_sample.final_screen,
            teamy_display_rows: final_teamy_sample.display_rows,
            teamy_display_glyphs: final_teamy_sample.display_glyphs,
            teamy_display: final_teamy_sample.display.clone(),
            teamy_trace: final_teamy_sample.trace.clone(),
        };

        if let Some(artifact_output) = artifact_output {
            if let Some(parent) = artifact_output.parent() {
                fs::create_dir_all(parent).wrap_err_with(|| {
                    format!(
                        "failed to create replay artifact directory {}",
                        parent.display()
                    )
                })?;
            }
            let json = facet_json::to_string_pretty(&report)
                .wrap_err("failed to serialize terminal replay report")?;
            fs::write(artifact_output, json).wrap_err_with(|| {
                format!(
                    "failed to write terminal replay artifact {}",
                    artifact_output.display()
                )
            })?;
        }

        Ok(report)
    }
}

#[cfg(feature = "ghostty")]
fn run_ghostty_terminal_replay_sample(
    fixture: &TerminalReplayFixture,
) -> eyre::Result<TerminalReplaySample> {
    let mut engine = GhosttyTerminalEngine::new(TerminalOptions {
        cols: fixture.cols,
        rows: fixture.rows,
        max_scrollback: fixture.max_scrollback,
    })?;

    let started_at = Instant::now();
    let mut vt_write_calls = 0_u64;
    let mut bytes_applied = 0_u64;
    for chunk in &fixture.chunks {
        let repeat = chunk.repeat.unwrap_or(1).max(1);
        for _ in 0..repeat {
            engine.vt_write(chunk.text.as_bytes());
            vt_write_calls = vt_write_calls.saturating_add(1);
            bytes_applied =
                bytes_applied.saturating_add(u64::try_from(chunk.text.len()).unwrap_or(u64::MAX));
        }
    }
    let apply_ms = started_at.elapsed().as_secs_f64() * 1000.0;
    let final_screen = visible_text(&mut engine)?;

    Ok(TerminalReplaySample {
        apply_ms,
        vt_write_calls,
        bytes_applied,
        final_screen,
        display_rows: 0,
        display_glyphs: 0,
        display: TeamyDisplayState {
            cols: usize::from(fixture.cols.max(1)),
            rows: usize::from(fixture.rows.max(1)),
            visible_rows: Vec::new(),
            cursor: TeamyDisplayCursor {
                row: 0,
                column: 0,
                style: TeamyCursorStyle::Block,
            },
            cursor_visible: true,
            total_rows: 0,
        },
        trace: TeamyTraceSnapshot { events: Vec::new() },
    })
}

fn run_teamy_terminal_replay_sample(fixture: &TerminalReplayFixture) -> TerminalReplaySample {
    let mut engine = TeamyTerminalEngine::new(fixture.cols, fixture.rows, fixture.max_scrollback);

    let started_at = Instant::now();
    let mut vt_write_calls = 0_u64;
    let mut bytes_applied = 0_u64;
    for chunk in &fixture.chunks {
        let repeat = chunk.repeat.unwrap_or(1).max(1);
        for _ in 0..repeat {
            engine.vt_write(chunk.text.as_bytes());
            vt_write_calls = vt_write_calls.saturating_add(1);
            bytes_applied =
                bytes_applied.saturating_add(u64::try_from(chunk.text.len()).unwrap_or(u64::MAX));
        }
    }

    let display = engine.display_state();
    let trace = engine.trace_snapshot();

    TerminalReplaySample {
        apply_ms: started_at.elapsed().as_secs_f64() * 1000.0,
        vt_write_calls,
        bytes_applied,
        final_screen: teamy_visible_text_from_display(&display),
        display_rows: display.visible_rows.len(),
        display_glyphs: display
            .visible_rows
            .iter()
            .map(|row| row.glyphs.len())
            .sum(),
        display,
        trace,
    }
}

fn teamy_visible_text_from_display(display: &TeamyDisplayState) -> String {
    let mut lines = display
        .visible_rows
        .iter()
        .map(|row| {
            let mut cells = vec![' '; display.cols];
            for glyph in &row.glyphs {
                if let Some(cell) = cells.get_mut(glyph.column) {
                    *cell = glyph.character;
                }
            }
            cells
                .iter()
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

#[cfg(feature = "ghostty")]
fn visible_text(engine: &mut GhosttyTerminalEngine) -> eyre::Result<String> {
    engine.with_snapshot(|snapshot| {
        let mut rows = RowIterator::new().wrap_err("failed to create row iterator")?;
        let mut cells = CellIterator::new().wrap_err("failed to create cell iterator")?;
        let mut lines = Vec::new();

        let mut row_iter = rows
            .update(snapshot)
            .wrap_err("failed to update row iterator")?;
        while let Some(row) = row_iter.next() {
            let mut line = String::new();
            let mut cell_iter = cells
                .update(row)
                .wrap_err("failed to update cell iterator")?;
            while let Some(cell) = cell_iter.next() {
                let graphemes = cell
                    .graphemes()
                    .wrap_err("failed to read replay cell text")?;
                if graphemes.is_empty() {
                    line.push(' ');
                } else {
                    for character in graphemes {
                        line.push(character);
                    }
                }
            }
            lines.push(line.trim_end_matches(' ').to_owned());
        }

        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }

        Ok(lines.join("\n"))
    })
}

fn median_f64<T>(samples: &[T], selector: impl Fn(&T) -> f64) -> f64 {
    let mut values = samples.iter().map(selector).collect::<Vec<_>>();
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

fn median_u64<T>(samples: &[T], selector: impl Fn(&T) -> u64) -> u64 {
    let mut values = samples.iter().map(selector).collect::<Vec<_>>();
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        u64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

#[cfg(all(test, feature = "ghostty"))]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::app::teamy_terminal_engine::TeamyTerminalEngine;
    use eyre::WrapErr;

    use super::{
        GhosttyTerminalEngine, TerminalReplayChunk, TerminalReplayFixture,
        run_ghostty_terminal_replay_sample, run_teamy_terminal_replay_sample,
        teamy_visible_text_from_display, visible_text,
    };
    use libghostty_vt::TerminalOptions;

    fn fixture_from_file(name: &str) -> eyre::Result<TerminalReplayFixture> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("terminal-replay")
            .join(name);
        let text = fs::read_to_string(&path)
            .map_err(eyre::Report::from)
            .wrap_err_with(|| format!("failed to read replay fixture {}", path.display()))?;
        facet_json::from_str(&text)
            .map_err(eyre::Report::from)
            .wrap_err_with(|| format!("failed to parse replay fixture {}", path.display()))
    }

    fn ghostty_visible_text(fixture: &TerminalReplayFixture) -> eyre::Result<String> {
        let mut engine = GhosttyTerminalEngine::new(TerminalOptions {
            cols: fixture.cols,
            rows: fixture.rows,
            max_scrollback: fixture.max_scrollback,
        })?;
        for chunk in &fixture.chunks {
            let repeat = chunk.repeat.unwrap_or(1).max(1);
            for _ in 0..repeat {
                engine.vt_write(chunk.text.as_bytes());
            }
        }
        visible_text(&mut engine)
    }

    #[test]
    fn replay_sample_produces_visible_text() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 20,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "hello\r\nworld\r\n".to_owned(),
                repeat: None,
            }],
        };

        let sample = run_ghostty_terminal_replay_sample(&fixture)?;

        assert_eq!(sample.final_screen, "hello\nworld");
        assert_eq!(sample.vt_write_calls, 1);
        assert!(sample.bytes_applied > 0);
        Ok(())
    }

    #[test]
    fn replay_sample_honors_repeat_counts() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 20,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "x".to_owned(),
                repeat: Some(3),
            }],
        };

        let sample = run_ghostty_terminal_replay_sample(&fixture)?;

        assert_eq!(sample.vt_write_calls, 3);
        assert_eq!(sample.bytes_applied, 3);
        assert_eq!(sample.final_screen, "xxx");
        Ok(())
    }

    #[test]
    fn teamy_display_export_round_trips_to_visible_text() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 16,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "value: old\u{1b}[8G\u{1b}[Knew\n".to_owned(),
                repeat: None,
            }],
        };

        let mut engine =
            TeamyTerminalEngine::new(fixture.cols, fixture.rows, fixture.max_scrollback);
        for chunk in &fixture.chunks {
            engine.vt_write(chunk.text.as_bytes());
        }

        assert_eq!(
            teamy_visible_text_from_display(&engine.display_state()),
            engine.visible_text()
        );
        Ok(())
    }

    #[test]
    fn teamy_replay_sample_includes_operation_trace() {
        let fixture = TerminalReplayFixture {
            cols: 16,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "value: old\u{1b}[8G\u{1b}[Knew\n".to_owned(),
                repeat: None,
            }],
        };

        let sample = run_teamy_terminal_replay_sample(&fixture);

        assert!(!sample.trace.events.is_empty());
        assert!(
            sample
                .trace
                .events
                .iter()
                .any(|event| event.action == "cursor-horizontal-absolute")
        );
        assert!(
            sample
                .trace
                .events
                .iter()
                .any(|event| event.action == "erase-in-line")
        );
    }

    #[test]
    fn teamy_engine_matches_ghostty_for_simple_crlf_fixture() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 20,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "hello\r\nworld\r\n".to_owned(),
                repeat: None,
            }],
        };

        assert_eq!(
            run_teamy_terminal_replay_sample(&fixture).final_screen,
            ghostty_visible_text(&fixture)?
        );
        Ok(())
    }

    #[test]
    fn teamy_engine_matches_ghostty_for_simple_wrap_fixture() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 5,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "abcdef".to_owned(),
                repeat: None,
            }],
        };

        assert_eq!(
            run_teamy_terminal_replay_sample(&fixture).final_screen,
            ghostty_visible_text(&fixture)?
        );
        Ok(())
    }

    #[test]
    fn teamy_engine_matches_ghostty_for_carriage_return_fixture() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 5,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "abcde\rZ".to_owned(),
                repeat: None,
            }],
        };

        assert_eq!(
            run_teamy_terminal_replay_sample(&fixture).final_screen,
            ghostty_visible_text(&fixture)?
        );
        Ok(())
    }

    #[test]
    fn teamy_engine_matches_ghostty_for_tab_fixture() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 12,
            rows: 4,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "a\tb".to_owned(),
                repeat: None,
            }],
        };

        assert_eq!(
            run_teamy_terminal_replay_sample(&fixture).final_screen,
            ghostty_visible_text(&fixture)?
        );
        Ok(())
    }

    #[test]
    fn teamy_engine_matches_ghostty_for_repeated_multiline_fixture() -> eyre::Result<()> {
        let fixture = TerminalReplayFixture {
            cols: 8,
            rows: 3,
            max_scrollback: 64,
            chunks: vec![TerminalReplayChunk {
                text: "one\r\ntwo\r\n".to_owned(),
                repeat: Some(2),
            }],
        };

        assert_eq!(
            run_teamy_terminal_replay_sample(&fixture).final_screen,
            ghostty_visible_text(&fixture)?
        );
        Ok(())
    }

    #[test]
    fn teamy_engine_matches_ghostty_for_supported_file_fixtures() -> eyre::Result<()> {
        for fixture_name in [
            "ansi-delete-character.json",
            "ansi-cursor-right.json",
            "hello.json",
            "ansi-cursor-horizontal-absolute.json",
            "ansi-cursor-left.json",
            "ansi-erase-line.json",
            "ansi-redraw-sequence.json",
            "carriage-return.json",
            "tabbed-columns.json",
            "repeated-multiline.json",
            "pwsh-noprofile-measure-command-8.json",
            "pwsh-noprofile-prompt-bursts-4.json",
            "pwsh-noprofile-scroll-flood-6.json",
            "pwsh-noprofile-wide-lines-2.json",
        ] {
            let fixture = fixture_from_file(fixture_name)?;
            let teamy = run_teamy_terminal_replay_sample(&fixture).final_screen;
            let ghostty = ghostty_visible_text(&fixture)?;
            assert_eq!(teamy, ghostty, "fixture mismatch for {fixture_name}");
        }

        Ok(())
    }
}
