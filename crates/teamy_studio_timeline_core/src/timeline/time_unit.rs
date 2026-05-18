use facet::Facet;

#[must_use]
pub trait TimeUnit: Clone + Copy + Default + Eq + Ord + std::fmt::Debug + 'static {
    const NAME: &'static str;
    const FEMTOSECONDS_PER_UNIT: i128;
}

macro_rules! define_time_unit {
    ($name:ident, $display_name:literal, $femtoseconds_per_unit:expr) => {
        #[derive(Clone, Copy, Debug, Default, Eq, Facet, Ord, PartialEq, PartialOrd)]
        pub struct $name;

        impl TimeUnit for $name {
            const NAME: &'static str = $display_name;
            const FEMTOSECONDS_PER_UNIT: i128 = $femtoseconds_per_unit;
        }
    };
}

define_time_unit!(Femtoseconds, "femtoseconds", 1);
define_time_unit!(Nanoseconds, "nanoseconds", 1_000_000);
define_time_unit!(Microseconds, "microseconds", 1_000_000_000);
define_time_unit!(Milliseconds, "milliseconds", 1_000_000_000_000);
define_time_unit!(Seconds, "seconds", 1_000_000_000_000_000);
