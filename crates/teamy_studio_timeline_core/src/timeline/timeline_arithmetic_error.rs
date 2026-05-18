use super::timeline_offset::TimelineOffset;
use super::{TimeUnit, TimelineRepr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimelineArithmeticOperation {
    Add,
    Sub,
    Neg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimelineArithmeticFailureReason {
    Overflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimelineArithmeticError<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    pub operation: TimelineArithmeticOperation,
    pub lhs: TimelineOffset<Repr, Unit>,
    pub rhs: Option<TimelineOffset<Repr, Unit>>,
    pub reason: TimelineArithmeticFailureReason,
}
