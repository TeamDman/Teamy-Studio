use std::marker::PhantomData;

use facet::Facet;

use super::{
    TimeLikeFacetProxy, TimeUnit, TimelineArithmeticError, TimelineArithmeticFailureReason,
    TimelineArithmeticOperation, TimelineRepr, TimelineUnitConversionError,
    TimelineUnitConversionFailureReason, TimelineUnitExtractionError,
    TimelineUnitExtractionFailureReason,
};

#[derive(Clone, Copy, Debug, Eq, Facet, Ord, PartialEq, PartialOrd)]
#[facet(opaque, proxy = TimeLikeFacetProxy)]
pub struct TimelineOffset<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    raw_value: Repr,
    unit_marker: PhantomData<Unit>,
}

impl<Repr, Unit> TimelineOffset<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    pub const ZERO: Self = Self::from_raw(Repr::ZERO);

    #[must_use]
    pub const fn from_raw(raw_value: Repr) -> Self {
        Self {
            raw_value,
            unit_marker: PhantomData,
        }
    }

    pub fn new<SourceUnit>(raw_value: Repr) -> Result<Self, TimelineUnitConversionError<Repr>>
    where
        SourceUnit: TimeUnit,
    {
        let Some(canonical_value) = raw_value
            .to_i128()
            .checked_mul(SourceUnit::FEMTOSECONDS_PER_UNIT)
        else {
            return Err(TimelineUnitConversionError {
                source_unit_name: SourceUnit::NAME,
                target_unit_name: Unit::NAME,
                raw_value,
                reason: TimelineUnitConversionFailureReason::Overflow,
            });
        };

        if canonical_value % Unit::FEMTOSECONDS_PER_UNIT != 0 {
            return Err(TimelineUnitConversionError {
                source_unit_name: SourceUnit::NAME,
                target_unit_name: Unit::NAME,
                raw_value,
                reason: TimelineUnitConversionFailureReason::InexactRepresentation,
            });
        }

        let target_value = canonical_value / Unit::FEMTOSECONDS_PER_UNIT;
        let Some(raw_value) = Repr::try_from_i128(target_value) else {
            return Err(TimelineUnitConversionError {
                source_unit_name: SourceUnit::NAME,
                target_unit_name: Unit::NAME,
                raw_value,
                reason: TimelineUnitConversionFailureReason::Overflow,
            });
        };

        Ok(Self::from_raw(raw_value))
    }

    #[must_use]
    pub const fn raw_value(self) -> Repr {
        self.raw_value
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self, TimelineArithmeticError<Repr, Unit>> {
        self.checked_binary_op(rhs, TimelineArithmeticOperation::Add, i128::checked_add)
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self, TimelineArithmeticError<Repr, Unit>> {
        self.checked_binary_op(rhs, TimelineArithmeticOperation::Sub, i128::checked_sub)
    }

    pub fn checked_neg(self) -> Result<Self, TimelineArithmeticError<Repr, Unit>> {
        let Some(value) = self.raw_value.to_i128().checked_neg() else {
            return Err(TimelineArithmeticError {
                operation: TimelineArithmeticOperation::Neg,
                lhs: self,
                rhs: None,
                reason: TimelineArithmeticFailureReason::Overflow,
            });
        };

        let Some(raw_value) = Repr::try_from_i128(value) else {
            return Err(TimelineArithmeticError {
                operation: TimelineArithmeticOperation::Neg,
                lhs: self,
                rhs: None,
                reason: TimelineArithmeticFailureReason::Overflow,
            });
        };

        Ok(Self::from_raw(raw_value))
    }

    pub fn get<TargetUnit>(self) -> Result<Repr, TimelineUnitExtractionError<Repr>>
    where
        TargetUnit: TimeUnit,
    {
        let Some(canonical_value) = self
            .raw_value
            .to_i128()
            .checked_mul(Unit::FEMTOSECONDS_PER_UNIT)
        else {
            return Err(TimelineUnitExtractionError {
                source_unit_name: Unit::NAME,
                target_unit_name: TargetUnit::NAME,
                raw_value: self.raw_value,
                reason: TimelineUnitExtractionFailureReason::Overflow,
            });
        };

        if canonical_value % TargetUnit::FEMTOSECONDS_PER_UNIT != 0 {
            return Err(TimelineUnitExtractionError {
                source_unit_name: Unit::NAME,
                target_unit_name: TargetUnit::NAME,
                raw_value: self.raw_value,
                reason: TimelineUnitExtractionFailureReason::InexactRepresentation,
            });
        }

        let target_value = canonical_value / TargetUnit::FEMTOSECONDS_PER_UNIT;
        let Some(raw_value) = Repr::try_from_i128(target_value) else {
            return Err(TimelineUnitExtractionError {
                source_unit_name: Unit::NAME,
                target_unit_name: TargetUnit::NAME,
                raw_value: self.raw_value,
                reason: TimelineUnitExtractionFailureReason::Overflow,
            });
        };

        Ok(raw_value)
    }

    #[must_use]
    pub fn is_zero(self) -> bool {
        self.raw_value.to_i128() == 0
    }

    #[must_use]
    pub fn is_positive(self) -> bool {
        self.raw_value.to_i128() > 0
    }

    #[must_use]
    pub fn is_negative(self) -> bool {
        self.raw_value.to_i128() < 0
    }

    fn checked_binary_op(
        self,
        rhs: Self,
        operation: TimelineArithmeticOperation,
        operation_impl: fn(i128, i128) -> Option<i128>,
    ) -> Result<Self, TimelineArithmeticError<Repr, Unit>> {
        let Some(value) = operation_impl(self.raw_value.to_i128(), rhs.raw_value.to_i128()) else {
            return Err(TimelineArithmeticError {
                operation,
                lhs: self,
                rhs: Some(rhs),
                reason: TimelineArithmeticFailureReason::Overflow,
            });
        };

        let Some(raw_value) = Repr::try_from_i128(value) else {
            return Err(TimelineArithmeticError {
                operation,
                lhs: self,
                rhs: Some(rhs),
                reason: TimelineArithmeticFailureReason::Overflow,
            });
        };

        Ok(Self::from_raw(raw_value))
    }
}

#[cfg(test)]
mod tests {
    use facet::Facet;

    use super::TimelineOffset;
    use crate::timeline::{
        Femtoseconds, Nanoseconds, Seconds, TIME_LIKE_CATEGORY_INSTANT, TimeLikeFacetProxy,
        TimelineArithmeticFailureReason,
        TimelineArithmeticOperation, TimelineUnitConversionFailureReason,
        TimelineUnitExtractionFailureReason,
    };

    #[test]
    fn checked_add_uses_the_stored_unit() {
        let lhs = TimelineOffset::<i128, Femtoseconds>::new::<Femtoseconds>(7)
            .expect("construction should succeed");
        let rhs = TimelineOffset::<i128, Femtoseconds>::new::<Femtoseconds>(5)
            .expect("construction should succeed");

        let sum = lhs.checked_add(rhs).expect("addition should succeed");

        assert_eq!(sum.raw_value(), 12);
    }

    #[test]
    fn checked_add_reports_overflow_with_operands() {
        let lhs = TimelineOffset::<i8, Femtoseconds>::new::<Femtoseconds>(i8::MAX)
            .expect("construction should succeed");
        let rhs = TimelineOffset::<i8, Femtoseconds>::new::<Femtoseconds>(1)
            .expect("construction should succeed");

        let error = lhs.checked_add(rhs).expect_err("addition should overflow");

        assert_eq!(error.operation, TimelineArithmeticOperation::Add);
        assert_eq!(error.lhs, lhs);
        assert_eq!(error.rhs, Some(rhs));
        assert_eq!(error.reason, TimelineArithmeticFailureReason::Overflow);
    }

    #[test]
    fn checked_neg_reports_overflow_for_min_value() {
        let value = TimelineOffset::<i8, Femtoseconds>::new::<Femtoseconds>(i8::MIN)
            .expect("construction should succeed");

        let error = value.checked_neg().expect_err("negation should overflow");

        assert_eq!(error.operation, TimelineArithmeticOperation::Neg);
        assert_eq!(error.lhs, value);
        assert_eq!(error.rhs, None);
        assert_eq!(error.reason, TimelineArithmeticFailureReason::Overflow);
    }

    #[test]
    fn get_exactly_converts_between_units() {
        let seconds = TimelineOffset::<i128, Seconds>::new::<Seconds>(2)
            .expect("construction should succeed");

        let nanos = seconds
            .get::<Nanoseconds>()
            .expect("seconds should convert exactly to nanoseconds");

        assert_eq!(nanos, 2_000_000_000);
    }

    #[test]
    fn get_rejects_inexact_conversions() {
        let femtoseconds = TimelineOffset::<i128, Femtoseconds>::new::<Femtoseconds>(1)
            .expect("construction should succeed");

        let error = femtoseconds
            .get::<Seconds>()
            .expect_err("femtoseconds should not convert exactly to seconds");

        assert_eq!(error.source_unit_name, "femtoseconds");
        assert_eq!(error.target_unit_name, "seconds");
        assert_eq!(error.raw_value, 1);
        assert_eq!(
            error.reason,
            TimelineUnitExtractionFailureReason::InexactRepresentation
        );
    }

    #[test]
    fn zero_and_sign_helpers_follow_the_raw_value() {
        let zero = TimelineOffset::<i128, Femtoseconds>::ZERO;
        let positive = TimelineOffset::<i128, Femtoseconds>::new::<Femtoseconds>(4)
            .expect("construction should succeed");
        let negative = TimelineOffset::<i128, Femtoseconds>::new::<Femtoseconds>(-4)
            .expect("construction should succeed");

        assert!(zero.is_zero());
        assert!(positive.is_positive());
        assert!(negative.is_negative());
    }

    #[test]
    fn new_converts_source_units_into_storage_units() {
        let femtoseconds = TimelineOffset::<i128, Femtoseconds>::new::<Seconds>(2)
            .expect("seconds should convert exactly into femtoseconds");

        assert_eq!(femtoseconds.raw_value(), 2_000_000_000_000_000);
    }

    #[test]
    fn new_reports_inexact_conversion_when_storage_unit_is_coarser() {
        let error = TimelineOffset::<i128, Seconds>::new::<Femtoseconds>(1)
            .expect_err("femtoseconds should not convert exactly into seconds");

        assert_eq!(error.source_unit_name, "femtoseconds");
        assert_eq!(error.target_unit_name, "seconds");
        assert_eq!(error.raw_value, 1);
        assert_eq!(
            error.reason,
            TimelineUnitConversionFailureReason::InexactRepresentation
        );
    }

    #[test]
    fn facet_proxy_uses_canonical_femtoseconds_for_offsets() {
        let value = TimelineOffset::<i128, Seconds>::from_raw(2);
        let proxy = TimeLikeFacetProxy::try_from(&value)
            .expect("offset should convert into the time-like proxy");
        let round_trip = TimelineOffset::<i128, Seconds>::try_from(proxy)
            .expect("proxy should round-trip back into the same offset");

        assert_eq!(
            proxy.category,
            crate::timeline::facet_time_proxy::TIME_LIKE_CATEGORY_OFFSET
        );
        assert_eq!(proxy.femtoseconds, 2_000_000_000_000_000);
        assert_eq!(round_trip, value);
        assert!(TimelineOffset::<i128, Seconds>::SHAPE.proxy.is_some());
    }

    #[test]
    fn facet_proxy_rejects_non_offset_categories_for_offsets() {
        let error = TimelineOffset::<i128, Femtoseconds>::try_from(TimeLikeFacetProxy {
            category: TIME_LIKE_CATEGORY_INSTANT,
            femtoseconds: 7,
        })
        .expect_err("instant proxy should not deserialize into an offset");

        assert_eq!(error, "time-like facet proxy category did not match an offset");
    }
}
