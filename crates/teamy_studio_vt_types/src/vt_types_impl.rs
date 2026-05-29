use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollViewport {
    Top,
    Bottom,
    Delta(isize),
}

pub mod key {
    use super::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Action {
        Press,
        Repeat,
        Release,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Key {
        A,
        B,
        C,
        D,
        E,
        F,
        G,
        H,
        I,
        J,
        K,
        L,
        M,
        N,
        O,
        P,
        Q,
        R,
        S,
        T,
        U,
        V,
        W,
        X,
        Y,
        Z,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Space,
        Minus,
        Equal,
        Period,
        Slash,
        Semicolon,
        Comma,
        Backquote,
        BracketLeft,
        Backslash,
        BracketRight,
        Quote,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        Backspace,
        Tab,
        Enter,
        Escape,
        PageUp,
        PageDown,
        End,
        Home,
        ArrowLeft,
        ArrowUp,
        ArrowRight,
        ArrowDown,
        Insert,
        Delete,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Mods(u16);

    impl Mods {
        pub const SHIFT: Self = Self(1 << 0);
        pub const SHIFT_SIDE: Self = Self(1 << 1);
        pub const CTRL: Self = Self(1 << 2);
        pub const CTRL_SIDE: Self = Self(1 << 3);
        pub const ALT: Self = Self(1 << 4);
        pub const ALT_SIDE: Self = Self(1 << 5);
        pub const SUPER: Self = Self(1 << 6);
        pub const SUPER_SIDE: Self = Self(1 << 7);
        pub const CAPS_LOCK: Self = Self(1 << 8);
        pub const NUM_LOCK: Self = Self(1 << 9);

        #[must_use]
        pub const fn empty() -> Self {
            Self(0)
        }

        #[must_use]
        pub const fn contains(self, other: Self) -> bool {
            (self.0 & other.0) == other.0
        }
    }

    impl BitOr for Mods {
        type Output = Self;

        fn bitor(self, rhs: Self) -> Self::Output {
            Self(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Mods {
        fn bitor_assign(&mut self, rhs: Self) {
            self.0 |= rhs.0;
        }
    }

    impl BitAnd for Mods {
        type Output = Self;

        fn bitand(self, rhs: Self) -> Self::Output {
            Self(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for Mods {
        fn bitand_assign(&mut self, rhs: Self) {
            self.0 &= rhs.0;
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct KittyKeyFlags(u8);

    impl KittyKeyFlags {
        pub const REPORT_ALL: Self = Self(1 << 0);
        pub const REPORT_ASSOCIATED: Self = Self(1 << 1);
        pub const REPORT_EVENTS: Self = Self(1 << 2);

        #[must_use]
        pub const fn empty() -> Self {
            Self(0)
        }

        #[must_use]
        pub const fn is_empty(self) -> bool {
            self.0 == 0
        }

        #[must_use]
        pub const fn contains(self, other: Self) -> bool {
            (self.0 & other.0) == other.0
        }

        #[must_use]
        pub const fn intersects(self, other: Self) -> bool {
            (self.0 & other.0) != 0
        }
    }

    impl BitOr for KittyKeyFlags {
        type Output = Self;

        fn bitor(self, rhs: Self) -> Self::Output {
            Self(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for KittyKeyFlags {
        fn bitor_assign(&mut self, rhs: Self) {
            self.0 |= rhs.0;
        }
    }

    impl BitAnd for KittyKeyFlags {
        type Output = Self;

        fn bitand(self, rhs: Self) -> Self::Output {
            Self(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for KittyKeyFlags {
        fn bitand_assign(&mut self, rhs: Self) {
            self.0 &= rhs.0;
        }
    }
}
