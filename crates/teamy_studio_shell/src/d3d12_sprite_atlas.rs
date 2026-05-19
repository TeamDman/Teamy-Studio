use std::sync::OnceLock;

const SPRITE_SLOT_SIZE: u32 = 128;
const SPRITE_ATLAS_COLUMNS: u32 = 4;
const SPRITE_ATLAS_ROWS: u32 = 4;
const GRID_UNITS: i32 = 16;

static SPRITE_ATLAS_PIXELS: OnceLock<Box<[u32]>> = OnceLock::new();

#[must_use]
pub const fn sprite_atlas_dimensions() -> (u32, u32) {
    (
        SPRITE_SLOT_SIZE * SPRITE_ATLAS_COLUMNS,
        SPRITE_SLOT_SIZE * SPRITE_ATLAS_ROWS,
    )
}

#[must_use]
pub fn sprite_buffer_size_bytes() -> u64 {
    let (width, height) = sprite_atlas_dimensions();
    u64::from(width) * u64::from(height) * std::mem::size_of::<u32>() as u64
}

#[must_use]
pub fn sprite_atlas_pixels() -> &'static [u32] {
    SPRITE_ATLAS_PIXELS.get_or_init(|| build_sprite_atlas().into_boxed_slice())
}

fn build_sprite_atlas() -> Vec<u32> {
    let (width, height) = sprite_atlas_dimensions();
    let mut pixels = vec![0_u32; (width * height) as usize];
    draw_terminal_sprite(&mut pixels, width, slot_origin(0));
    draw_storage_sprite(&mut pixels, width, slot_origin(1));
    draw_audio_sprite(&mut pixels, width, slot_origin(2));
    draw_cursor_arrow_sprite(&mut pixels, width, slot_origin(3));
    draw_cursor_hand_sprite(&mut pixels, width, slot_origin(4));
    draw_cursor_ibeam_sprite(&mut pixels, width, slot_origin(5));
    draw_cursor_cross_sprite(&mut pixels, width, slot_origin(6));
    draw_cursor_wait_sprite(&mut pixels, width, slot_origin(7));
    draw_cursor_size_all_sprite(&mut pixels, width, slot_origin(8));
    draw_cursor_help_sprite(&mut pixels, width, slot_origin(9));
    pixels
}

const fn slot_origin(slot_index: u32) -> (i32, i32) {
    (
        ((slot_index % SPRITE_ATLAS_COLUMNS) * SPRITE_SLOT_SIZE) as i32,
        ((slot_index / SPRITE_ATLAS_COLUMNS) * SPRITE_SLOT_SIZE) as i32,
    )
}

fn draw_terminal_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let bezel = inset_rect(pct_rect(origin, 0, 0, 100, 100), 2);
    let screen = inset_rect(bezel, 3);
    fill_rect(pixels, atlas_width, bezel, [46, 61, 82, 255]);
    fill_rect(pixels, atlas_width, screen, [8, 15, 26, 255]);
    draw_grid_block(
        pixels,
        atlas_width,
        origin,
        (1, 1, 1, 1),
        [87, 212, 255, 255],
    );
    draw_grid_block(
        pixels,
        atlas_width,
        origin,
        (2, 2, 1, 1),
        [87, 212, 255, 255],
    );
    draw_grid_block(
        pixels,
        atlas_width,
        origin,
        (4, 4, 3, 1),
        [87, 212, 255, 255],
    );
    fill_rect(
        pixels,
        atlas_width,
        pct_rect(origin, 34, 78, 66, 84),
        [64, 79, 112, 255],
    );
    fill_rect(
        pixels,
        atlas_width,
        pct_rect(origin, 22, 86, 78, 92),
        [51, 66, 97, 255],
    );
}

fn draw_storage_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let rack_fill = [94, 163, 134, 255];
    let rack_stroke = [115, 199, 163, 255];
    for row in 0..3 {
        let top = 18 + row * 22;
        let rack = pct_rect(origin, 16, top, 84, top + 12);
        fill_rect(pixels, atlas_width, rack, rack_fill);
        stroke_rect(pixels, atlas_width, rack, 2, rack_stroke);
        fill_rect(
            pixels,
            atlas_width,
            pct_rect(origin, 22, top + 4, 28, top + 8),
            [191, 245, 214, 255],
        );
    }
}

fn draw_audio_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    fill_rect(
        pixels,
        atlas_width,
        pct_rect(origin, 28, 46, 80, 56),
        [66, 66, 71, 255],
    );
    fill_rect(
        pixels,
        atlas_width,
        pct_rect(origin, 18, 42, 38, 62),
        [214, 181, 99, 255],
    );
    fill_rect(
        pixels,
        atlas_width,
        pct_rect(origin, 54, 42, 74, 62),
        [201, 71, 46, 255],
    );
    fill_rect(
        pixels,
        atlas_width,
        pct_rect(origin, 14, 48, 18, 56),
        [245, 230, 168, 255],
    );
    fill_rect(
        pixels,
        atlas_width,
        pct_rect(origin, 74, 48, 78, 56),
        [250, 219, 179, 255],
    );
}

fn draw_cursor_arrow_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let color = [247, 230, 20, 255];
    let shadow = [5, 5, 8, 107];
    let blocks = [
        (0, 0, 1, 8),
        (1, 1, 1, 8),
        (2, 2, 1, 8),
        (3, 3, 1, 8),
        (4, 4, 1, 7),
        (5, 5, 1, 6),
        (6, 6, 1, 5),
        (7, 7, 1, 4),
        (8, 8, 1, 3),
        (9, 9, 1, 2),
        (3, 8, 4, 1),
        (4, 9, 4, 1),
        (5, 10, 4, 1),
        (6, 11, 3, 1),
        (5, 12, 2, 1),
        (7, 10, 1, 4),
    ];
    for (x, y, width, height) in blocks {
        draw_grid_block(
            pixels,
            atlas_width,
            origin,
            (x + 1, y + 1, width, height),
            shadow,
        );
        draw_grid_block(pixels, atlas_width, origin, (x, y, width, height), color);
    }
}

fn draw_cursor_hand_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let color = [255, 255, 255, 255];
    let shadow = [16, 22, 30, 120];
    let blocks = [
        (6, 2, 1, 8),
        (8, 1, 1, 9),
        (10, 2, 1, 8),
        (12, 3, 1, 7),
        (5, 9, 9, 2),
        (4, 11, 10, 2),
        (4, 13, 8, 2),
        (5, 15, 6, 1),
    ];
    draw_sprite_blocks(pixels, atlas_width, origin, &blocks, color, shadow);
}

fn draw_cursor_ibeam_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let color = [255, 255, 255, 255];
    let shadow = [16, 22, 30, 120];
    let blocks = [(4, 2, 8, 1), (4, 13, 8, 1), (7, 3, 2, 10)];
    draw_sprite_blocks(pixels, atlas_width, origin, &blocks, color, shadow);
}

fn draw_cursor_cross_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let color = [255, 255, 255, 255];
    let shadow = [16, 22, 30, 120];
    let blocks = [
        (7, 1, 2, 14),
        (1, 7, 14, 2),
        (4, 4, 2, 2),
        (10, 4, 2, 2),
        (4, 10, 2, 2),
        (10, 10, 2, 2),
    ];
    draw_sprite_blocks(pixels, atlas_width, origin, &blocks, color, shadow);
}

fn draw_cursor_wait_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let ring = [255, 255, 255, 255];
    let accent = [255, 255, 255, 160];
    let shadow = [16, 22, 30, 120];
    let blocks = [
        (5, 1, 6, 1),
        (5, 14, 6, 1),
        (6, 2, 4, 1),
        (6, 13, 4, 1),
        (3, 4, 1, 2),
        (12, 4, 1, 2),
        (3, 10, 1, 2),
        (12, 10, 1, 2),
        (4, 3, 1, 1),
        (11, 3, 1, 1),
        (4, 12, 1, 1),
        (11, 12, 1, 1),
        (7, 4, 2, 5),
        (8, 8, 3, 2),
    ];
    draw_sprite_blocks(pixels, atlas_width, origin, &blocks[..12], ring, shadow);
    draw_sprite_blocks(pixels, atlas_width, origin, &blocks[12..], accent, shadow);
}

fn draw_cursor_size_all_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let color = [255, 255, 255, 255];
    let shadow = [16, 22, 30, 120];
    let blocks = [
        (7, 1, 2, 14),
        (1, 7, 14, 2),
        (6, 1, 4, 2),
        (6, 13, 4, 2),
        (1, 6, 2, 4),
        (13, 6, 2, 4),
    ];
    draw_sprite_blocks(pixels, atlas_width, origin, &blocks, color, shadow);
}

fn draw_cursor_help_sprite(pixels: &mut [u32], atlas_width: u32, origin: (i32, i32)) {
    let color = [255, 255, 255, 255];
    let shadow = [16, 22, 30, 120];
    let blocks = [
        (5, 2, 5, 1),
        (4, 3, 1, 2),
        (10, 3, 1, 2),
        (8, 4, 1, 2),
        (7, 6, 2, 1),
        (7, 8, 2, 1),
        (7, 11, 2, 2),
    ];
    draw_sprite_blocks(pixels, atlas_width, origin, &blocks, color, shadow);
}

fn draw_sprite_blocks(
    pixels: &mut [u32],
    atlas_width: u32,
    origin: (i32, i32),
    blocks: &[(i32, i32, i32, i32)],
    color: [u8; 4],
    shadow: [u8; 4],
) {
    for (x, y, width, height) in blocks {
        draw_grid_block(
            pixels,
            atlas_width,
            origin,
            (x + 1, y + 1, *width, *height),
            shadow,
        );
        draw_grid_block(
            pixels,
            atlas_width,
            origin,
            (*x, *y, *width, *height),
            color,
        );
    }
}

fn pct_rect(
    origin: (i32, i32),
    left_pct: i32,
    top_pct: i32,
    right_pct: i32,
    bottom_pct: i32,
) -> (i32, i32, i32, i32) {
    (
        origin.0 + scale_pct(left_pct),
        origin.1 + scale_pct(top_pct),
        origin.0 + scale_pct(right_pct),
        origin.1 + scale_pct(bottom_pct),
    )
}

const fn scale_pct(value: i32) -> i32 {
    (value * SPRITE_SLOT_SIZE as i32) / 100
}

fn inset_rect(rect: (i32, i32, i32, i32), inset: i32) -> (i32, i32, i32, i32) {
    (
        rect.0 + scale_grid(inset),
        rect.1 + scale_grid(inset),
        rect.2 - scale_grid(inset),
        rect.3 - scale_grid(inset),
    )
}

const fn scale_grid(value: i32) -> i32 {
    (value * SPRITE_SLOT_SIZE as i32) / GRID_UNITS
}

fn draw_grid_block(
    pixels: &mut [u32],
    atlas_width: u32,
    origin: (i32, i32),
    rect: (i32, i32, i32, i32),
    color: [u8; 4],
) {
    let (x, y, width, height) = rect;
    fill_rect(
        pixels,
        atlas_width,
        (
            origin.0 + scale_grid(x),
            origin.1 + scale_grid(y),
            origin.0 + scale_grid(x + width),
            origin.1 + scale_grid(y + height),
        ),
        color,
    );
}

fn stroke_rect(
    pixels: &mut [u32],
    atlas_width: u32,
    rect: (i32, i32, i32, i32),
    thickness: i32,
    color: [u8; 4],
) {
    fill_rect(
        pixels,
        atlas_width,
        (rect.0, rect.1, rect.2, rect.1 + thickness),
        color,
    );
    fill_rect(
        pixels,
        atlas_width,
        (rect.0, rect.3 - thickness, rect.2, rect.3),
        color,
    );
    fill_rect(
        pixels,
        atlas_width,
        (rect.0, rect.1, rect.0 + thickness, rect.3),
        color,
    );
    fill_rect(
        pixels,
        atlas_width,
        (rect.2 - thickness, rect.1, rect.2, rect.3),
        color,
    );
}

fn fill_rect(pixels: &mut [u32], atlas_width: u32, rect: (i32, i32, i32, i32), color: [u8; 4]) {
    let atlas_height = i32::try_from(pixels.len() / atlas_width as usize).unwrap_or_default();
    let left = rect.0.clamp(0, atlas_width as i32);
    let top = rect.1.clamp(0, atlas_height);
    let right = rect.2.clamp(left, atlas_width as i32);
    let bottom = rect.3.clamp(top, atlas_height);
    for y in top..bottom {
        for x in left..right {
            blend_pixel(pixels, atlas_width, x as u32, y as u32, color);
        }
    }
}

fn blend_pixel(pixels: &mut [u32], atlas_width: u32, x: u32, y: u32, color: [u8; 4]) {
    let index = (y * atlas_width + x) as usize;
    let dst = unpack_rgba8(pixels[index]);
    let src_alpha = f32::from(color[3]) / 255.0;
    let dst_alpha = f32::from(dst[3]) / 255.0;
    let out_alpha = src_alpha + (dst_alpha * (1.0 - src_alpha));
    let out = if out_alpha <= 0.0 {
        [0, 0, 0, 0]
    } else {
        let blend = |src: u8, dst: u8| -> u8 {
            let src = f32::from(src) / 255.0;
            let dst = f32::from(dst) / 255.0;
            let value = ((src * src_alpha) + (dst * dst_alpha * (1.0 - src_alpha))) / out_alpha;
            (value * 255.0).round().clamp(0.0, 255.0) as u8
        };
        [
            blend(color[0], dst[0]),
            blend(color[1], dst[1]),
            blend(color[2], dst[2]),
            (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    };
    pixels[index] = pack_rgba8(out);
}

const fn pack_rgba8(color: [u8; 4]) -> u32 {
    u32::from_le_bytes(color)
}

const fn unpack_rgba8(packed: u32) -> [u8; 4] {
    packed.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::{sprite_atlas_dimensions, sprite_atlas_pixels, sprite_buffer_size_bytes};

    #[test]
    fn sprite_atlas_size_matches_buffer_size() {
        let (width, height) = sprite_atlas_dimensions();
        assert_eq!(
            sprite_buffer_size_bytes(),
            u64::from(width) * u64::from(height) * 4
        );
    }

    #[test]
    fn sprite_atlas_contains_visible_pixels() {
        assert!(sprite_atlas_pixels().iter().any(|pixel| *pixel != 0));
    }
}
