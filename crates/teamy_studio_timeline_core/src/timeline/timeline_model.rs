use super::{
    TimeUnit, TimelineId, TimelineOrigin, TimelineOffset, TimelineRepr, TimelineTransform,
    TimelineTransformError, TimelineTransformFailureReason,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timeline<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    id: TimelineId,
    origin: TimelineOrigin<Repr, Unit>,
}

impl<Repr, Unit> Timeline<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    #[must_use]
    pub fn new() -> Self {
        Self::with_origin(TimelineOrigin::Ungrounded)
    }

    #[must_use]
    pub fn with_origin(origin: TimelineOrigin<Repr, Unit>) -> Self {
        Self::with_id_and_origin(TimelineId::new(), origin)
    }

    #[must_use]
    pub const fn with_id(id: TimelineId) -> Self {
        Self::with_id_and_origin(id, TimelineOrigin::Ungrounded)
    }

    #[must_use]
    pub const fn with_id_and_origin(id: TimelineId, origin: TimelineOrigin<Repr, Unit>) -> Self {
        Self { id, origin }
    }

    #[must_use]
    pub const fn id(self) -> TimelineId {
        self.id
    }

    #[must_use]
    pub const fn origin(self) -> TimelineOrigin<Repr, Unit> {
        self.origin
    }

    pub fn transform_to(
        &self,
        destination: &Self,
    ) -> Result<TimelineTransform<Repr, Unit>, TimelineTransformError<Repr, Unit>> {
        let source_origin = self.origin;
        let destination_origin = destination.origin;

        if self.id == destination.id {
            return Ok(TimelineTransform::new(
                self.id,
                destination.id,
                TimelineOffset::ZERO,
                super::timeline_transform::ResolvedTimelineRelationship::SameTimeline,
            ));
        }

        if let TimelineOrigin::Relative { timeline_id, offset } = source_origin
            && timeline_id == destination.id
        {
            return Ok(TimelineTransform::new(
                self.id,
                destination.id,
                offset,
                super::timeline_transform::ResolvedTimelineRelationship::DirectRelative,
            ));
        }

        if let TimelineOrigin::Relative { timeline_id, offset } = destination_origin
            && timeline_id == self.id
        {
            let delta = offset.checked_neg().map_err(|_| TimelineTransformError {
                source_timeline_id: self.id,
                destination_timeline_id: destination.id,
                reason: TimelineTransformFailureReason::ArithmeticOverflow {
                    source_origin,
                    destination_origin,
                },
            })?;
            return Ok(TimelineTransform::new(
                self.id,
                destination.id,
                delta,
                super::timeline_transform::ResolvedTimelineRelationship::DirectRelative,
            ));
        }

        match (source_origin, destination_origin) {
            (
                TimelineOrigin::Grounded {
                    anchor_offset: source_anchor,
                },
                TimelineOrigin::Grounded {
                    anchor_offset: destination_anchor,
                },
            ) => {
                let delta = source_anchor
                    .checked_sub(destination_anchor)
                    .map_err(|_| TimelineTransformError {
                        source_timeline_id: self.id,
                        destination_timeline_id: destination.id,
                        reason: TimelineTransformFailureReason::ArithmeticOverflow {
                            source_origin,
                            destination_origin,
                        },
                    })?;
                Ok(TimelineTransform::new(
                    self.id,
                    destination.id,
                    delta,
                    super::timeline_transform::ResolvedTimelineRelationship::SharedGrounding,
                ))
            }
            (
                TimelineOrigin::Relative {
                    timeline_id: source_parent,
                    offset: source_offset,
                },
                TimelineOrigin::Relative {
                    timeline_id: destination_parent,
                    offset: destination_offset,
                },
            ) if source_parent == destination_parent => {
                let delta = source_offset
                    .checked_sub(destination_offset)
                    .map_err(|_| TimelineTransformError {
                        source_timeline_id: self.id,
                        destination_timeline_id: destination.id,
                        reason: TimelineTransformFailureReason::ArithmeticOverflow {
                            source_origin,
                            destination_origin,
                        },
                    })?;
                Ok(TimelineTransform::new(
                    self.id,
                    destination.id,
                    delta,
                    super::timeline_transform::ResolvedTimelineRelationship::SharedRelativeParent,
                ))
            }
            _ => Err(TimelineTransformError {
                source_timeline_id: self.id,
                destination_timeline_id: destination.id,
                reason: TimelineTransformFailureReason::UnrelatedOrigins {
                    source_origin,
                    destination_origin,
                },
            }),
        }
    }
}

impl<Repr, Unit> Default for Timeline<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Timeline;
    use crate::timeline::{Femtoseconds, TimelineId, TimelineOffset, TimelineOrigin};

    #[test]
    fn default_timeline_gets_a_fresh_id_and_ungrounded_origin() {
        let timeline = Timeline::<i128, Femtoseconds>::default();

        assert_ne!(timeline.id().as_bytes(), [0; 16]);
        assert_eq!(timeline.origin(), TimelineOrigin::Ungrounded);
    }

    #[test]
    fn explicit_timeline_creation_preserves_supplied_id_and_origin() {
        let id = TimelineId::from_bytes([0x44; 16]);
        let origin = TimelineOrigin::Grounded {
            anchor_offset: TimelineOffset::from_raw(5),
        };

        let timeline = Timeline::<i128, Femtoseconds>::with_id_and_origin(id, origin);

        assert_eq!(timeline.id(), id);
        assert_eq!(timeline.origin(), origin);
    }
}