use facet::Facet;

use super::{ArenaOffset, TimeUnit, TimelineOffset, TimelineRepr};

pub(crate) const TIME_LIKE_CATEGORY_OFFSET: &str = "offset";
pub(crate) const TIME_LIKE_CATEGORY_INSTANT: &str = "instant";

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub(crate) struct TimeLikeFacetProxy {
    pub category: &'static str,
    pub femtoseconds: i128,
}

fn offset_to_femtoseconds<Repr, Unit>(
    value: TimelineOffset<Repr, Unit>,
) -> Result<i128, &'static str>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    value
        .raw_value()
        .to_i128()
        .checked_mul(Unit::FEMTOSECONDS_PER_UNIT)
        .ok_or("time-like facet proxy conversion overflowed")
}

impl<Repr, Unit> TryFrom<&TimelineOffset<Repr, Unit>> for TimeLikeFacetProxy
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    type Error = &'static str;

    fn try_from(value: &TimelineOffset<Repr, Unit>) -> Result<Self, Self::Error> {
        Ok(Self {
            category: TIME_LIKE_CATEGORY_OFFSET,
            femtoseconds: offset_to_femtoseconds(*value)?,
        })
    }
}

impl<Repr, Unit> TryFrom<TimeLikeFacetProxy> for TimelineOffset<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    type Error = &'static str;

    fn try_from(proxy: TimeLikeFacetProxy) -> Result<Self, Self::Error> {
        if proxy.category != TIME_LIKE_CATEGORY_OFFSET {
            return Err("time-like facet proxy category did not match an offset");
        }

        if proxy.femtoseconds % Unit::FEMTOSECONDS_PER_UNIT != 0 {
            return Err("time-like facet proxy could not represent the offset exactly");
        }

        let raw_value = proxy.femtoseconds / Unit::FEMTOSECONDS_PER_UNIT;
        let Some(raw_value) = Repr::try_from_i128(raw_value) else {
            return Err(
                "time-like facet proxy offset conversion overflowed the destination representation",
            );
        };

        Ok(Self::from_raw(raw_value))
    }
}

impl<Repr, Unit> TryFrom<&ArenaOffset<Repr, Unit>> for TimeLikeFacetProxy
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    type Error = &'static str;

    fn try_from(value: &ArenaOffset<Repr, Unit>) -> Result<Self, Self::Error> {
        Self::try_from(&value.as_timeline_offset())
    }
}

impl<Repr, Unit> TryFrom<TimeLikeFacetProxy> for ArenaOffset<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    type Error = &'static str;

    fn try_from(proxy: TimeLikeFacetProxy) -> Result<Self, Self::Error> {
        TimelineOffset::<Repr, Unit>::try_from(proxy)
            .map(|offset| Self::from_raw(offset.raw_value()))
    }
}
