use eyre::{Result, eyre};
use facet::Facet;

use crate::{CanonicalTimeKey, CanonicalTimelineOffset};

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct CanonicalTimeRange {
    start: CanonicalTimeKey,
    end: CanonicalTimeKey,
}

impl CanonicalTimeRange {
    /// # Errors
    ///
    /// Returns an error when `end` is earlier than `start`.
    pub fn try_new(start: CanonicalTimeKey, end: CanonicalTimeKey) -> Result<Self> {
        if end < start {
            return Err(eyre!(
                "timeline range end {} is earlier than start {}",
                end.raw_femtoseconds(),
                start.raw_femtoseconds()
            ));
        }

        Ok(Self { start, end })
    }

    #[must_use]
    pub fn from_unordered(first: CanonicalTimeKey, other: CanonicalTimeKey) -> Self {
        if first <= other {
            Self {
                start: first,
                end: other,
            }
        } else {
            Self {
                start: other,
                end: first,
            }
        }
    }

    #[must_use]
    pub const fn start(self) -> CanonicalTimeKey {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> CanonicalTimeKey {
        self.end
    }

    #[must_use]
    pub fn duration(self) -> CanonicalTimelineOffset {
        self.end
            .as_offset()
            .checked_sub(self.start.as_offset())
            .expect("validated canonical time range should not underflow")
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalTimeRange;
    use crate::CanonicalTimeKey;

    #[test]
    fn canonical_time_range_accepts_equal_endpoints() {
        let range = CanonicalTimeRange::try_new(
            CanonicalTimeKey::from_femtoseconds(7),
            CanonicalTimeKey::from_femtoseconds(7),
        )
        .expect("equal endpoints should be valid");

        assert_eq!(range.duration().raw_value(), 0);
    }

    #[test]
    fn canonical_time_range_rejects_reversed_endpoints() {
        let error = CanonicalTimeRange::try_new(
            CanonicalTimeKey::from_femtoseconds(9),
            CanonicalTimeKey::from_femtoseconds(3),
        )
        .expect_err("reversed endpoints should be rejected");

        assert_eq!(
            error.to_string(),
            "timeline range end 3 is earlier than start 9"
        );
    }

    #[test]
    fn canonical_time_range_from_unordered_sorts_endpoints() {
        let range = CanonicalTimeRange::from_unordered(
            CanonicalTimeKey::from_femtoseconds(12),
            CanonicalTimeKey::from_femtoseconds(-5),
        );

        assert_eq!(range.start().raw_femtoseconds(), -5);
        assert_eq!(range.end().raw_femtoseconds(), 12);
    }
}
