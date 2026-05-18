use super::{TimeUnit, TimelineId, TimelineOffset, TimelineRepr};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TimelineOrigin<Repr, Unit>
where
    Repr: TimelineRepr,
    Unit: TimeUnit,
{
    Grounded {
        anchor_offset: TimelineOffset<Repr, Unit>,
    },
    Relative {
        timeline_id: TimelineId,
        offset: TimelineOffset<Repr, Unit>,
    },
    #[default]
    Ungrounded,
}

#[cfg(test)]
mod tests {
    use super::TimelineOrigin;
    use crate::timeline::{Femtoseconds, TimelineId, TimelineOffset};

    #[test]
    fn default_origin_is_ungrounded() {
        assert_eq!(
            TimelineOrigin::<i128, Femtoseconds>::default(),
            TimelineOrigin::Ungrounded
        );
    }

    #[test]
    fn relative_origin_keeps_target_timeline_identity() {
        let timeline_id = TimelineId::from_bytes([0x22; 16]);
        let origin = TimelineOrigin::<i128, Femtoseconds>::Relative {
            timeline_id,
            offset: TimelineOffset::from_raw(7),
        };

        assert_eq!(
            origin,
            TimelineOrigin::Relative {
                timeline_id,
                offset: TimelineOffset::from_raw(7),
            }
        );
    }
}