use super::{TimeUnit, TimelineId, TimelineOffset, TimelineRepr};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelineTransform<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    source_timeline_id: TimelineId,
    destination_timeline_id: TimelineId,
    offset_delta: TimelineOffset<Repr, Unit>,
    relationship: ResolvedTimelineRelationship,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResolvedTimelineRelationship {
    SameTimeline,
    DirectRelative,
    SharedRelativeParent,
    SharedGrounding,
}

impl<Repr, Unit> TimelineTransform<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    pub(crate) const fn new(
        source_timeline_id: TimelineId,
        destination_timeline_id: TimelineId,
        offset_delta: TimelineOffset<Repr, Unit>,
        relationship: ResolvedTimelineRelationship,
    ) -> Self {
        Self {
            source_timeline_id,
            destination_timeline_id,
            offset_delta,
            relationship,
        }
    }

    #[must_use]
    pub const fn source_timeline_id(&self) -> TimelineId {
        self.source_timeline_id
    }

    #[must_use]
    pub const fn destination_timeline_id(&self) -> TimelineId {
        self.destination_timeline_id
    }

    #[must_use]
    pub fn apply(&self, offset: TimelineOffset<Repr, Unit>) -> TimelineOffset<Repr, Unit> {
        match self.relationship {
            ResolvedTimelineRelationship::SameTimeline
            | ResolvedTimelineRelationship::DirectRelative
            | ResolvedTimelineRelationship::SharedRelativeParent
            | ResolvedTimelineRelationship::SharedGrounding => offset
                .checked_add(self.offset_delta)
                .expect("validated timeline transform overflowed while applying an offset"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::timeline::{
        Femtoseconds, Timeline, TimelineId, TimelineOffset, TimelineOrigin,
        TimelineTransformFailureReason,
    };

    #[test]
    fn same_timeline_transform_is_identity() {
        let timeline = Timeline::<i128, Femtoseconds>::with_id(TimelineId::from_bytes([1; 16]));
        let transform = timeline
            .transform_to(&timeline)
            .expect("same timeline should be relatable");

        assert_eq!(transform.source_timeline_id(), timeline.id());
        assert_eq!(transform.destination_timeline_id(), timeline.id());
        assert_eq!(
            transform.apply(TimelineOffset::from_raw(9)),
            TimelineOffset::from_raw(9)
        );
    }

    #[test]
    fn grounded_timelines_translate_by_anchor_delta() {
        let source = Timeline::<i128, Femtoseconds>::with_id_and_origin(
            TimelineId::from_bytes([2; 16]),
            TimelineOrigin::Grounded {
                anchor_offset: TimelineOffset::from_raw(10),
            },
        );
        let destination = Timeline::<i128, Femtoseconds>::with_id_and_origin(
            TimelineId::from_bytes([3; 16]),
            TimelineOrigin::Grounded {
                anchor_offset: TimelineOffset::from_raw(4),
            },
        );

        let transform = source
            .transform_to(&destination)
            .expect("grounded timelines should be relatable");

        assert_eq!(transform.apply(TimelineOffset::from_raw(3)), TimelineOffset::from_raw(9));
    }

    #[test]
    fn direct_relative_transform_uses_declared_parent_offset() {
        let destination = Timeline::<i128, Femtoseconds>::with_id(TimelineId::from_bytes([4; 16]));
        let source = Timeline::<i128, Femtoseconds>::with_id_and_origin(
            TimelineId::from_bytes([5; 16]),
            TimelineOrigin::Relative {
                timeline_id: destination.id(),
                offset: TimelineOffset::from_raw(5),
            },
        );

        let transform = source
            .transform_to(&destination)
            .expect("direct relative timelines should be relatable");

        assert_eq!(transform.apply(TimelineOffset::from_raw(7)), TimelineOffset::from_raw(12));
    }

    #[test]
    fn sibling_relative_timelines_translate_via_shared_parent_offset_delta() {
        let parent_id = TimelineId::from_bytes([6; 16]);
        let source = Timeline::<i128, Femtoseconds>::with_id_and_origin(
            TimelineId::from_bytes([7; 16]),
            TimelineOrigin::Relative {
                timeline_id: parent_id,
                offset: TimelineOffset::from_raw(11),
            },
        );
        let destination = Timeline::<i128, Femtoseconds>::with_id_and_origin(
            TimelineId::from_bytes([8; 16]),
            TimelineOrigin::Relative {
                timeline_id: parent_id,
                offset: TimelineOffset::from_raw(3),
            },
        );

        let transform = source
            .transform_to(&destination)
            .expect("siblings under the same parent should be relatable");

        assert_eq!(transform.apply(TimelineOffset::from_raw(2)), TimelineOffset::from_raw(10));
    }

    #[test]
    fn transform_round_trips_for_relations_we_accept_in_v1() {
        let relatable_pairs = [
            (
                Timeline::<i128, Femtoseconds>::with_id(TimelineId::from_bytes([9; 16])),
                Timeline::<i128, Femtoseconds>::with_id(TimelineId::from_bytes([9; 16])),
            ),
            (
                Timeline::<i128, Femtoseconds>::with_id_and_origin(
                    TimelineId::from_bytes([10; 16]),
                    TimelineOrigin::Grounded {
                        anchor_offset: TimelineOffset::from_raw(12),
                    },
                ),
                Timeline::<i128, Femtoseconds>::with_id_and_origin(
                    TimelineId::from_bytes([11; 16]),
                    TimelineOrigin::Grounded {
                        anchor_offset: TimelineOffset::from_raw(-7),
                    },
                ),
            ),
            (
                Timeline::<i128, Femtoseconds>::with_id_and_origin(
                    TimelineId::from_bytes([12; 16]),
                    TimelineOrigin::Relative {
                        timeline_id: TimelineId::from_bytes([13; 16]),
                        offset: TimelineOffset::from_raw(8),
                    },
                ),
                Timeline::<i128, Femtoseconds>::with_id(TimelineId::from_bytes([13; 16])),
            ),
        ];

        for (source, destination) in relatable_pairs {
            let forward = source
                .transform_to(&destination)
                .expect("pair should be relatable");
            let reverse = destination
                .transform_to(&source)
                .expect("pair should be relatable in reverse");

            for raw_value in [-17_i128, -1, 0, 1, 42] {
                let start = TimelineOffset::from_raw(raw_value);
                let round_trip = reverse.apply(forward.apply(start));

                assert_eq!(round_trip, start);
            }
        }
    }

    #[test]
    fn unrelated_timelines_fail_with_structured_reason() {
        let source = Timeline::<i128, Femtoseconds>::with_id(TimelineId::from_bytes([14; 16]));
        let destination = Timeline::<i128, Femtoseconds>::with_id(TimelineId::from_bytes([15; 16]));

        let error = source
            .transform_to(&destination)
            .expect_err("distinct ungrounded timelines should not be relatable");

        assert_eq!(error.source_timeline_id, source.id());
        assert_eq!(error.destination_timeline_id, destination.id());
        assert_eq!(
            error.reason,
            TimelineTransformFailureReason::UnrelatedOrigins {
                source_origin: TimelineOrigin::Ungrounded,
                destination_origin: TimelineOrigin::Ungrounded,
            }
        );
    }

    #[test]
    fn different_relative_parents_are_not_relatable() {
        let source = Timeline::<i128, Femtoseconds>::with_id_and_origin(
            TimelineId::from_bytes([16; 16]),
            TimelineOrigin::Relative {
                timeline_id: TimelineId::from_bytes([17; 16]),
                offset: TimelineOffset::from_raw(5),
            },
        );
        let destination = Timeline::<i128, Femtoseconds>::with_id_and_origin(
            TimelineId::from_bytes([18; 16]),
            TimelineOrigin::Relative {
                timeline_id: TimelineId::from_bytes([19; 16]),
                offset: TimelineOffset::from_raw(5),
            },
        );

        let error = source
            .transform_to(&destination)
            .expect_err("different relative parents should not be relatable");

        assert_eq!(
            error.reason,
            TimelineTransformFailureReason::UnrelatedOrigins {
                source_origin: source.origin(),
                destination_origin: destination.origin(),
            }
        );
    }
}