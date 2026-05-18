pub trait TimelineRepr:
    Clone + Copy + Default + Eq + Ord + std::fmt::Debug + Send + Sync + 'static
{
    const ZERO: Self;

    fn to_i128(self) -> i128;

    fn try_from_i128(value: i128) -> Option<Self>;
}

macro_rules! impl_timeline_repr {
    ($repr:ty) => {
        impl TimelineRepr for $repr {
            const ZERO: Self = 0;

            fn to_i128(self) -> i128 {
                i128::from(self)
            }

            fn try_from_i128(value: i128) -> Option<Self> {
                <$repr>::try_from(value).ok()
            }
        }
    };
}

impl_timeline_repr!(i8);
impl_timeline_repr!(i16);
impl_timeline_repr!(i32);
impl_timeline_repr!(i64);
impl_timeline_repr!(i128);
