use facet::Facet;

use super::{
    TimeLikeFacetProxy, TimeUnit, TimelineArithmeticError, TimelineOffset, TimelineRepr,
    TimelineUnitConversionError,
};

#[derive(Clone, Copy, Debug, Eq, Facet, Ord, PartialEq, PartialOrd)]
#[facet(opaque, proxy = TimeLikeFacetProxy)]
pub struct ArenaOffset<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    offset: TimelineOffset<Repr, Unit>,
}

impl<Repr, Unit> ArenaOffset<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    pub const ZERO: Self = Self::from_raw(Repr::ZERO);

    #[must_use]
    pub const fn from_raw(raw_value: Repr) -> Self {
        Self {
            offset: TimelineOffset::from_raw(raw_value),
        }
    }

    pub fn new<SourceUnit>(raw_value: Repr) -> Result<Self, TimelineUnitConversionError<Repr>>
    where
        SourceUnit: TimeUnit,
    {
        TimelineOffset::new::<SourceUnit>(raw_value).map(|offset| Self { offset })
    }

    #[must_use]
    pub const fn raw_value(self) -> Repr {
        self.offset.raw_value()
    }

    #[must_use]
    pub const fn as_timeline_offset(self) -> TimelineOffset<Repr, Unit> {
        self.offset
    }

    pub fn compose_with(
        self,
        arena_base_offset: TimelineOffset<Repr, Unit>,
    ) -> Result<TimelineOffset<Repr, Unit>, TimelineArithmeticError<Repr, Unit>> {
        arena_base_offset.checked_add(self.offset)
    }
}

#[cfg(test)]
mod tests {
    use facet::Facet;

    use super::ArenaOffset;
    use crate::timeline::{Femtoseconds, Seconds, TimeLikeFacetProxy, TimelineOffset};

    #[test]
    fn arena_offset_converts_from_source_units() {
        let value = ArenaOffset::<i128, Femtoseconds>::new::<Seconds>(2)
            .expect("seconds should convert exactly into femtoseconds");

        assert_eq!(value.raw_value(), 2_000_000_000_000_000);
    }

    #[test]
    fn arena_offset_composes_with_arena_base() {
        let base = TimelineOffset::<i128, Femtoseconds>::from_raw(15);
        let arena_offset = ArenaOffset::<i128, Femtoseconds>::from_raw(4);

        let composed = arena_offset
            .compose_with(base)
            .expect("composition should succeed");

        assert_eq!(composed, TimelineOffset::from_raw(19));
    }

    #[test]
    fn facet_proxy_uses_offset_category_for_arena_offsets() {
        let value = ArenaOffset::<i128, Seconds>::from_raw(3);
        let proxy = TimeLikeFacetProxy::try_from(&value)
            .expect("arena offset should convert into the time-like proxy");
        let round_trip = ArenaOffset::<i128, Seconds>::try_from(proxy)
            .expect("proxy should round-trip back into the same arena offset");

        assert_eq!(
            proxy.category,
            crate::timeline::facet_time_proxy::TIME_LIKE_CATEGORY_OFFSET
        );
        assert_eq!(proxy.femtoseconds, 3_000_000_000_000_000);
        assert_eq!(round_trip, value);
        assert!(ArenaOffset::<i128, Seconds>::SHAPE.proxy.is_some());
    }
}
