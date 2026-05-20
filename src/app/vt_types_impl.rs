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

#[cfg(feature = "ghostty")]
impl From<key::Action> for libghostty_vt::key::Action {
    fn from(value: key::Action) -> Self {
        match value {
            key::Action::Press => Self::Press,
            key::Action::Repeat => Self::Repeat,
            key::Action::Release => Self::Release,
        }
    }
}

#[cfg(feature = "ghostty")]
impl From<key::Key> for libghostty_vt::key::Key {
    fn from(value: key::Key) -> Self {
        match value {
            key::Key::A => Self::A,
            key::Key::B => Self::B,
            key::Key::C => Self::C,
            key::Key::D => Self::D,
            key::Key::E => Self::E,
            key::Key::F => Self::F,
            key::Key::G => Self::G,
            key::Key::H => Self::H,
            key::Key::I => Self::I,
            key::Key::J => Self::J,
            key::Key::K => Self::K,
            key::Key::L => Self::L,
            key::Key::M => Self::M,
            key::Key::N => Self::N,
            key::Key::O => Self::O,
            key::Key::P => Self::P,
            key::Key::Q => Self::Q,
            key::Key::R => Self::R,
            key::Key::S => Self::S,
            key::Key::T => Self::T,
            key::Key::U => Self::U,
            key::Key::V => Self::V,
            key::Key::W => Self::W,
            key::Key::X => Self::X,
            key::Key::Y => Self::Y,
            key::Key::Z => Self::Z,
            key::Key::Digit0 => Self::Digit0,
            key::Key::Digit1 => Self::Digit1,
            key::Key::Digit2 => Self::Digit2,
            key::Key::Digit3 => Self::Digit3,
            key::Key::Digit4 => Self::Digit4,
            key::Key::Digit5 => Self::Digit5,
            key::Key::Digit6 => Self::Digit6,
            key::Key::Digit7 => Self::Digit7,
            key::Key::Digit8 => Self::Digit8,
            key::Key::Digit9 => Self::Digit9,
            key::Key::Space => Self::Space,
            key::Key::Minus => Self::Minus,
            key::Key::Equal => Self::Equal,
            key::Key::Period => Self::Period,
            key::Key::Slash => Self::Slash,
            key::Key::Semicolon => Self::Semicolon,
            key::Key::Comma => Self::Comma,
            key::Key::Backquote => Self::Backquote,
            key::Key::BracketLeft => Self::BracketLeft,
            key::Key::Backslash => Self::Backslash,
            key::Key::BracketRight => Self::BracketRight,
            key::Key::Quote => Self::Quote,
            key::Key::ShiftLeft => Self::ShiftLeft,
            key::Key::ShiftRight => Self::ShiftRight,
            key::Key::ControlLeft => Self::ControlLeft,
            key::Key::ControlRight => Self::ControlRight,
            key::Key::AltLeft => Self::AltLeft,
            key::Key::AltRight => Self::AltRight,
            key::Key::Backspace => Self::Backspace,
            key::Key::Tab => Self::Tab,
            key::Key::Enter => Self::Enter,
            key::Key::Escape => Self::Escape,
            key::Key::PageUp => Self::PageUp,
            key::Key::PageDown => Self::PageDown,
            key::Key::End => Self::End,
            key::Key::Home => Self::Home,
            key::Key::ArrowLeft => Self::ArrowLeft,
            key::Key::ArrowUp => Self::ArrowUp,
            key::Key::ArrowRight => Self::ArrowRight,
            key::Key::ArrowDown => Self::ArrowDown,
            key::Key::Insert => Self::Insert,
            key::Key::Delete => Self::Delete,
        }
    }
}

#[cfg(feature = "ghostty")]
impl From<key::Mods> for libghostty_vt::key::Mods {
    fn from(value: key::Mods) -> Self {
        let mut mods = Self::empty();
        if value.contains(key::Mods::SHIFT) {
            mods |= Self::SHIFT;
        }
        if value.contains(key::Mods::SHIFT_SIDE) {
            mods |= Self::SHIFT_SIDE;
        }
        if value.contains(key::Mods::CTRL) {
            mods |= Self::CTRL;
        }
        if value.contains(key::Mods::CTRL_SIDE) {
            mods |= Self::CTRL_SIDE;
        }
        if value.contains(key::Mods::ALT) {
            mods |= Self::ALT;
        }
        if value.contains(key::Mods::ALT_SIDE) {
            mods |= Self::ALT_SIDE;
        }
        if value.contains(key::Mods::SUPER) {
            mods |= Self::SUPER;
        }
        if value.contains(key::Mods::SUPER_SIDE) {
            mods |= Self::SUPER_SIDE;
        }
        if value.contains(key::Mods::CAPS_LOCK) {
            mods |= Self::CAPS_LOCK;
        }
        if value.contains(key::Mods::NUM_LOCK) {
            mods |= Self::NUM_LOCK;
        }
        mods
    }
}

#[cfg(feature = "ghostty")]
impl From<key::KittyKeyFlags> for libghostty_vt::key::KittyKeyFlags {
    fn from(value: key::KittyKeyFlags) -> Self {
        let mut flags = Self::empty();
        if value.contains(key::KittyKeyFlags::REPORT_ALL) {
            flags |= Self::REPORT_ALL;
        }
        if value.contains(key::KittyKeyFlags::REPORT_ASSOCIATED) {
            flags |= Self::REPORT_ASSOCIATED;
        }
        if value.contains(key::KittyKeyFlags::REPORT_EVENTS) {
            flags |= Self::REPORT_EVENTS;
        }
        flags
    }
}

#[cfg(feature = "ghostty")]
impl From<libghostty_vt::key::KittyKeyFlags> for key::KittyKeyFlags {
    fn from(value: libghostty_vt::key::KittyKeyFlags) -> Self {
        let mut flags = Self::empty();
        if value.contains(libghostty_vt::key::KittyKeyFlags::REPORT_ALL) {
            flags |= Self::REPORT_ALL;
        }
        if value.contains(libghostty_vt::key::KittyKeyFlags::REPORT_ASSOCIATED) {
            flags |= Self::REPORT_ASSOCIATED;
        }
        if value.contains(libghostty_vt::key::KittyKeyFlags::REPORT_EVENTS) {
            flags |= Self::REPORT_EVENTS;
        }
        flags
    }
}

#[cfg(feature = "ghostty")]
impl From<ScrollViewport> for libghostty_vt::terminal::ScrollViewport {
    fn from(value: ScrollViewport) -> Self {
        match value {
            ScrollViewport::Top => Self::Top,
            ScrollViewport::Bottom => Self::Bottom,
            ScrollViewport::Delta(delta) => Self::Delta(delta),
        }
    }
}
