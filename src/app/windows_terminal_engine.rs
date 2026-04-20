#[cfg(feature = "ghostty")]
use eyre::Context;

use super::vt_types::{ScrollViewport, key};

#[cfg(feature = "ghostty")]
use libghostty_vt::key as ghostty_key;
#[cfg(feature = "ghostty")]
use libghostty_vt::render::RenderState;
#[cfg(feature = "ghostty")]
use libghostty_vt::render::Snapshot;
#[cfg(feature = "ghostty")]
use libghostty_vt::screen::GridRef;
#[cfg(feature = "ghostty")]
use libghostty_vt::terminal::{Point, PointCoordinate};
#[cfg(feature = "ghostty")]
use libghostty_vt::{Terminal, TerminalOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhosttyViewportMetrics {
    pub total: u64,
    pub offset: u64,
    pub visible: u64,
    pub scrollback: usize,
}

#[cfg(feature = "ghostty")]
pub struct GhosttyTerminalEngine {
    terminal: Terminal<'static, 'static>,
    render_state: RenderState<'static>,
    key_encoder: ghostty_key::Encoder<'static>,
    key_event: ghostty_key::Event<'static>,
}

#[cfg(feature = "ghostty")]
impl GhosttyTerminalEngine {
    pub fn new(options: TerminalOptions) -> eyre::Result<Self> {
        Ok(Self {
            terminal: Terminal::new(options).wrap_err("failed to create libghostty terminal")?,
            render_state: RenderState::new().wrap_err("failed to create render state")?,
            key_encoder: ghostty_key::Encoder::new().wrap_err("failed to create key encoder")?,
            key_event: ghostty_key::Event::new().wrap_err("failed to create key event")?,
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
        self.terminal.scroll_viewport(viewport.into());
    }

    pub fn kitty_keyboard_flags(&self) -> eyre::Result<key::KittyKeyFlags> {
        self.terminal
            .kitty_keyboard_flags()
            .map(Into::into)
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
            .set_action(action.into())
            .set_key(mapped_key.into())
            .set_mods(mods.into())
            .set_consumed_mods(consumed_mods.into())
            .set_unshifted_codepoint(unshifted_codepoint)
            .set_utf8::<String>(None);

        self.key_encoder
            .set_options_from_terminal(&self.terminal)
            .encode_to_vec(&self.key_event, response)
            .wrap_err("failed to encode special key event")
    }
}

#[cfg(not(feature = "ghostty"))]
#[derive(Debug, Default)]
pub struct GhosttyTerminalEngine;

#[cfg(not(feature = "ghostty"))]
impl GhosttyTerminalEngine {
    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        cell_width: u32,
        cell_height: u32,
    ) -> eyre::Result<()> {
        let _ = (self, cols, rows, cell_width, cell_height);
        eyre::bail!("Ghostty VT engine requires the `ghostty` cargo feature")
    }

    pub fn vt_write(&mut self, bytes: &[u8]) {
        let _ = (self, bytes);
    }

    pub fn scroll_viewport(&mut self, viewport: ScrollViewport) {
        let _ = (self, viewport);
    }

    pub fn kitty_keyboard_flags(&self) -> eyre::Result<key::KittyKeyFlags> {
        let _ = self;
        eyre::bail!("Ghostty VT engine requires the `ghostty` cargo feature")
    }

    pub fn viewport_metrics(&self) -> eyre::Result<GhosttyViewportMetrics> {
        let _ = self;
        eyre::bail!("Ghostty VT engine requires the `ghostty` cargo feature")
    }

    pub fn total_rows(&self) -> eyre::Result<usize> {
        let _ = self;
        eyre::bail!("Ghostty VT engine requires the `ghostty` cargo feature")
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
        let _ = (
            self,
            action,
            mapped_key,
            mods,
            consumed_mods,
            unshifted_codepoint,
            response,
        );
        eyre::bail!("Ghostty VT engine requires the `ghostty` cargo feature")
    }
}
