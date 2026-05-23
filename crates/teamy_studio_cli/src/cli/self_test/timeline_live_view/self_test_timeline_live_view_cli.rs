use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};

use crate::cli::output::CliOutput;

/// Viewport strategies for the live timeline FPS self-test.
#[derive(Facet, Arbitrary, Clone, Copy, Debug, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum SelfTestTimelineLiveViewViewportMode {
    FollowTail,
    FitContent,
}

impl From<SelfTestTimelineLiveViewViewportMode> for crate::app::TimelineLiveViewSelfTestViewportMode {
    fn from(value: SelfTestTimelineLiveViewViewportMode) -> Self {
        match value {
            SelfTestTimelineLiveViewViewportMode::FollowTail => Self::FollowTail,
            SelfTestTimelineLiveViewViewportMode::FitContent => Self::FitContent,
        }
    }
}

/// Run the live timeline view FPS self-test.
// cli[impl command.surface.self-test-timeline-live-view]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestTimelineLiveViewArgs {
    // cli[impl self-test.timeline-live-view.sample-ms-flag]
    /// Duration of the measured live-view sample in milliseconds.
    #[facet(args::named)]
    pub sample_ms: Option<u64>,

    // cli[impl self-test.timeline-live-view.warmup-ms-flag]
    /// Warmup time before frame intervals are sampled, in milliseconds.
    #[facet(args::named)]
    pub warmup_ms: Option<u64>,

    // cli[impl self-test.timeline-live-view.samples-flag]
    /// Number of samples to run before reporting median results.
    #[facet(args::named)]
    pub samples: Option<usize>,

    // cli[impl self-test.timeline-live-view.bucket-ms-flag]
    /// Interval bucket size for per-window FPS and dataset-growth reporting, in milliseconds.
    #[facet(args::named)]
    pub bucket_ms: Option<u64>,

    // cli[impl self-test.timeline-live-view.viewport-mode-flag]
    /// Viewport strategy to exercise during the self-test.
    #[facet(args::named)]
    pub viewport_mode: Option<SelfTestTimelineLiveViewViewportMode>,

    // cli[impl self-test.timeline-live-view.fit-content-interval-ms-flag]
    /// How often the fit-content viewport should be refreshed, in milliseconds.
    #[facet(args::named)]
    pub fit_content_interval_ms: Option<u64>,

    // cli[impl self-test.timeline-live-view.minimum-visible-pixels-flag]
    /// Minimum visible pixels used for event/span folding while the self-test runs.
    #[facet(args::named)]
    pub minimum_visible_pixels: Option<u32>,

    // cli[impl self-test.timeline-live-view.overlay-message-flag]
    /// Optional overlay banner text shown while the unattended self-test window is running.
    #[facet(args::named)]
    pub overlay_message: Option<String>,

    // cli[impl self-test.timeline-live-view.fail-below-fps-flag]
    /// Fail the command if any interval bucket averages below this FPS threshold.
    #[facet(args::named)]
    pub fail_below_fps: Option<f64>,
}

impl SelfTestTimelineLiveViewArgs {
    /// # Errors
    ///
    /// This function will return an error if the live timeline self-test fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let sample_ms = self.sample_ms.unwrap_or(5_000).max(1);
        let bucket_ms = self.bucket_ms.unwrap_or(1_000).max(1).min(sample_ms);
        Ok(CliOutput::facet(crate::app::run_timeline_live_view_self_test(
            app_home,
            cache_home,
            crate::app::TimelineLiveViewSelfTestOptions {
                sample_duration_ms: sample_ms,
                warmup_duration_ms: self.warmup_ms.unwrap_or(1_000),
                bucket_duration_ms: bucket_ms,
                samples: self.samples.unwrap_or(1).max(1),
                viewport_mode: self
                    .viewport_mode
                    .unwrap_or(SelfTestTimelineLiveViewViewportMode::FitContent)
                    .into(),
                fit_content_interval_ms: self.fit_content_interval_ms.or_else(|| {
                    Some(bucket_ms).filter(|_| {
                        self.viewport_mode
                            .unwrap_or(SelfTestTimelineLiveViewViewportMode::FitContent)
                            == SelfTestTimelineLiveViewViewportMode::FitContent
                    })
                }),
                minimum_visible_pixels: self.minimum_visible_pixels.unwrap_or(4).max(1),
                overlay_message: self.overlay_message,
                fail_below_fps: self.fail_below_fps,
            },
        )?))
    }
}