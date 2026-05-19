use super::{TimeUnit, TimelineId, TimelineOrigin, TimelineRepr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimelineTransformFailureReason<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    UnrelatedOrigins {
        source_origin: TimelineOrigin<Repr, Unit>,
        destination_origin: TimelineOrigin<Repr, Unit>,
    },
    ArithmeticOverflow {
        source_origin: TimelineOrigin<Repr, Unit>,
        destination_origin: TimelineOrigin<Repr, Unit>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimelineTransformError<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    pub source_timeline_id: TimelineId,
    pub destination_timeline_id: TimelineId,
    pub reason: TimelineTransformFailureReason<Repr, Unit>,
}
