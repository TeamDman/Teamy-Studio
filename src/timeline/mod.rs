use std::num::NonZeroU64;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineTrack {
    id: TimelineTrackId,
    name: String,
}

impl TimelineTrack {
    #[must_use]
    pub fn new(id: TimelineTrackId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineViewport {
    origin: TimelineTimeNs,
    nanoseconds_per_pixel: f64,
}

impl Default for TimelineViewport {
    fn default() -> Self {
        Self {
            origin: TimelineTimeNs::ZERO,
            nanoseconds_per_pixel: 1_000_000.0,
        }
    }
}

// timeline[impl viewport.projection]
impl TimelineViewport {
    #[must_use]
    pub fn new(origin: TimelineTimeNs, nanoseconds_per_pixel: f64) -> Self {
        Self {
            origin,
            nanoseconds_per_pixel: nanoseconds_per_pixel.max(f64::EPSILON),
        }
    }

    #[must_use]
    pub const fn origin(self) -> TimelineTimeNs {
        self.origin
    }

    #[must_use]
    pub const fn nanoseconds_per_pixel(self) -> f64 {
        self.nanoseconds_per_pixel
    }

    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "timeline math keeps stored positions as integer nanoseconds and converts only at the pixel projection boundary"
    )]
    pub fn time_to_x(self, time: TimelineTimeNs) -> f64 {
        (time.as_i64().saturating_sub(self.origin.as_i64()) as f64) / self.nanoseconds_per_pixel
    }

    #[must_use]
    pub fn x_to_time(self, x: f64) -> TimelineTimeNs {
        let offset = (x * self.nanoseconds_per_pixel).round();
        TimelineTimeNs::new(
            self.origin
                .as_i64()
                .saturating_add(f64_to_i64_saturating(offset)),
        )
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

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineDocument {
    id: TimelineDocumentId,
    tracks: Vec<TimelineTrack>,
    viewport: TimelineViewport,
}

impl TimelineDocument {
    #[must_use]
    // timeline[impl document.blank-model]
    // timeline[impl viewport.nanoseconds]
    pub fn blank() -> Self {
        Self {
            id: TimelineDocumentId::DEFAULT_BLANK,
            tracks: Vec::new(),
            viewport: TimelineViewport::default(),
        }
    }

    #[must_use]
    pub const fn id(&self) -> TimelineDocumentId {
        self.id
    }

    #[must_use]
    pub fn tracks(&self) -> &[TimelineTrack] {
        &self.tracks
    }

    #[must_use]
    pub const fn viewport(&self) -> TimelineViewport {
        self.viewport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // timeline[verify document.blank-model]
    fn blank_document_starts_empty_at_zero_nanoseconds() {
        let document = TimelineDocument::blank();

        assert_eq!(document.id(), TimelineDocumentId::DEFAULT_BLANK);
        assert!(document.tracks().is_empty());
        assert_eq!(document.viewport().origin(), TimelineTimeNs::ZERO);
    }

    #[test]
    // timeline[verify viewport.nanoseconds]
    // timeline[verify viewport.projection]
    fn viewport_projects_integer_nanosecond_positions_to_pixels() {
        let viewport = TimelineViewport::new(TimelineTimeNs::new(1_000), 10.0);

        assert_eq!(viewport.time_to_x(TimelineTimeNs::new(1_250)), 25.0);
        assert_eq!(viewport.x_to_time(25.0), TimelineTimeNs::new(1_250));
    }

    #[test]
    // timeline[verify viewport.projection]
    fn viewport_projection_does_not_mutate_origin() {
        let viewport = TimelineViewport::new(TimelineTimeNs::new(500), 100.0);

        let _ = viewport.time_to_x(TimelineTimeNs::new(700));
        let _ = viewport.x_to_time(2.0);

        assert_eq!(viewport.origin(), TimelineTimeNs::new(500));
    }
}
