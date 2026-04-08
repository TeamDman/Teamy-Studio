use std::marker::PhantomData;

use eyre::Context;
use sguaba::{Coordinate, CoordinateSystem, math::RigidBodyTransform, systems::RightHandedXyzLike};
use uom::si::f64::Length;
use uom::si::length::meter;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
};

sguaba::system!(pub(crate) struct ScreenSpace using right-handed XYZ);
sguaba::system!(pub(crate) struct ClientSpace using right-handed XYZ);

#[derive(Debug, PartialEq)]
pub(crate) struct Point<Space> {
    coordinate: Coordinate<Space>,
}

impl<Space> Clone for Point<Space> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Space> Copy for Point<Space> {}

impl<Space> Point<Space>
where
    Space: CoordinateSystem<Convention = RightHandedXyzLike>,
{
    #[must_use]
    pub(crate) fn new(x: i32, y: i32) -> Self {
        Self {
            coordinate: Coordinate::<Space>::builder()
                .x(pixel_length(x))
                .y(pixel_length(y))
                .z(pixel_length(0))
                .build(),
        }
    }

    #[must_use]
    pub(crate) fn from_lparam(lparam: LPARAM) -> Self {
        Self::new(signed_word_i32(lparam.0), signed_word_i32(lparam.0 >> 16))
    }

    #[must_use]
    pub(crate) fn from_win32_point(point: POINT) -> Self {
        Self::new(point.x, point.y)
    }

    pub(crate) fn to_win32_point(self) -> eyre::Result<POINT> {
        Ok(POINT {
            x: integral_pixel(self.x()).wrap_err("failed to convert x pixel coordinate")?,
            y: integral_pixel(self.y()).wrap_err("failed to convert y pixel coordinate")?,
        })
    }

    #[must_use]
    pub(crate) fn x(&self) -> Length {
        self.coordinate.x()
    }

    #[must_use]
    pub(crate) fn y(&self) -> Length {
        self.coordinate.y()
    }

    #[must_use]
    pub(crate) fn coordinate(self) -> Coordinate<Space> {
        self.coordinate
    }
}

pub(crate) type ScreenPoint = Point<ScreenSpace>;
pub(crate) type ClientPoint = Point<ClientSpace>;

impl ScreenPoint {
    pub(crate) fn pack_lparam(self) -> eyre::Result<isize> {
        let point = self.to_win32_point()?;
        let x = u16::from_le_bytes(
            i16::try_from(point.x)
                .expect("screen x coordinate must fit in signed 16-bit LPARAM packing")
                .to_le_bytes(),
        );
        let y = u16::from_le_bytes(
            i16::try_from(point.y)
                .expect("screen y coordinate must fit in signed 16-bit LPARAM packing")
                .to_le_bytes(),
        );
        Ok(isize::try_from((u32::from(y) << 16) | u32::from(x))
            .expect("packed LPARAM must fit in isize"))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Rect<Space> {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    _space: PhantomData<Space>,
}

impl<Space> Clone for Rect<Space> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Space> Copy for Rect<Space> {}

impl<Space> Rect<Space> {
    #[must_use]
    pub(crate) fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
            _space: PhantomData,
        }
    }

    #[must_use]
    pub(crate) fn from_win32_rect(rect: RECT) -> Self {
        Self::new(rect.left, rect.top, rect.right, rect.bottom)
    }

    #[must_use]
    pub(crate) fn to_win32_rect(self) -> RECT {
        RECT {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }

    #[must_use]
    pub(crate) fn left(self) -> i32 {
        self.left
    }

    #[must_use]
    pub(crate) fn top(self) -> i32 {
        self.top
    }

    #[must_use]
    pub(crate) fn right(self) -> i32 {
        self.right
    }

    #[must_use]
    pub(crate) fn bottom(self) -> i32 {
        self.bottom
    }

    #[must_use]
    pub(crate) fn width(self) -> i32 {
        self.right - self.left
    }

    #[must_use]
    pub(crate) fn height(self) -> i32 {
        self.bottom - self.top
    }

    #[must_use]
    pub(crate) fn inset(self, amount: i32) -> Self {
        Self::new(
            self.left + amount,
            self.top + amount,
            (self.right - amount).max(self.left + amount),
            (self.bottom - amount).max(self.top + amount),
        )
    }
}

impl<Space> Rect<Space>
where
    Space: CoordinateSystem<Convention = RightHandedXyzLike>,
{
    #[must_use]
    pub(crate) fn contains(self, point: Point<Space>) -> bool {
        point.x() >= pixel_length(self.left)
            && point.x() < pixel_length(self.right)
            && point.y() >= pixel_length(self.top)
            && point.y() < pixel_length(self.bottom)
    }
}

pub(crate) type ScreenRect = Rect<ScreenSpace>;
pub(crate) type ClientRect = Rect<ClientSpace>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCellPoint {
    column: i32,
    row: i32,
}

impl TerminalCellPoint {
    #[must_use]
    pub(crate) fn new(column: i32, row: i32) -> Self {
        Self { column, row }
    }

    #[must_use]
    pub(crate) fn column(self) -> i32 {
        self.column
    }

    #[must_use]
    pub(crate) fn row(self) -> i32 {
        self.row
    }

    #[must_use]
    pub(crate) fn to_client_rect(
        self,
        terminal_rect: ClientRect,
        cell_width: i32,
        cell_height: i32,
    ) -> ClientRect {
        let left = terminal_rect.left() + (self.column() * cell_width);
        let top = terminal_rect.top() + (self.row() * cell_height);
        ClientRect::new(left, top, left + cell_width, top + cell_height)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScreenToClientTransform {
    inner: RigidBodyTransform<ScreenSpace, ClientSpace>,
}

impl ScreenToClientTransform {
    #[must_use]
    pub(crate) fn for_window(window_rect: ScreenRect) -> Self {
        let window_origin = ScreenPoint::new(window_rect.left(), window_rect.top());
        Self {
            // SAFETY: the window's top-left screen-space point defines the client-space origin.
            inner: unsafe { window_origin.coordinate().map_as_zero_in::<ClientSpace>() },
        }
    }

    #[must_use]
    pub(crate) fn screen_to_client(self, point: ScreenPoint) -> ClientPoint {
        ClientPoint {
            coordinate: self.inner.transform(point.coordinate()),
        }
    }

    #[must_use]
    pub(crate) fn client_to_screen(self, point: ClientPoint) -> ScreenPoint {
        ScreenPoint {
            coordinate: self.inner.inverse_transform(point.coordinate()),
        }
    }
}

#[must_use]
pub(crate) fn drag_threshold_exceeded(
    origin: ClientPoint,
    current: ClientPoint,
    threshold_x: i32,
    threshold_y: i32,
) -> bool {
    if threshold_x <= 0 || threshold_y <= 0 {
        return true;
    }

    let delta = current.coordinate() - origin.coordinate();
    delta.x().get::<meter>().abs() >= f64::from(threshold_x)
        || delta.y().get::<meter>().abs() >= f64::from(threshold_y)
}

#[must_use]
pub(crate) fn classify_resize_border_hit(
    client_rect: ClientRect,
    point: ClientPoint,
    resize_border_x: i32,
    resize_border_y: i32,
) -> Option<u32> {
    let left = point.x() < pixel_length(client_rect.left() + resize_border_x);
    let right = point.x() >= pixel_length(client_rect.right() - resize_border_x);
    let top = point.y() < pixel_length(client_rect.top() + resize_border_y);
    let bottom = point.y() >= pixel_length(client_rect.bottom() - resize_border_y);

    if top && left {
        Some(HTTOPLEFT)
    } else if top && right {
        Some(HTTOPRIGHT)
    } else if bottom && left {
        Some(HTBOTTOMLEFT)
    } else if bottom && right {
        Some(HTBOTTOMRIGHT)
    } else if left {
        Some(HTLEFT)
    } else if right {
        Some(HTRIGHT)
    } else if top {
        Some(HTTOP)
    } else if bottom {
        Some(HTBOTTOM)
    } else {
        None
    }
}

fn pixel_length(value: i32) -> Length {
    Length::new::<meter>(f64::from(value))
}

fn integral_pixel(length: Length) -> eyre::Result<i32> {
    let pixels = length.get::<meter>();
    if !pixels.is_finite() {
        eyre::bail!("pixel coordinate was not finite")
    }

    let rounded = pixels.round();
    if (rounded - pixels).abs() > f64::EPSILON {
        eyre::bail!("pixel coordinate {pixels} was not integral")
    }
    if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        eyre::bail!("pixel coordinate {pixels} was outside i32 range")
    }

    rounded
        .to_string()
        .parse::<i32>()
        .wrap_err_with(|| format!("pixel coordinate {pixels} could not be represented as i32"))
}

fn signed_word_i32(value: isize) -> i32 {
    let low_word = u16::try_from(value & 0xFFFF).expect("masking to 16 bits must fit in u16");
    i32::from(i16::from_le_bytes(low_word.to_le_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_client_transform_roundtrips_points() {
        let transform = ScreenToClientTransform::for_window(ScreenRect::new(300, 400, 800, 900));

        let client = transform.screen_to_client(ScreenPoint::new(325, 442));
        assert_eq!(client.to_win32_point().unwrap(), POINT { x: 25, y: 42 });

        let screen = transform.client_to_screen(client);
        assert_eq!(screen.to_win32_point().unwrap(), POINT { x: 325, y: 442 });
    }

    #[test]
    fn client_rect_contains_only_points_in_same_space() {
        let rect = ClientRect::new(10, 20, 30, 40);

        assert!(rect.contains(ClientPoint::new(10, 20)));
        assert!(rect.contains(ClientPoint::new(29, 39)));
        assert!(!rect.contains(ClientPoint::new(30, 39)));
        assert!(!rect.contains(ClientPoint::new(29, 40)));
    }

    #[test]
    fn resize_border_prefers_top_left_corner() {
        let hit = classify_resize_border_hit(
            ClientRect::new(0, 0, 400, 300),
            ClientPoint::new(2, 3),
            8,
            8,
        );

        assert_eq!(hit, Some(HTTOPLEFT));
    }

    #[test]
    fn resize_border_ignores_interior_points() {
        let hit = classify_resize_border_hit(
            ClientRect::new(0, 0, 400, 300),
            ClientPoint::new(200, 120),
            8,
            8,
        );

        assert_eq!(hit, None);
    }

    #[test]
    fn zero_drag_threshold_has_no_deadzone() {
        assert!(drag_threshold_exceeded(
            ClientPoint::new(10, 20),
            ClientPoint::new(10, 20),
            0,
            0,
        ));
    }

    #[test]
    fn positive_drag_threshold_requires_real_motion() {
        assert!(!drag_threshold_exceeded(
            ClientPoint::new(10, 20),
            ClientPoint::new(10, 20),
            1,
            1,
        ));
    }

    #[test]
    fn terminal_cell_point_maps_to_client_rect() {
        let rect =
            TerminalCellPoint::new(3, 4).to_client_rect(ClientRect::new(10, 20, 210, 220), 8, 16);

        assert_eq!(rect, ClientRect::new(34, 84, 42, 100));
    }
}
