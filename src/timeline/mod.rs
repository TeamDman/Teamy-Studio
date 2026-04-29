use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Read as _;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use eyre::Context;
use sguaba::Coordinate;
use uom::si::f64::{Length, Time};
use uom::si::length::meter;
use uom::si::time::{millisecond, nanosecond, second};

pub mod dataset;
pub mod query;
pub mod synthetic;
pub mod time;

pub use dataset::{
    TimelineCompactionReport, TimelineDataset, TimelineDatasetRevision, TimelineEventItem,
    TimelineField, TimelineFieldInputValue, TimelineFieldValue, TimelineInternedStringId,
    TimelineItem, TimelineItemId, TimelineItemInput, TimelineItemKind, TimelineItemSequence,
    TimelineObjectRef, TimelineSpanItem, TimelineWriteLogEntry,
};
pub use query::{
    TimelineGroupingMode, TimelineRenderCluster, TimelineRenderEvent, TimelineRenderItem,
    TimelineRenderPlan, TimelineRenderRow, TimelineRenderRowId, TimelineRenderRowKey,
    TimelineRenderSpan, TimelineViewportQuery,
};
pub use synthetic::{TimelineSyntheticConfig, generate_synthetic_timeline_dataset};
pub use time::{TimelineDurationNs, TimelineInstantNs, TimelineRangeNs};

use crate::model::DEFAULT_TRANSCRIPTION_MODEL_NAME;

sguaba::system!(pub struct TimelineViewportSpace using right-handed XYZ);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimelineTimeNs(i64);

impl TimelineTimeNs {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(nanoseconds: i64) -> Self {
        Self(nanoseconds)
    }

    #[must_use]
    pub const fn as_i64(self) -> i64 {
        self.0
    }

    #[must_use]
    // convention[impl convention.measurements.use-uom]
    #[expect(
        clippy::cast_precision_loss,
        reason = "timeline stores integer nanoseconds and converts to uom quantities only at typed projection boundaries"
    )]
    pub fn duration(self) -> Time {
        Time::new::<nanosecond>(self.0 as f64)
    }

    #[must_use]
    pub fn from_duration(duration: Time) -> Self {
        Self::new(f64_to_i64_saturating(duration.get::<nanosecond>().round()))
    }
}

// convention[impl convention.types.newtypes-for-domain-boundaries]
// convention[impl convention.measurements.use-uom]
// convention[impl convention.spatial.transforms.use-sguaba]
#[derive(Debug, PartialEq)]
pub struct TimelineViewportPoint {
    coordinate: Coordinate<TimelineViewportSpace>,
}

#[expect(
    clippy::expl_impl_clone_on_copy,
    reason = "this coordinate wrapper mirrors the existing spatial point pattern in the repo"
)]
impl Clone for TimelineViewportPoint {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for TimelineViewportPoint {}

impl TimelineViewportPoint {
    #[must_use]
    pub fn new_pixels(x_pixels: f64) -> Self {
        Self {
            coordinate: Coordinate::<TimelineViewportSpace>::builder()
                .x(Length::new::<meter>(x_pixels))
                .y(Length::new::<meter>(0.0))
                .z(Length::new::<meter>(0.0))
                .build(),
        }
    }

    #[must_use]
    pub fn x(self) -> Length {
        self.coordinate.x()
    }

    #[must_use]
    pub fn pixels(self) -> f64 {
        self.x().get::<meter>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineProjectedSpan {
    start: TimelineViewportPoint,
    end: TimelineViewportPoint,
}

impl TimelineProjectedSpan {
    #[must_use]
    pub const fn start(self) -> TimelineViewportPoint {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> TimelineViewportPoint {
        self.end
    }

    #[must_use]
    pub fn width(self) -> Length {
        self.end.x() - self.start.x()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineRulerTick {
    time: TimelineTimeNs,
    x: TimelineViewportPoint,
    label: String,
}

impl TimelineRulerTick {
    #[must_use]
    pub const fn time(&self) -> TimelineTimeNs {
        self.time
    }

    #[must_use]
    pub const fn x(&self) -> TimelineViewportPoint {
        self.x
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimelineDocumentId(NonZeroU64);

impl TimelineDocumentId {
    pub const DEFAULT_BLANK: Self = Self(NonZeroU64::MIN);

    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimelineTrackId(NonZeroU64);

impl TimelineTrackId {
    #[must_use]
    pub const fn new(id: NonZeroU64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineTimeRangeNs {
    start: TimelineTimeNs,
    end: TimelineTimeNs,
}

impl TimelineTimeRangeNs {
    #[must_use]
    pub fn new(start: TimelineTimeNs, end: TimelineTimeNs) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    #[must_use]
    pub const fn start(self) -> TimelineTimeNs {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> TimelineTimeNs {
        self.end
    }
}

const TIMELINE_VIEWPORT_MIN_DURATION_PER_PIXEL_NS: f64 = 10.0;
const TIMELINE_VIEWPORT_MAX_DURATION_PER_PIXEL_NS: f64 = 5_000_000_000.0;
const TIMELINE_VIEWPORT_PAN_STEP_PIXELS: i32 = 160;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineEdit {
    RippleDelete { range: TimelineTimeRangeNs },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineTrackKind {
    Audio,
    Transcription,
    Text,
    TracingSpans,
}

impl TimelineTrackKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Transcription => "Transcription",
            Self::Text => "Text",
            Self::TracingSpans => "Tracing spans",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineAudioTrackProjection {
    source_label: String,
    source_device_id: Option<String>,
    preview_range: TimelineTimeRangeNs,
}

impl TimelineAudioTrackProjection {
    #[must_use]
    // timeline[impl track.preview-ranges]
    pub fn new(source_label: impl Into<String>) -> Self {
        Self {
            source_label: source_label.into(),
            source_device_id: None,
            preview_range: TimelineTimeRangeNs::new(
                TimelineTimeNs::ZERO,
                TimelineTimeNs::from_duration(Time::new::<millisecond>(320.0)),
            ),
        }
    }

    #[must_use]
    pub fn new_microphone(
        source_label: impl Into<String>,
        source_device_id: impl Into<String>,
    ) -> Self {
        Self {
            source_label: source_label.into(),
            source_device_id: Some(source_device_id.into()),
            preview_range: TimelineTimeRangeNs::new(
                TimelineTimeNs::ZERO,
                TimelineTimeNs::from_duration(Time::new::<millisecond>(320.0)),
            ),
        }
    }

    #[must_use]
    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    #[must_use]
    pub fn source_device_id(&self) -> Option<&str> {
        self.source_device_id.as_deref()
    }

    #[must_use]
    pub const fn preview_range(&self) -> TimelineTimeRangeNs {
        self.preview_range
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
// timeline[impl transcription.targets]
// timeline[impl transcription.settings]
// timeline[impl transcription.defaults]
pub struct TimelineTranscriptionTrackProjection {
    source_label: String,
    preview_range: TimelineTimeRangeNs,
    model_name: String,
    target_audio_track_id: Option<TimelineTrackId>,
    target_text_track_id: Option<TimelineTrackId>,
    inactivity_detection_period: TimelineTimeNs,
    activity_threshold: u16,
    chunk_range: TimelineTimeRangeNs,
    progress_head: TimelineTimeNs,
    automatically_advance_chunk_boundaries: bool,
    automatically_submit_chunks: bool,
}

impl TimelineTranscriptionTrackProjection {
    #[must_use]
    pub fn new(source_label: impl Into<String>) -> Self {
        Self {
            source_label: source_label.into(),
            preview_range: TimelineTimeRangeNs::new(
                TimelineTimeNs::ZERO,
                TimelineTimeNs::from_duration(Time::new::<millisecond>(320.0)),
            ),
            model_name: DEFAULT_TRANSCRIPTION_MODEL_NAME.to_owned(),
            target_audio_track_id: None,
            target_text_track_id: None,
            inactivity_detection_period: TimelineTimeNs::from_duration(Time::new::<second>(3.0)),
            activity_threshold: 500,
            chunk_range: TimelineTimeRangeNs::new(
                TimelineTimeNs::ZERO,
                TimelineTimeNs::from_duration(Time::new::<second>(30.0)),
            ),
            progress_head: TimelineTimeNs::ZERO,
            automatically_advance_chunk_boundaries: true,
            automatically_submit_chunks: true,
        }
    }

    #[must_use]
    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    #[must_use]
    pub const fn preview_range(&self) -> TimelineTimeRangeNs {
        self.preview_range
    }

    #[must_use]
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    #[must_use]
    pub const fn target_text_track_id(&self) -> Option<TimelineTrackId> {
        self.target_text_track_id
    }

    #[must_use]
    pub const fn target_audio_track_id(&self) -> Option<TimelineTrackId> {
        self.target_audio_track_id
    }

    #[must_use]
    pub const fn inactivity_detection_period(&self) -> TimelineTimeNs {
        self.inactivity_detection_period
    }

    #[must_use]
    pub const fn activity_threshold(&self) -> u16 {
        self.activity_threshold
    }

    #[must_use]
    pub const fn chunk_range(&self) -> TimelineTimeRangeNs {
        self.chunk_range
    }

    #[must_use]
    pub const fn progress_head(&self) -> TimelineTimeNs {
        self.progress_head
    }

    #[must_use]
    pub const fn automatically_advance_chunk_boundaries(&self) -> bool {
        self.automatically_advance_chunk_boundaries
    }

    #[must_use]
    pub const fn automatically_submit_chunks(&self) -> bool {
        self.automatically_submit_chunks
    }

    fn set_model_name(&mut self, model_name: impl Into<String>) {
        self.model_name = model_name.into();
    }

    fn set_target_audio_track_id(&mut self, target_audio_track_id: Option<TimelineTrackId>) {
        self.target_audio_track_id = target_audio_track_id;
    }

    fn set_target_text_track_id(&mut self, target_text_track_id: Option<TimelineTrackId>) {
        self.target_text_track_id = target_text_track_id;
    }

    fn set_automation(&mut self, advance_boundaries: bool, submit_chunks: bool) {
        self.automatically_advance_chunk_boundaries = advance_boundaries;
        self.automatically_submit_chunks = submit_chunks;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineTextTrackProjection {
    source_label: String,
    preview_range: TimelineTimeRangeNs,
}

impl TimelineTextTrackProjection {
    #[must_use]
    pub fn new(source_label: impl Into<String>) -> Self {
        Self {
            source_label: source_label.into(),
            preview_range: TimelineTimeRangeNs::new(
                TimelineTimeNs::ZERO,
                TimelineTimeNs::from_duration(Time::new::<millisecond>(320.0)),
            ),
        }
    }

    #[must_use]
    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    #[must_use]
    pub const fn preview_range(&self) -> TimelineTimeRangeNs {
        self.preview_range
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineTracyCompression {
    Lz4,
    Zstd,
}

impl TimelineTracyCompression {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lz4 => "LZ4",
            Self::Zstd => "Zstd",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineTracyCaptureSource {
    path: PathBuf,
    compression: TimelineTracyCompression,
    stream_count: u8,
    file_size_bytes: u64,
}

impl TimelineTracyCaptureSource {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn compression(&self) -> TimelineTracyCompression {
        self.compression
    }

    #[must_use]
    pub const fn stream_count(&self) -> u8 {
        self.stream_count
    }

    #[must_use]
    pub const fn file_size_bytes(&self) -> u64 {
        self.file_size_bytes
    }

    #[must_use]
    pub fn display_name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into(),
        )
    }

    fn from_path(path: &Path) -> eyre::Result<Self> {
        let metadata = std::fs::metadata(path).wrap_err_with(|| {
            format!(
                "failed to read Tracy capture metadata from {}",
                path.display()
            )
        })?;
        let mut file = File::open(path)
            .wrap_err_with(|| format!("failed to open Tracy capture {}", path.display()))?;

        let mut magic = [0_u8; 4];
        file.read_exact(&mut magic).wrap_err_with(|| {
            format!(
                "failed to read Tracy capture header from {}",
                path.display()
            )
        })?;

        let (compression, stream_count) = match magic {
            [b't', b'r', 253, b'P'] => {
                let mut tracy_header = [0_u8; 2];
                file.read_exact(&mut tracy_header).wrap_err_with(|| {
                    format!("failed to read Tracy stream header from {}", path.display())
                })?;
                let compression = match tracy_header[0] {
                    0 => TimelineTracyCompression::Lz4,
                    1 => TimelineTracyCompression::Zstd,
                    other => eyre::bail!(
                        "unsupported Tracy compression type {other} in {}",
                        path.display()
                    ),
                };
                (compression, tracy_header[1].max(1))
            }
            [b't', b'l', b'Z', 4] => (TimelineTracyCompression::Lz4, 1),
            [b't', b'Z', b's', b't'] => (TimelineTracyCompression::Zstd, 1),
            _ => eyre::bail!("{} is not a Tracy capture", path.display()),
        };

        Ok(Self {
            path: path.to_path_buf(),
            compression,
            stream_count,
            file_size_bytes: metadata.len(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineTracingTrackProjection {
    source: TimelineTracyCaptureSource,
    preview_range: TimelineTimeRangeNs,
}

impl TimelineTracingTrackProjection {
    #[must_use]
    pub const fn source(&self) -> &TimelineTracyCaptureSource {
        &self.source
    }

    #[must_use]
    pub const fn preview_range(&self) -> TimelineTimeRangeNs {
        self.preview_range
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineTrackProjection {
    Audio(TimelineAudioTrackProjection),
    Transcription(TimelineTranscriptionTrackProjection),
    Text(TimelineTextTrackProjection),
    TracingSpans(TimelineTracingTrackProjection),
}

impl TimelineTrackProjection {
    #[must_use]
    pub const fn kind(&self) -> TimelineTrackKind {
        match self {
            Self::Audio(_) => TimelineTrackKind::Audio,
            Self::Transcription(_) => TimelineTrackKind::Transcription,
            Self::Text(_) => TimelineTrackKind::Text,
            Self::TracingSpans(_) => TimelineTrackKind::TracingSpans,
        }
    }

    #[must_use]
    pub const fn preview_range(&self) -> TimelineTimeRangeNs {
        match self {
            Self::Audio(projection) => projection.preview_range(),
            Self::Transcription(projection) => projection.preview_range(),
            Self::Text(projection) => projection.preview_range(),
            Self::TracingSpans(projection) => projection.preview_range(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineTrack {
    id: TimelineTrackId,
    name: String,
    projection: TimelineTrackProjection,
}

impl TimelineTrack {
    #[must_use]
    pub fn new_audio(
        id: TimelineTrackId,
        name: impl Into<String>,
        source_label: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            projection: TimelineTrackProjection::Audio(TimelineAudioTrackProjection::new(
                source_label,
            )),
        }
    }

    #[must_use]
    pub fn new_microphone_audio(
        id: TimelineTrackId,
        name: impl Into<String>,
        source_label: impl Into<String>,
        source_device_id: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            projection: TimelineTrackProjection::Audio(
                TimelineAudioTrackProjection::new_microphone(source_label, source_device_id),
            ),
        }
    }

    #[must_use]
    pub fn new_transcription(
        id: TimelineTrackId,
        name: impl Into<String>,
        source_label: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            projection: TimelineTrackProjection::Transcription(
                TimelineTranscriptionTrackProjection::new(source_label),
            ),
        }
    }

    #[must_use]
    pub fn new_text(
        id: TimelineTrackId,
        name: impl Into<String>,
        source_label: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            projection: TimelineTrackProjection::Text(TimelineTextTrackProjection::new(
                source_label,
            )),
        }
    }

    #[must_use]
    pub fn new_tracing_spans(
        id: TimelineTrackId,
        name: impl Into<String>,
        source: TimelineTracyCaptureSource,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            projection: TimelineTrackProjection::TracingSpans(TimelineTracingTrackProjection {
                source,
                preview_range: TimelineTimeRangeNs::new(
                    TimelineTimeNs::ZERO,
                    TimelineTimeNs::from_duration(Time::new::<millisecond>(250.0)),
                ),
            }),
        }
    }

    #[must_use]
    pub const fn id(&self) -> TimelineTrackId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn projection(&self) -> &TimelineTrackProjection {
        &self.projection
    }

    #[must_use]
    pub const fn kind(&self) -> TimelineTrackKind {
        self.projection.kind()
    }

    #[must_use]
    pub const fn preview_range(&self) -> TimelineTimeRangeNs {
        self.projection.preview_range()
    }

    #[must_use]
    pub fn detail_line(&self) -> String {
        match &self.projection {
            TimelineTrackProjection::Audio(projection) => {
                format!("{} · {}", self.kind().label(), projection.source_label())
            }
            TimelineTrackProjection::Transcription(projection) => {
                format!(
                    "{} · {} · {} · in {:?} · out {:?}",
                    self.kind().label(),
                    projection.source_label(),
                    projection.model_name(),
                    projection
                        .target_audio_track_id()
                        .map(TimelineTrackId::as_u64),
                    projection
                        .target_text_track_id()
                        .map(TimelineTrackId::as_u64)
                )
            }
            TimelineTrackProjection::Text(projection) => {
                format!("{} · {}", self.kind().label(), projection.source_label())
            }
            TimelineTrackProjection::TracingSpans(projection) => {
                let source = projection.source();
                format!(
                    "{} · {} · {} stream{}",
                    self.kind().label(),
                    source.display_name(),
                    source.stream_count(),
                    if source.stream_count() == 1 { "" } else { "s" },
                )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineTextBlock {
    track_id: TimelineTrackId,
    time_range: TimelineTimeRangeNs,
    text: String,
}

impl TimelineTextBlock {
    #[must_use]
    pub fn new(
        track_id: TimelineTrackId,
        time_range: TimelineTimeRangeNs,
        text: impl Into<String>,
    ) -> Self {
        Self {
            track_id,
            time_range,
            text: text.into(),
        }
    }

    #[must_use]
    pub const fn track_id(&self) -> TimelineTrackId {
        self.track_id
    }

    #[must_use]
    pub const fn time_range(&self) -> TimelineTimeRangeNs {
        self.time_range
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineViewport {
    origin: TimelineTimeNs,
    duration_per_pixel: Time,
}

impl Default for TimelineViewport {
    fn default() -> Self {
        Self {
            origin: TimelineTimeNs::ZERO,
            duration_per_pixel: Time::new::<millisecond>(1.0),
        }
    }
}

// timeline[impl viewport.projection]
impl TimelineViewport {
    #[must_use]
    pub fn new(origin: TimelineTimeNs, duration_per_pixel: Time) -> Self {
        Self {
            origin,
            duration_per_pixel: if duration_per_pixel <= Time::new::<nanosecond>(0.0) {
                Time::new::<nanosecond>(f64::EPSILON)
            } else {
                duration_per_pixel
            },
        }
    }

    #[must_use]
    pub const fn origin(self) -> TimelineTimeNs {
        self.origin
    }

    #[must_use]
    pub const fn duration_per_pixel(self) -> Time {
        self.duration_per_pixel
    }

    #[must_use]
    pub fn time_to_x(self, time: TimelineTimeNs) -> TimelineViewportPoint {
        let delta = time.duration() - self.origin.duration();
        TimelineViewportPoint::new_pixels(
            delta.get::<nanosecond>() / self.duration_per_pixel.get::<nanosecond>(),
        )
    }

    #[must_use]
    pub fn x_to_time(self, x: TimelineViewportPoint) -> TimelineTimeNs {
        let offset = x.pixels() * self.duration_per_pixel.get::<nanosecond>();
        TimelineTimeNs::new(
            self.origin
                .as_i64()
                .saturating_add(f64_to_i64_saturating(offset.round())),
        )
    }

    #[must_use]
    pub fn visible_duration(self, viewport_width_pixels: i32) -> Time {
        self.duration_per_pixel * f64::from(viewport_width_pixels.max(0))
    }

    #[must_use]
    pub fn project_range(self, range: TimelineTimeRangeNs) -> TimelineProjectedSpan {
        TimelineProjectedSpan {
            start: self.time_to_x(range.start()),
            end: self.time_to_x(range.end()),
        }
    }

    #[must_use]
    // timeline[impl viewport.pan-controls]
    pub fn pan_pixels(self, delta_pixels: i32) -> Self {
        let delta =
            TimelineTimeNs::from_duration(self.duration_per_pixel * f64::from(delta_pixels));
        Self::new(
            TimelineTimeNs::new(self.origin.as_i64().saturating_add(delta.as_i64())),
            self.duration_per_pixel,
        )
    }

    #[must_use]
    // timeline[impl viewport.zoom-controls]
    pub fn scaled(self, factor: f64) -> Self {
        let factor = if factor.is_finite() { factor } else { 1.0 };
        let next_duration_per_pixel = (self.duration_per_pixel.get::<nanosecond>() * factor).clamp(
            TIMELINE_VIEWPORT_MIN_DURATION_PER_PIXEL_NS,
            TIMELINE_VIEWPORT_MAX_DURATION_PER_PIXEL_NS,
        );
        Self::new(
            self.origin,
            Time::new::<nanosecond>(next_duration_per_pixel),
        )
    }

    #[must_use]
    // timeline[impl viewport.zoom-controls]
    pub fn scaled_about(self, anchor: TimelineViewportPoint, factor: f64) -> Self {
        let anchor_time = self.x_to_time(anchor);
        let next = self.scaled(factor);
        let anchor_offset =
            TimelineTimeNs::from_duration(next.duration_per_pixel * anchor.pixels());
        Self::new(
            TimelineTimeNs::new(anchor_time.as_i64().saturating_sub(anchor_offset.as_i64())),
            next.duration_per_pixel,
        )
    }

    #[must_use]
    // timeline[impl ruler.ticks]
    // timeline[impl viewport.typed-projection]
    pub fn ruler_ticks(
        self,
        viewport_width_pixels: i32,
        target_tick_count: usize,
    ) -> Vec<TimelineRulerTick> {
        if viewport_width_pixels <= 0 || target_tick_count == 0 {
            return Vec::new();
        }

        let visible_end = self.x_to_time(TimelineViewportPoint::new_pixels(f64::from(
            viewport_width_pixels,
        )));
        let target_spacing = (visible_end.as_i64() - self.origin.as_i64())
            .abs()
            .saturating_div(i64::try_from(target_tick_count).unwrap_or(1).max(1))
            .max(1);
        let step_ns = nice_time_step_ns(target_spacing);
        let first_tick_ns = self.origin.as_i64().div_euclid(step_ns) * step_ns;
        let mut current_tick_ns = first_tick_ns;
        let mut ticks = Vec::new();

        while current_tick_ns <= visible_end.as_i64().saturating_add(step_ns) && ticks.len() < 256 {
            let time = TimelineTimeNs::new(current_tick_ns);
            let x = self.time_to_x(time);
            if x.pixels() >= 0.0 && x.pixels() <= f64::from(viewport_width_pixels) {
                ticks.push(TimelineRulerTick {
                    time,
                    x,
                    label: format_timeline_time_label(time),
                });
            }
            let next_tick_ns = current_tick_ns.saturating_add(step_ns);
            if next_tick_ns == current_tick_ns {
                break;
            }
            current_tick_ns = next_tick_ns;
        }

        ticks
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "rounded viewport pixel projection is clamped before returning to integer nanoseconds"
)]
fn f64_to_i64_saturating(value: f64) -> i64 {
    if value.is_nan() {
        0
    } else if value >= i64::MAX as f64 {
        i64::MAX
    } else if value <= i64::MIN as f64 {
        i64::MIN
    } else {
        value as i64
    }
}

fn nice_time_step_ns(target_spacing_ns: i64) -> i64 {
    let mut magnitude = 1_i64;
    let target_spacing_ns = target_spacing_ns.abs().max(1);
    while magnitude <= target_spacing_ns.saturating_div(10) {
        magnitude = magnitude.saturating_mul(10);
    }

    for factor in [1_i64, 2, 5, 10] {
        let candidate = magnitude.saturating_mul(factor);
        if candidate >= target_spacing_ns {
            return candidate;
        }
    }

    target_spacing_ns
}

fn format_timeline_time_label(time: TimelineTimeNs) -> String {
    let nanoseconds = time.as_i64();
    let absolute_nanoseconds = nanoseconds.abs();
    if absolute_nanoseconds >= 1_000_000_000 {
        format!("{:.3} s", time.duration().get::<second>())
    } else if absolute_nanoseconds >= 1_000_000 {
        format_scaled_time_label(nanoseconds, 1_000_000, "ms")
    } else if absolute_nanoseconds >= 1_000 {
        format_scaled_time_label(nanoseconds, 1_000, "us")
    } else {
        format!("{nanoseconds} ns")
    }
}

fn format_scaled_time_label(nanoseconds: i64, divisor: u64, unit: &str) -> String {
    let sign = if nanoseconds < 0 { "-" } else { "" };
    let absolute_nanoseconds = nanoseconds.unsigned_abs();
    let whole = absolute_nanoseconds / divisor;
    let tenths = (absolute_nanoseconds % divisor).saturating_mul(10) / divisor;
    format!("{sign}{whole}.{tenths} {unit}")
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineDocument {
    id: TimelineDocumentId,
    title: String,
    subtitle: String,
    tracks: Vec<TimelineTrack>,
    text_blocks: Vec<TimelineTextBlock>,
    viewport: TimelineViewport,
    edits: Vec<TimelineEdit>,
}

impl TimelineDocument {
    #[must_use]
    // timeline[impl document.blank-model]
    // timeline[impl viewport.nanoseconds]
    pub fn blank() -> Self {
        Self {
            id: TimelineDocumentId::DEFAULT_BLANK,
            title: "Blank timeline".to_owned(),
            subtitle: "Import a Tracy capture or add audio, transcription, and text tracks."
                .to_owned(),
            tracks: Vec::new(),
            text_blocks: Vec::new(),
            viewport: TimelineViewport::default(),
            edits: Vec::new(),
        }
    }

    // timeline[impl import.tracy.document]
    // timeline[impl track.kinds]
    // timeline[impl track.projection-model]
    // timeline[impl edit-list.model]
    /// # Errors
    ///
    /// Returns an error when the file cannot be opened, cannot be read, or does
    /// not start with a supported Tracy capture header.
    pub fn import_tracy_capture(path: &Path) -> eyre::Result<Self> {
        let source = TimelineTracyCaptureSource::from_path(path)?;
        let document_id = hashed_non_zero_id(path, source.file_size_bytes());
        let track_id = hashed_non_zero_id(path, source.file_size_bytes().saturating_add(1));
        let title = source.display_name();
        let track_name = format!("Tracing spans · {title}");
        let subtitle = format!(
            "Tracy capture · {} · {} stream{} · {} bytes",
            source.compression().label(),
            source.stream_count(),
            if source.stream_count() == 1 { "" } else { "s" },
            source.file_size_bytes(),
        );

        Ok(Self {
            id: TimelineDocumentId(document_id),
            title,
            subtitle,
            tracks: vec![TimelineTrack::new_tracing_spans(
                TimelineTrackId::new(track_id),
                track_name,
                source,
            )],
            text_blocks: Vec::new(),
            viewport: TimelineViewport::default(),
            edits: Vec::new(),
        })
    }

    #[must_use]
    pub const fn id(&self) -> TimelineDocumentId {
        self.id
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn subtitle(&self) -> &str {
        &self.subtitle
    }

    #[must_use]
    pub fn tracks(&self) -> &[TimelineTrack] {
        &self.tracks
    }

    #[must_use]
    pub fn text_blocks(&self) -> &[TimelineTextBlock] {
        &self.text_blocks
    }

    #[must_use]
    pub const fn viewport(&self) -> TimelineViewport {
        self.viewport
    }

    #[must_use]
    pub fn edits(&self) -> &[TimelineEdit] {
        &self.edits
    }

    // timeline[impl add-track.tracy]
    /// # Errors
    ///
    /// Returns an error when the file cannot be opened, cannot be read, or does
    /// not start with a supported Tracy capture header.
    pub fn append_tracy_capture_track(&mut self, path: &Path) -> eyre::Result<()> {
        let source = TimelineTracyCaptureSource::from_path(path)?;
        self.retitle_for_composition_if_blank();
        let track_name = format!("Tracing spans · {}", source.display_name());
        let track_id = self.next_track_id();
        self.tracks.push(TimelineTrack::new_tracing_spans(
            track_id, track_name, source,
        ));
        Ok(())
    }

    #[must_use]
    // timeline[impl add-track.microphone-placeholder]
    pub fn append_microphone_track(&mut self) -> TimelineTrackId {
        let microphone_track_number = self
            .tracks
            .iter()
            .filter(|track| track.kind() == TimelineTrackKind::Audio)
            .count()
            + 1;
        self.append_audio_track(
            format!("Microphone {microphone_track_number}"),
            "Pending microphone recording",
        )
    }

    #[must_use]
    pub fn append_microphone_track_for_device(
        &mut self,
        device_name: impl Into<String>,
    ) -> TimelineTrackId {
        let device_name = device_name.into();
        self.append_audio_track(device_name.clone(), device_name)
    }

    #[must_use]
    pub fn append_microphone_track_for_device_id(
        &mut self,
        device_name: impl Into<String>,
        device_id: impl Into<String>,
    ) -> TimelineTrackId {
        let device_name = device_name.into();
        let device_id = device_id.into();
        self.retitle_for_composition_if_blank();
        let track_id = self.next_track_id();
        self.tracks.push(TimelineTrack::new_microphone_audio(
            track_id,
            device_name.clone(),
            device_name,
            device_id,
        ));
        track_id
    }

    #[must_use]
    pub fn append_transcription_track(&mut self) -> TimelineTrackId {
        let track_number = self
            .tracks
            .iter()
            .filter(|track| track.kind() == TimelineTrackKind::Transcription)
            .count()
            + 1;
        self.append_transcription_track_with_source(format!("Transcription {track_number}"))
    }

    #[must_use]
    pub fn append_text_track(&mut self) -> TimelineTrackId {
        let track_number = self
            .tracks
            .iter()
            .filter(|track| track.kind() == TimelineTrackKind::Text)
            .count()
            + 1;
        self.append_text_track_with_source(format!("Text {track_number}"))
    }

    #[must_use]
    pub fn append_empty_text_block(
        &mut self,
        track_id: TimelineTrackId,
        time_range: TimelineTimeRangeNs,
    ) -> bool {
        self.append_text_block(track_id, time_range, String::new())
    }

    #[must_use]
    pub fn append_text_block(
        &mut self,
        track_id: TimelineTrackId,
        time_range: TimelineTimeRangeNs,
        text: impl Into<String>,
    ) -> bool {
        if !self
            .tracks
            .iter()
            .any(|track| track.id() == track_id && track.kind() == TimelineTrackKind::Text)
        {
            return false;
        }
        self.text_blocks
            .push(TimelineTextBlock::new(track_id, time_range, text));
        true
    }

    #[must_use]
    pub fn move_track(&mut self, from_index: usize, to_index: usize) -> bool {
        if from_index >= self.tracks.len()
            || to_index >= self.tracks.len()
            || from_index == to_index
        {
            return from_index < self.tracks.len() && to_index < self.tracks.len();
        }

        let track = self.tracks.remove(from_index);
        self.tracks.insert(to_index, track);
        true
    }

    #[must_use]
    pub fn restore_track_order(&mut self, track_order: &[TimelineTrackId]) -> bool {
        if track_order.len() != self.tracks.len() {
            return false;
        }

        let original_tracks = self.tracks.clone();
        let mut remaining = original_tracks.clone();
        let mut reordered = Vec::with_capacity(remaining.len());
        for track_id in track_order {
            let Some(index) = remaining.iter().position(|track| track.id() == *track_id) else {
                return false;
            };
            reordered.push(remaining.remove(index));
        }
        if !remaining.is_empty() {
            return false;
        }
        self.tracks = reordered;
        true
    }

    #[must_use]
    pub fn set_transcription_track_model_name(
        &mut self,
        track_id: TimelineTrackId,
        model_name: impl Into<String>,
    ) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|track| track.id() == track_id) else {
            return false;
        };
        let TimelineTrackProjection::Transcription(projection) = &mut track.projection else {
            return false;
        };
        projection.set_model_name(model_name);
        true
    }

    #[must_use]
    pub fn set_transcription_track_target_text_track(
        &mut self,
        track_id: TimelineTrackId,
        target_text_track_id: Option<TimelineTrackId>,
    ) -> bool {
        if let Some(target_text_track_id) = target_text_track_id
            && !self.tracks.iter().any(|track| {
                track.id() == target_text_track_id && track.kind() == TimelineTrackKind::Text
            })
        {
            return false;
        }

        let Some(track) = self.tracks.iter_mut().find(|track| track.id() == track_id) else {
            return false;
        };
        let TimelineTrackProjection::Transcription(projection) = &mut track.projection else {
            return false;
        };
        projection.set_target_text_track_id(target_text_track_id);
        true
    }

    #[must_use]
    pub fn set_transcription_track_target_audio_track(
        &mut self,
        track_id: TimelineTrackId,
        target_audio_track_id: Option<TimelineTrackId>,
    ) -> bool {
        if let Some(target_audio_track_id) = target_audio_track_id
            && !self.tracks.iter().any(|track| {
                track.id() == target_audio_track_id && track.kind() == TimelineTrackKind::Audio
            })
        {
            return false;
        }

        let Some(track) = self.tracks.iter_mut().find(|track| track.id() == track_id) else {
            return false;
        };
        let TimelineTrackProjection::Transcription(projection) = &mut track.projection else {
            return false;
        };
        projection.set_target_audio_track_id(target_audio_track_id);
        true
    }

    #[must_use]
    pub fn set_transcription_track_automation(
        &mut self,
        track_id: TimelineTrackId,
        advance_boundaries: bool,
        submit_chunks: bool,
    ) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|track| track.id() == track_id) else {
            return false;
        };
        let TimelineTrackProjection::Transcription(projection) = &mut track.projection else {
            return false;
        };
        projection.set_automation(advance_boundaries, submit_chunks);
        true
    }

    // timeline[impl viewport.pan-controls]
    pub fn pan_viewport_left(&mut self) {
        self.viewport = self.viewport.pan_pixels(-TIMELINE_VIEWPORT_PAN_STEP_PIXELS);
    }

    // timeline[impl viewport.pan-controls]
    pub fn pan_viewport_right(&mut self) {
        self.viewport = self.viewport.pan_pixels(TIMELINE_VIEWPORT_PAN_STEP_PIXELS);
    }

    // timeline[impl viewport.zoom-controls]
    pub fn zoom_viewport_in(&mut self) {
        self.viewport = self.viewport.scaled(0.5);
    }

    // timeline[impl viewport.zoom-controls]
    pub fn zoom_viewport_out(&mut self) {
        self.viewport = self.viewport.scaled(2.0);
    }

    // timeline[impl viewport.zoom-controls]
    // timeline[impl viewport.mouse-zoom-anchor]
    pub fn zoom_viewport_about(&mut self, anchor_x_pixels: f64, factor: f64) {
        self.viewport = self
            .viewport
            .scaled_about(TimelineViewportPoint::new_pixels(anchor_x_pixels), factor);
    }

    pub fn set_viewport(&mut self, viewport: TimelineViewport) {
        self.viewport = viewport;
    }

    fn next_track_id(&self) -> TimelineTrackId {
        TimelineTrackId::new(hashed_non_zero_id(
            (self.id.as_u64(), self.tracks.len(), self.edits.len()),
            self.tracks.len().saturating_add(1) as u64,
        ))
    }

    fn append_audio_track(
        &mut self,
        name: impl Into<String>,
        source_label: impl Into<String>,
    ) -> TimelineTrackId {
        self.retitle_for_composition_if_blank();
        let track_id = self.next_track_id();
        self.tracks
            .push(TimelineTrack::new_audio(track_id, name, source_label));
        track_id
    }

    fn append_transcription_track_with_source(
        &mut self,
        source_label: impl Into<String>,
    ) -> TimelineTrackId {
        self.retitle_for_composition_if_blank();
        let source_label = source_label.into();
        let track_id = self.next_track_id();
        self.tracks.push(TimelineTrack::new_transcription(
            track_id,
            source_label.clone(),
            source_label,
        ));
        track_id
    }

    fn append_text_track_with_source(
        &mut self,
        source_label: impl Into<String>,
    ) -> TimelineTrackId {
        self.retitle_for_composition_if_blank();
        let source_label = source_label.into();
        let track_id = self.next_track_id();
        self.tracks.push(TimelineTrack::new_text(
            track_id,
            source_label.clone(),
            source_label,
        ));
        track_id
    }

    fn retitle_for_composition_if_blank(&mut self) {
        if self.id == TimelineDocumentId::DEFAULT_BLANK && self.tracks.is_empty() {
            "Timeline composition".clone_into(&mut self.title);
            "Mix tracing captures, live audio, transcription, and text blocks in one non-destructive document."
                .clone_into(&mut self.subtitle);
        }
    }
}

fn hashed_non_zero_id(value: impl Hash, salt: u64) -> NonZeroU64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    salt.hash(&mut hasher);
    let hash = hasher.finish().max(1);
    NonZeroU64::new(hash).unwrap_or(NonZeroU64::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    // timeline[verify document.blank-model]
    fn blank_document_starts_empty_at_zero_nanoseconds() {
        let document = TimelineDocument::blank();

        assert_eq!(document.id(), TimelineDocumentId::DEFAULT_BLANK);
        assert!(document.tracks().is_empty());
        assert_eq!(document.viewport().origin(), TimelineTimeNs::ZERO);
        assert!(document.edits().is_empty());
    }

    #[test]
    // timeline[verify track.kinds]
    // timeline[verify track.projection-model]
    // timeline[verify import.tracy.document]
    // timeline[verify track.preview-ranges]
    fn import_tracy_capture_creates_tracing_track_from_header() {
        let temp_dir = tempdir().expect("tempdir");
        let capture_path = temp_dir.path().join("capture.tracy");
        std::fs::write(&capture_path, [b't', b'r', 253, b'P', 1, 4, 0, 0]).expect("capture");

        let document = TimelineDocument::import_tracy_capture(&capture_path).expect("import");

        assert_eq!(document.title(), "capture.tracy");
        assert_eq!(document.tracks().len(), 1);
        match document.tracks()[0].projection() {
            TimelineTrackProjection::TracingSpans(projection) => {
                assert_eq!(
                    projection.source().compression(),
                    TimelineTracyCompression::Zstd
                );
                assert_eq!(projection.source().stream_count(), 4);
                assert_eq!(projection.source().path(), capture_path.as_path());
                assert_eq!(
                    projection.preview_range(),
                    TimelineTimeRangeNs::new(
                        TimelineTimeNs::ZERO,
                        TimelineTimeNs::from_duration(Time::new::<millisecond>(250.0)),
                    )
                );
            }
            other => panic!("expected tracing track, got {other:?}"),
        }
    }

    #[test]
    // timeline[verify import.tracy.document]
    fn import_tracy_capture_rejects_non_tracy_files() {
        let temp_dir = tempdir().expect("tempdir");
        let capture_path = temp_dir.path().join("not-tracy.bin");
        std::fs::write(&capture_path, b"not a tracy dump").expect("payload");

        let error = TimelineDocument::import_tracy_capture(&capture_path).expect_err("error");

        assert!(error.to_string().contains("not a Tracy capture"));
    }

    #[test]
    // timeline[verify add-track.tracy]
    fn append_tracy_capture_track_preserves_existing_tracks() {
        let temp_dir = tempdir().expect("tempdir");
        let capture_path = temp_dir.path().join("capture.tracy");
        std::fs::write(&capture_path, [b't', b'r', 253, b'P', 0, 2, 0, 0]).expect("capture");
        let mut document = TimelineDocument::blank();
        let audio_track_id = document.append_microphone_track();

        document
            .append_tracy_capture_track(&capture_path)
            .expect("append tracy track");

        assert_eq!(document.tracks().len(), 2);
        assert_eq!(document.tracks()[0].id(), audio_track_id);
        assert_eq!(document.tracks()[0].kind(), TimelineTrackKind::Audio);
        assert_eq!(document.tracks()[1].kind(), TimelineTrackKind::TracingSpans);
    }

    #[test]
    // timeline[verify add-track.microphone-placeholder]
    // timeline[verify track.preview-ranges]
    fn append_microphone_track_creates_audio_track_placeholder() {
        let mut document = TimelineDocument::blank();

        let track_id = document.append_microphone_track();

        assert_eq!(document.title(), "Timeline composition");
        assert_eq!(document.tracks().len(), 1);
        assert_eq!(document.tracks()[0].id(), track_id);
        assert_eq!(document.tracks()[0].name(), "Microphone 1");
        assert_eq!(document.tracks()[0].kind(), TimelineTrackKind::Audio);
        assert_eq!(
            document.tracks()[0].preview_range(),
            TimelineTimeRangeNs::new(
                TimelineTimeNs::ZERO,
                TimelineTimeNs::from_duration(Time::new::<millisecond>(320.0)),
            )
        );
        assert!(
            document.tracks()[0]
                .detail_line()
                .contains("Pending microphone recording")
        );
    }

    #[test]
    // timeline[verify viewport.nanoseconds]
    // timeline[verify viewport.projection]
    fn viewport_projects_integer_nanosecond_positions_to_pixels() {
        let viewport =
            TimelineViewport::new(TimelineTimeNs::new(1_000), Time::new::<nanosecond>(10.0));

        assert!((viewport.time_to_x(TimelineTimeNs::new(1_250)).pixels() - 25.0).abs() < 1e-9);
        assert_eq!(
            viewport.x_to_time(TimelineViewportPoint::new_pixels(25.0)),
            TimelineTimeNs::new(1_250)
        );
    }

    #[test]
    // timeline[verify viewport.projection]
    fn viewport_projection_does_not_mutate_origin() {
        let viewport =
            TimelineViewport::new(TimelineTimeNs::new(500), Time::new::<nanosecond>(100.0));

        let _ = viewport.time_to_x(TimelineTimeNs::new(700));
        let _ = viewport.x_to_time(TimelineViewportPoint::new_pixels(2.0));

        assert_eq!(viewport.origin(), TimelineTimeNs::new(500));
    }

    #[test]
    // timeline[verify viewport.typed-projection]
    // timeline[verify ruler.ticks]
    fn viewport_projection_and_ruler_ticks_use_typed_spaces() {
        let viewport = TimelineViewport::new(TimelineTimeNs::new(0), Time::new::<millisecond>(1.0));

        let projected = viewport.project_range(TimelineTimeRangeNs::new(
            TimelineTimeNs::new(0),
            TimelineTimeNs::from_duration(Time::new::<millisecond>(250.0)),
        ));
        let ticks = viewport.ruler_ticks(800, 5);

        assert!(projected.start().pixels().abs() < 1e-9);
        assert!((projected.end().pixels() - 250.0).abs() < 1e-9);
        assert!((projected.width().get::<meter>() - 250.0).abs() < 1e-9);
        assert!(!ticks.is_empty());
        assert_eq!(ticks[0].time(), TimelineTimeNs::new(0));
        assert!(
            ticks
                .windows(2)
                .all(|pair| pair[0].x().pixels() < pair[1].x().pixels())
        );
    }

    #[test]
    // timeline[verify viewport.pan-controls]
    // timeline[verify viewport.zoom-controls]
    fn document_viewport_pan_and_zoom_mutate_projection_without_touching_tracks() {
        let mut document = TimelineDocument::blank();
        let original_viewport = document.viewport();

        document.pan_viewport_right();
        let panned_viewport = document.viewport();
        assert!(panned_viewport.origin().as_i64() > original_viewport.origin().as_i64());
        assert_eq!(
            panned_viewport.duration_per_pixel(),
            original_viewport.duration_per_pixel()
        );

        document.pan_viewport_left();
        assert_eq!(document.viewport().origin(), original_viewport.origin());

        document.zoom_viewport_in();
        assert!(
            document.viewport().duration_per_pixel().get::<nanosecond>()
                < original_viewport.duration_per_pixel().get::<nanosecond>()
        );

        document.zoom_viewport_out();
        assert_eq!(
            document.viewport().duration_per_pixel().get::<nanosecond>(),
            original_viewport.duration_per_pixel().get::<nanosecond>()
        );
        assert!(document.tracks().is_empty());
    }

    #[test]
    // timeline[verify viewport.zoom-controls]
    // timeline[verify viewport.mouse-zoom-anchor]
    fn viewport_zoom_about_keeps_anchor_time_pinned_to_the_same_pixel() {
        let viewport =
            TimelineViewport::new(TimelineTimeNs::new(1_000), Time::new::<nanosecond>(100.0));
        let anchor = TimelineViewportPoint::new_pixels(25.0);
        let anchor_time = viewport.x_to_time(anchor);

        let zoomed = viewport.scaled_about(anchor, 0.5);

        assert_eq!(zoomed.x_to_time(anchor), anchor_time);
        assert!(
            zoomed.duration_per_pixel().get::<nanosecond>()
                < viewport.duration_per_pixel().get::<nanosecond>()
        );
    }

    #[test]
    // timeline[verify transcription.defaults]
    fn transcription_tracks_start_with_default_model_targets_and_automation() {
        let mut document = TimelineDocument::blank();
        let track_id = document.append_transcription_track();

        let TimelineTrackProjection::Transcription(projection) = document
            .tracks()
            .iter()
            .find(|track| track.id() == track_id)
            .expect("transcription track")
            .projection()
        else {
            panic!("expected transcription track projection");
        };

        assert_eq!(projection.model_name(), DEFAULT_TRANSCRIPTION_MODEL_NAME);
        assert_eq!(projection.target_audio_track_id(), None);
        assert_eq!(projection.target_text_track_id(), None);
        assert_eq!(
            projection.inactivity_detection_period(),
            TimelineTimeNs::from_duration(Time::new::<second>(3.0))
        );
        assert_eq!(projection.activity_threshold(), 500);
        assert_eq!(
            projection.chunk_range(),
            TimelineTimeRangeNs::new(
                TimelineTimeNs::ZERO,
                TimelineTimeNs::from_duration(Time::new::<second>(30.0)),
            )
        );
        assert_eq!(projection.progress_head(), TimelineTimeNs::ZERO);
        assert!(projection.automatically_advance_chunk_boundaries());
        assert!(projection.automatically_submit_chunks());
    }

    #[test]
    // timeline[verify transcription.targets]
    // timeline[verify transcription.settings]
    fn document_updates_transcription_track_settings_against_real_source_and_output_tracks() {
        let mut document = TimelineDocument::blank();
        let audio_track_id = document.append_microphone_track();
        let transcription_track_id = document.append_transcription_track();
        let text_track_id = document.append_text_track();

        assert!(document.set_transcription_track_model_name(transcription_track_id, "small.en"));
        assert!(document.set_transcription_track_target_audio_track(
            transcription_track_id,
            Some(audio_track_id),
        ));
        assert!(document.set_transcription_track_target_text_track(
            transcription_track_id,
            Some(text_track_id),
        ));
        assert!(document.set_transcription_track_automation(transcription_track_id, false, true,));
        assert!(!document.set_transcription_track_target_audio_track(
            transcription_track_id,
            Some(text_track_id),
        ));
        assert!(!document.set_transcription_track_target_text_track(
            transcription_track_id,
            Some(transcription_track_id),
        ));

        let TimelineTrackProjection::Transcription(projection) = document
            .tracks()
            .iter()
            .find(|track| track.id() == transcription_track_id)
            .expect("transcription track")
            .projection()
        else {
            panic!("expected transcription track projection");
        };

        assert_eq!(projection.model_name(), "small.en");
        assert_eq!(projection.target_audio_track_id(), Some(audio_track_id));
        assert_eq!(projection.target_text_track_id(), Some(text_track_id));
        assert!(!projection.automatically_advance_chunk_boundaries());
        assert!(projection.automatically_submit_chunks());
    }

    #[test]
    fn document_can_move_and_restore_track_order() {
        let mut document = TimelineDocument::blank();
        let first_track_id = document.append_microphone_track();
        let transcription_track_id = document.append_transcription_track();
        let text_track_id = document.append_text_track();
        let original_order = document
            .tracks()
            .iter()
            .map(TimelineTrack::id)
            .collect::<Vec<_>>();

        assert!(document.move_track(0, 2));
        assert_eq!(
            document
                .tracks()
                .iter()
                .map(TimelineTrack::id)
                .collect::<Vec<_>>(),
            vec![transcription_track_id, text_track_id, first_track_id]
        );
        assert!(document.restore_track_order(&original_order));
        assert_eq!(
            document
                .tracks()
                .iter()
                .map(TimelineTrack::id)
                .collect::<Vec<_>>(),
            original_order
        );
    }
}
