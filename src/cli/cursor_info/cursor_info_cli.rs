use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

use crate::cli::output::CliOutput;

#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum CursorInfoCliRenderMode {
    #[default]
    Mask,
    Desktop,
    Overlay,
}

impl From<CursorInfoCliRenderMode> for crate::app::CursorInfoRenderMode {
    fn from(value: CursorInfoCliRenderMode) -> Self {
        match value {
            CursorInfoCliRenderMode::Mask => Self::Mask,
            CursorInfoCliRenderMode::Desktop => Self::Desktop,
            CursorInfoCliRenderMode::Overlay => Self::Overlay,
        }
    }
}

#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum CursorInfoCliPixelSize {
    Full,
    #[default]
    HalfHeight,
}

impl From<CursorInfoCliPixelSize> for crate::app::CursorInfoPixelSize {
    fn from(value: CursorInfoCliPixelSize) -> Self {
        match value {
            CursorInfoCliPixelSize::Full => Self::Full,
            CursorInfoCliPixelSize::HalfHeight => Self::HalfHeight,
        }
    }
}

/// Launch the standalone cursor diagnostics TUI.
// cli[impl command.surface.cursor-info]
///
/// The cursor-info app renders a live desktop inspection viewport on `stderr`.
///
/// Current interactive behavior targets:
/// - live-updating cursor-centered inspection
/// - `x` cycles among `mask`, `desktop`, and `overlay` render modes
/// - screenshot-backed desktop view plus semantic overlay diagnostics
/// - keyboard and mouse interaction for viewport inspection
/// - `f` toggles follow-cursor recentering
/// - right-drag pans the viewport and the mouse wheel adjusts zoom
#[derive(Facet, Arbitrary, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
pub struct CursorInfoArgs {
    /// Initial render mode before `x` cycles to the next mode.
    #[facet(args::named)]
    pub render_mode: Option<CursorInfoCliRenderMode>,

    /// Initial desktop pixels per logical sample. Lower values zoom in and higher values zoom out.
    #[facet(args::named)]
    pub scale: Option<u16>,

    /// Logical pixel density used when mapping logical samples into terminal cells.
    #[facet(args::named)]
    pub pixel_size: Option<CursorInfoCliPixelSize>,
}

impl CursorInfoArgs {
    /// # Errors
    ///
    /// This function will return an error if the cursor-info TUI fails to launch or exits with an error.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = cache_home;
        crate::app::run_cursor_info(
            app_home,
            crate::app::CursorInfoConfig {
                initial_mode: self.render_mode.unwrap_or_default().into(),
                scale: i32::from(self.scale.unwrap_or(4).max(1)),
                pixel_size: self.pixel_size.unwrap_or_default().into(),
            },
        )?;
        Ok(CliOutput::none())
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::cursor_info::CursorInfoArgs;
    use crate::cli::{Cli, Command};

    #[test]
    fn cursor_info_parser_accepts_top_level_command() {
        let cli: Cli = figue::Driver::new(
            figue::builder::<Cli>()
                .expect("schema should be valid")
                .cli(move |cli| cli.args(["cursor-info"]).strict())
                .build(),
        )
        .run()
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                global_args: Default::default(),
                builtins: Default::default(),
                command: Some(Command::CursorInfo(CursorInfoArgs {
                    render_mode: None,
                    scale: None,
                    pixel_size: None,
                })),
            }
        );
    }
}
