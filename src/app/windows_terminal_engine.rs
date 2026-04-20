use eyre::Context;
use libghostty_vt::key;
use libghostty_vt::render::RenderState;
use libghostty_vt::render::Snapshot;
use libghostty_vt::screen::GridRef;
use libghostty_vt::terminal::{Point, PointCoordinate, ScrollViewport};
use libghostty_vt::{Terminal, TerminalOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhosttyViewportMetrics {
    pub total: u64,
    pub offset: u64,
    pub visible: u64,
    pub scrollback: usize,
}

pub struct GhosttyTerminalEngine {
    terminal: Terminal<'static, 'static>,
    render_state: RenderState<'static>,
    key_encoder: key::Encoder<'static>,
    key_event: key::Event<'static>,
}

impl GhosttyTerminalEngine {
    pub fn new(options: TerminalOptions) -> eyre::Result<Self> {
        Ok(Self {
            terminal: Terminal::new(options).wrap_err("failed to create libghostty terminal")?,
            render_state: RenderState::new().wrap_err("failed to create render state")?,
            key_encoder: key::Encoder::new().wrap_err("failed to create key encoder")?,
            key_event: key::Event::new().wrap_err("failed to create key event")?,
        })
    }

    pub fn on_pty_write<F>(&mut self, effect: F) -> eyre::Result<()>
    where
        F: FnMut(&Terminal<'static, 'static>, &[u8]) + Send + 'static,
    {
        self.terminal
            .on_pty_write(effect)
            .map(|_| ())
            .wrap_err("failed to register PTY write effect")
    }

    pub fn on_bell<F>(&mut self, effect: F) -> eyre::Result<()>
    where
        F: FnMut() + Send + 'static,
    {
        let mut effect = effect;
        self.terminal
            .on_bell(move |_terminal| {
                effect();
            })
            .map(|_| ())
            .wrap_err("failed to register terminal bell effect")
    }

    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        cell_width: u32,
        cell_height: u32,
    ) -> eyre::Result<()> {
        self.terminal
            .resize(cols, rows, cell_width, cell_height)
            .wrap_err("failed to resize libghostty terminal")
    }

    pub fn vt_write(&mut self, bytes: &[u8]) {
        self.terminal.vt_write(bytes);
    }

    pub fn scroll_viewport(&mut self, viewport: ScrollViewport) {
        self.terminal.scroll_viewport(viewport);
    }

    pub fn kitty_keyboard_flags(&self) -> eyre::Result<key::KittyKeyFlags> {
        self.terminal
            .kitty_keyboard_flags()
            .wrap_err("failed to query kitty keyboard flags")
    }

    pub fn viewport_metrics(&self) -> eyre::Result<GhosttyViewportMetrics> {
        let scrollbar = self
            .terminal
            .scrollbar()
            .wrap_err("failed to query terminal scrollbar state")?;
        let scrollback = self
            .terminal
            .scrollback_rows()
            .wrap_err("failed to query terminal scrollback row count")?;

        Ok(GhosttyViewportMetrics {
            total: scrollbar.total,
            offset: scrollbar.offset,
            visible: scrollbar.len,
            scrollback,
        })
    }

    pub fn total_rows(&self) -> eyre::Result<usize> {
        self.terminal
            .total_rows()
            .wrap_err("failed to query terminal total row count")
    }

    pub fn screen_grid_ref(&self, column: u16, row: u32) -> eyre::Result<GridRef<'_>> {
        self.terminal
            .grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))
            .wrap_err_with(|| {
                format!("failed to resolve terminal screen point at column {column}, row {row}")
            })
    }

    pub fn with_snapshot<T>(
        &mut self,
        callback: impl FnOnce(&Snapshot<'_, '_>) -> eyre::Result<T>,
    ) -> eyre::Result<T> {
        let snapshot = self
            .render_state
            .update(&self.terminal)
            .wrap_err("failed to update terminal render state")?;
        callback(&snapshot)
    }

    pub fn encode_key_event(
        &mut self,
        action: key::Action,
        mapped_key: key::Key,
        mods: key::Mods,
        consumed_mods: key::Mods,
        unshifted_codepoint: char,
        response: &mut Vec<u8>,
    ) -> eyre::Result<()> {
        self.key_event
            .set_action(action)
            .set_key(mapped_key)
            .set_mods(mods)
            .set_consumed_mods(consumed_mods)
            .set_unshifted_codepoint(unshifted_codepoint)
            .set_utf8::<String>(None);

        self.key_encoder
            .set_options_from_terminal(&self.terminal)
            .encode_to_vec(&self.key_event, response)
            .wrap_err("failed to encode special key event")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use libghostty_vt::TerminalOptions;

    use super::GhosttyTerminalEngine;

    // behavior[verify window.interaction.output.bell.audible]
    #[test]
    fn standalone_bel_triggers_the_ghostty_bell_callback() -> eyre::Result<()> {
        let mut engine = GhosttyTerminalEngine::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 64,
        })?;
        let bells = Arc::new(AtomicUsize::new(0));
        let bells_for_effect = Arc::clone(&bells);
        engine.on_bell(move || {
            bells_for_effect.fetch_add(1, Ordering::Relaxed);
        })?;

        engine.vt_write(b"\x07");

        assert_eq!(bells.load(Ordering::Relaxed), 1);
        Ok(())
    }

    #[test]
    fn osc_bel_terminator_does_not_trigger_the_ghostty_bell_callback() -> eyre::Result<()> {
        let mut engine = GhosttyTerminalEngine::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 64,
        })?;
        let bells = Arc::new(AtomicUsize::new(0));
        let bells_for_effect = Arc::clone(&bells);
        engine.on_bell(move || {
            bells_for_effect.fetch_add(1, Ordering::Relaxed);
        })?;

        engine.vt_write(b"\x1b]0;pwsh.exe\x07");

        assert_eq!(bells.load(Ordering::Relaxed), 0);
        Ok(())
    }
}
