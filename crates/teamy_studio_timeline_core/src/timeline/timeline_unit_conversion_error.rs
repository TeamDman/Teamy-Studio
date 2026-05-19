use super::TimelineRepr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimelineUnitConversionFailureReason {
    InexactRepresentation,
    Overflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimelineUnitConversionError<Repr>
where
    Repr: TimelineRepr,
{
    pub source_unit_name: &'static str,
    pub target_unit_name: &'static str,
    pub raw_value: Repr,
    pub reason: TimelineUnitConversionFailureReason,
}
