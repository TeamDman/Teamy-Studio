use arbitrary::Arbitrary;
use facet::Facet;

#[derive(Arbitrary, Facet, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineInstantNs(i64);

impl TimelineInstantNs {
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

#[derive(Arbitrary, Facet, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineDurationNs(u64);

impl TimelineDurationNs {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(nanoseconds: u64) -> Self {
        Self(nanoseconds)
    }

    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
// timeline[impl display.time-strict]
pub struct TimelineRangeNs {
    start: TimelineInstantNs,
    end: TimelineInstantNs,
}

impl TimelineRangeNs {
    /// # Errors
    ///
    /// Returns an error when `end` is earlier than `start`.
    pub fn try_new(start: TimelineInstantNs, end: TimelineInstantNs) -> eyre::Result<Self> {
        if end < start {
            eyre::bail!(
                "timeline range end {} is earlier than start {}",
                end.as_i64(),
                start.as_i64()
            );
        }
        Ok(Self { start, end })
    }

    #[must_use]
    pub const fn start(self) -> TimelineInstantNs {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> TimelineInstantNs {
        self.end
    }

    #[must_use]
    pub fn duration(self) -> TimelineDurationNs {
        let duration = i128::from(self.end.as_i64()) - i128::from(self.start.as_i64());
        TimelineDurationNs::new(u64::try_from(duration).unwrap_or(u64::MAX))
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.as_i64() == self.end.as_i64()
    }
}

impl<'a> Arbitrary<'a> for TimelineRangeNs {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let first = TimelineInstantNs::arbitrary(u)?;
        let second = TimelineInstantNs::arbitrary(u)?;
        let (start, end) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        Ok(Self { start, end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // timeline[verify display.time-strict]
    fn range_creation_accepts_ordered_endpoints() {
        let range =
            TimelineRangeNs::try_new(TimelineInstantNs::new(10), TimelineInstantNs::new(42))
                .expect("valid range");

        assert_eq!(range.start(), TimelineInstantNs::new(10));
        assert_eq!(range.end(), TimelineInstantNs::new(42));
        assert_eq!(range.duration(), TimelineDurationNs::new(32));
        assert!(!range.is_empty());
    }

    #[test]
    // timeline[verify display.time-strict]
    fn range_creation_accepts_empty_ranges() {
        let range =
            TimelineRangeNs::try_new(TimelineInstantNs::new(-7), TimelineInstantNs::new(-7))
                .expect("empty range");

        assert_eq!(range.duration(), TimelineDurationNs::ZERO);
        assert!(range.is_empty());
    }

    #[test]
    // timeline[verify display.time-strict]
    fn range_creation_rejects_reversed_endpoints() {
        let error =
            TimelineRangeNs::try_new(TimelineInstantNs::new(42), TimelineInstantNs::new(10))
                .expect_err("reversed range must fail");

        assert!(error.to_string().contains("earlier than start"));
    }

    #[test]
    fn instant_ordering_is_nanosecond_ordering() {
        assert!(TimelineInstantNs::new(-1) < TimelineInstantNs::ZERO);
        assert!(TimelineInstantNs::new(1) > TimelineInstantNs::ZERO);
    }

    #[test]
    fn duration_can_represent_the_full_i64_instant_span() {
        let range = TimelineRangeNs::try_new(
            TimelineInstantNs::new(i64::MIN),
            TimelineInstantNs::new(i64::MAX),
        )
        .expect("full range");

        assert_eq!(range.duration(), TimelineDurationNs::new(u64::MAX));
    }

    #[test]
    // timeline[verify display.time-strict]
    fn arbitrary_ranges_are_always_ordered() {
        for seed in 0_u8..=u8::MAX {
            let bytes = [seed; 32];
            let mut unstructured = arbitrary::Unstructured::new(&bytes);
            let Ok(range) = TimelineRangeNs::arbitrary(&mut unstructured) else {
                continue;
            };

            assert!(range.start() <= range.end());
        }
    }
}
