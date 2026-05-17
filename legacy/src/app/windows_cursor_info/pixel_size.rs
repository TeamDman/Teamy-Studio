#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum PixelSize {
    HalfHeight,
    #[default]
    Full,
}

impl PixelSize {
    #[must_use]
    pub const fn pixels_per_cell(self) -> (u16, u16) {
        match self {
            Self::HalfHeight => (1, 2),
            Self::Full => (1, 1),
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HalfHeight => "half-height",
            Self::Full => "full",
        }
    }
}
