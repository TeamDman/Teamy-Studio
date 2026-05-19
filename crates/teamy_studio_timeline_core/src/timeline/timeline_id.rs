use facet::Facet;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, Ord, PartialEq, PartialOrd)]
pub struct TimelineId(Uuid);

impl TimelineId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0.into_bytes()
    }

    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TimelineId {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use facet::Facet;

    use super::TimelineId;

    #[test]
    fn timeline_id_round_trips_bytes() {
        let value = TimelineId::from_bytes([0x11; 16]);

        assert_eq!(value.as_bytes(), [0x11; 16]);
    }

    #[test]
    fn default_generates_nonzero_id() {
        let value = TimelineId::default();

        assert_ne!(value.as_bytes(), [0; 16]);
    }

    #[test]
    fn timeline_id_implements_facet() {
        fn assert_facet<T>()
        where
            T: for<'facet> Facet<'facet>,
        {
        }

        assert_facet::<TimelineId>();
    }
}
