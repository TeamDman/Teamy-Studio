use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

use eyre::WrapErr;
use fontdb::{Database, Family, Query, Source};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

const FALLBACK_GLYPH: char = '?';
const TERMINAL_FONT_FAMILY: &str = "CaskaydiaCove Nerd Font Mono";
const SLUG_BAND_SIZE_FONT_UNITS: f32 = 64.0;

static TERMINAL_FONT_CACHE: OnceLock<Result<Arc<LoadedTerminalFont>, String>> = OnceLock::new();
static TERMINAL_FONT_LAYOUT_CACHE: OnceLock<Result<Arc<TerminalFontLayoutSnapshot>, String>> =
    OnceLock::new();

#[derive(Clone, Debug)]
pub struct LoadedTerminalFont {
    pub font_bytes: Vec<u8>,
    pub face_index: u32,
    pub units_per_em: f32,
    pub ascender: f32,
    pub descender: f32,
    pub cell_advance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalFontGlyphMetrics {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
    pub advance: f32,
}

#[derive(Clone, Debug)]
pub struct TerminalFontLayoutSnapshot {
    pub units_per_em: f32,
    pub ascender: f32,
    pub descender: f32,
    pub cell_advance: f32,
    pub glyphs: HashMap<char, TerminalFontGlyphMetrics>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlugGlyph {
    pub curve_start: u32,
    pub curve_count: u32,
    pub band_start: u32,
    pub band_count_x: u32,
    pub band_count_y: u32,
    pub band_transform: [f32; 4],
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
    pub advance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadraticCurve {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub p2: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct SlugAtlasData {
    pub font: Arc<LoadedTerminalFont>,
    pub curve_data: Vec<[f32; 4]>,
    pub band_data: Vec<u32>,
    pub glyphs: HashMap<char, SlugGlyph>,
}

#[derive(Clone, Copy, Debug)]
struct CurveExtents {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

pub fn cached_terminal_font() -> eyre::Result<Arc<LoadedTerminalFont>> {
    TERMINAL_FONT_CACHE
        .get_or_init(|| {
            load_terminal_font()
                .map(Arc::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map(Arc::clone)
        .map_err(|error| eyre::eyre!(error.clone()))
}

pub fn terminal_font_layout_snapshot() -> eyre::Result<Arc<TerminalFontLayoutSnapshot>> {
    TERMINAL_FONT_LAYOUT_CACHE
        .get_or_init(|| {
            build_terminal_font_layout_snapshot()
                .map(Arc::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map(Arc::clone)
        .map_err(|error| eyre::eyre!(error.clone()))
}

pub fn terminal_font_unicode_chars() -> eyre::Result<Vec<char>> {
    let font = load_terminal_font()?;
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font face for glyph enumeration")?;
    Ok(collect_font_unicode_chars(&face))
}

pub fn build_terminal_slug_atlas(chars: &[char]) -> eyre::Result<SlugAtlasData> {
    let font = cached_terminal_font()?;
    let (curve_data, band_data, glyphs) = build_slug_curve_buffer(font.as_ref(), chars)?;
    Ok(SlugAtlasData {
        font,
        curve_data,
        band_data,
        glyphs,
    })
}

fn load_terminal_font() -> eyre::Result<LoadedTerminalFont> {
    let mut database = Database::new();
    database.load_system_fonts();
    let query = Query {
        families: &[Family::Name(TERMINAL_FONT_FAMILY)],
        ..Query::default()
    };
    let font_id = database
        .query(&query)
        .ok_or_else(|| eyre::eyre!("failed to locate installed terminal font"))?;
    let face_info = database
        .face(font_id)
        .ok_or_else(|| eyre::eyre!("fontdb returned an invalid font handle"))?;

    let font_bytes = match &face_info.source {
        Source::File(path) => std::fs::read(path)
            .wrap_err_with(|| format!("failed to read font file {}", path.display()))?,
        Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        Source::SharedFile(path, _) => std::fs::read(path)
            .wrap_err_with(|| format!("failed to read shared font file {}", path.display()))?,
    };

    let face = Face::parse(&font_bytes, face_info.index)
        .wrap_err("failed to parse installed terminal font")?;
    let fallback_id = face
        .glyph_index(FALLBACK_GLYPH)
        .or_else(|| face.glyph_index('W'))
        .ok_or_else(|| eyre::eyre!("terminal font did not contain expected fallback glyphs"))?;
    let cell_advance = face
        .glyph_hor_advance(fallback_id)
        .map_or(1024.0, f32::from);
    let units_per_em = f32::from(face.units_per_em());
    let ascender = f32::from(face.ascender());
    let descender = f32::from(face.descender());

    Ok(LoadedTerminalFont {
        font_bytes,
        face_index: face_info.index,
        units_per_em,
        ascender,
        descender,
        cell_advance,
    })
}

fn build_terminal_font_layout_snapshot() -> eyre::Result<TerminalFontLayoutSnapshot> {
    let font = load_terminal_font()?;
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font for layout snapshot")?;
    let glyphs = collect_font_unicode_chars(&face)
        .into_iter()
        .filter_map(|character| {
            let glyph_id = face.glyph_index(character)?;
            let glyph = build_slug_glyph_from_face(&font, &face, glyph_id, 0, 0);
            Some((
                character,
                TerminalFontGlyphMetrics {
                    x_min: glyph.x_min,
                    y_min: glyph.y_min,
                    x_max: glyph.x_max,
                    y_max: glyph.y_max,
                    advance: glyph.advance,
                },
            ))
        })
        .collect();

    Ok(TerminalFontLayoutSnapshot {
        units_per_em: font.units_per_em,
        ascender: font.ascender,
        descender: font.descender,
        cell_advance: font.cell_advance,
        glyphs,
    })
}

fn build_slug_curve_buffer(
    font: &LoadedTerminalFont,
    chars: &[char],
) -> eyre::Result<(Vec<[f32; 4]>, Vec<u32>, HashMap<char, SlugGlyph>)> {
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font for slug curve build")?;
    let fallback_id = face
        .glyph_index(FALLBACK_GLYPH)
        .ok_or_else(|| eyre::eyre!("terminal font did not contain fallback glyph"))?;
    let mut curve_data = Vec::new();
    let mut band_data = Vec::new();
    let mut glyphs = HashMap::new();

    for character in chars {
        let glyph_id = face.glyph_index(*character).unwrap_or(fallback_id);
        let curves = extract_glyph_curves(&face, glyph_id);
        let curve_start = u32::try_from(curve_data.len()).unwrap_or(u32::MAX);
        for curve in &curves {
            curve_data.push([curve.p0[0], curve.p0[1], curve.p1[0], curve.p1[1]]);
            curve_data.push([curve.p2[0], curve.p2[1], 0.0, 0.0]);
        }
        let band_start = u32::try_from(band_data.len()).unwrap_or(u32::MAX);
        let glyph = build_slug_glyph_from_face(font, &face, glyph_id, curve_start, curves.len());
        let (band_count_x, band_count_y, band_transform) =
            append_slug_band_data(&curves, glyph, &mut band_data);

        glyphs.insert(
            *character,
            SlugGlyph {
                band_start,
                band_count_x,
                band_count_y,
                band_transform,
                ..glyph
            },
        );
    }

    Ok((curve_data, band_data, glyphs))
}

fn extract_glyph_curves(face: &Face<'_>, glyph_id: GlyphId) -> Vec<QuadraticCurve> {
    let mut builder = QuadraticCurveBuilder::default();
    let _ = face.outline_glyph(glyph_id, &mut builder);
    builder.curves
}

fn build_slug_glyph_from_face(
    font: &LoadedTerminalFont,
    face: &Face<'_>,
    glyph_id: GlyphId,
    curve_start: u32,
    curve_count: usize,
) -> SlugGlyph {
    let advance = face
        .glyph_hor_advance(glyph_id)
        .map_or(font.cell_advance, f32::from);
    let bbox = face.glyph_bounding_box(glyph_id);

    SlugGlyph {
        curve_start,
        curve_count: u32::try_from(curve_count).unwrap_or(u32::MAX),
        band_start: 0,
        band_count_x: 1,
        band_count_y: 1,
        band_transform: [0.0; 4],
        x_min: bbox.map_or(0.0, |rect| f32::from(rect.x_min)),
        y_min: bbox.map_or(font.descender, |rect| f32::from(rect.y_min)),
        x_max: bbox.map_or(advance, |rect| f32::from(rect.x_max)),
        y_max: bbox.map_or(font.ascender, |rect| f32::from(rect.y_max)),
        advance,
    }
}

fn append_slug_band_data(
    curves: &[QuadraticCurve],
    glyph: SlugGlyph,
    band_data: &mut Vec<u32>,
) -> (u32, u32, [f32; 4]) {
    let band_count_x = compute_band_count(glyph.x_max - glyph.x_min);
    let band_count_y = compute_band_count(glyph.y_max - glyph.y_min);
    let band_transform = [
        compute_band_scale(glyph.x_min, glyph.x_max, band_count_x),
        compute_band_scale(glyph.y_min, glyph.y_max, band_count_y),
        compute_band_offset(glyph.x_min, glyph.x_max, band_count_x),
        compute_band_offset(glyph.y_min, glyph.y_max, band_count_y),
    ];
    let curve_extents: Vec<_> = curves.iter().copied().map(curve_extents).collect();
    let mut horizontal_bands = vec![Vec::<usize>::new(); band_count_y as usize];
    let mut horizontal_bands_ascending = vec![Vec::<usize>::new(); band_count_y as usize];
    let mut vertical_bands = vec![Vec::<usize>::new(); band_count_x as usize];
    let mut vertical_bands_ascending = vec![Vec::<usize>::new(); band_count_x as usize];

    for (curve_index, extents) in curve_extents.iter().enumerate() {
        let horizontal_start = band_index(
            extents.min_y,
            band_transform[1],
            band_transform[3],
            band_count_y,
        );
        let horizontal_end = band_index(
            extents.max_y,
            band_transform[1],
            band_transform[3],
            band_count_y,
        );
        for band in horizontal_start..=horizontal_end {
            horizontal_bands[band as usize].push(curve_index);
        }

        let vertical_start = band_index(
            extents.min_x,
            band_transform[0],
            band_transform[2],
            band_count_x,
        );
        let vertical_end = band_index(
            extents.max_x,
            band_transform[0],
            band_transform[2],
            band_count_x,
        );
        for band in vertical_start..=vertical_end {
            vertical_bands[band as usize].push(curve_index);
        }
    }

    for band in &mut horizontal_bands {
        band.sort_by(|lhs, rhs| {
            curve_extents[*rhs]
                .max_x
                .total_cmp(&curve_extents[*lhs].max_x)
        });
    }
    for (ascending_band, descending_band) in horizontal_bands_ascending
        .iter_mut()
        .zip(horizontal_bands.iter())
    {
        *ascending_band = descending_band.clone();
        ascending_band.sort_by(|lhs, rhs| {
            curve_extents[*lhs]
                .min_x
                .total_cmp(&curve_extents[*rhs].min_x)
        });
    }
    for band in &mut vertical_bands {
        band.sort_by(|lhs, rhs| {
            curve_extents[*rhs]
                .max_y
                .total_cmp(&curve_extents[*lhs].max_y)
        });
    }
    for (ascending_band, descending_band) in vertical_bands_ascending
        .iter_mut()
        .zip(vertical_bands.iter())
    {
        *ascending_band = descending_band.clone();
        ascending_band.sort_by(|lhs, rhs| {
            curve_extents[*lhs]
                .min_y
                .total_cmp(&curve_extents[*rhs].min_y)
        });
    }

    let table_start = band_data.len();
    let table_len = ((band_count_x + band_count_y) as usize) * 4;
    band_data.resize(table_start + table_len, 0);

    for (band_index, band) in horizontal_bands.iter().enumerate() {
        let ascending_band = &horizontal_bands_ascending[band_index];
        let split = choose_band_split(
            band,
            ascending_band,
            &curve_extents,
            |extents| extents.max_x,
            |extents| extents.min_x,
            glyph.x_min,
            glyph.x_max,
        );
        let entry_index = table_start + (band_index * 4);
        band_data[entry_index] = u32::try_from(band.len()).unwrap_or(u32::MAX);
        band_data[entry_index + 1] = u32::try_from(band_data.len()).unwrap_or(u32::MAX);
        for curve_index in band {
            band_data.push(u32::try_from(*curve_index).unwrap_or(u32::MAX));
        }
        band_data[entry_index + 2] = u32::try_from(band_data.len()).unwrap_or(u32::MAX);
        for curve_index in ascending_band {
            band_data.push(u32::try_from(*curve_index).unwrap_or(u32::MAX));
        }
        band_data[entry_index + 3] = split.to_bits();
    }

    let vertical_table_start = table_start + (band_count_y as usize * 4);
    for (band_index, band) in vertical_bands.iter().enumerate() {
        let ascending_band = &vertical_bands_ascending[band_index];
        let split = choose_band_split(
            band,
            ascending_band,
            &curve_extents,
            |extents| extents.max_y,
            |extents| extents.min_y,
            glyph.y_min,
            glyph.y_max,
        );
        let entry_index = vertical_table_start + (band_index * 4);
        band_data[entry_index] = u32::try_from(band.len()).unwrap_or(u32::MAX);
        band_data[entry_index + 1] = u32::try_from(band_data.len()).unwrap_or(u32::MAX);
        for curve_index in band {
            band_data.push(u32::try_from(*curve_index).unwrap_or(u32::MAX));
        }
        band_data[entry_index + 2] = u32::try_from(band_data.len()).unwrap_or(u32::MAX);
        for curve_index in ascending_band {
            band_data.push(u32::try_from(*curve_index).unwrap_or(u32::MAX));
        }
        band_data[entry_index + 3] = split.to_bits();
    }

    (band_count_x, band_count_y, band_transform)
}

fn choose_band_split(
    descending_band: &[usize],
    ascending_band: &[usize],
    curve_extents: &[CurveExtents],
    descending_value: impl Fn(CurveExtents) -> f32,
    ascending_value: impl Fn(CurveExtents) -> f32,
    fallback_min: f32,
    fallback_max: f32,
) -> f32 {
    let count = descending_band.len();
    if count == 0 {
        return (fallback_min + fallback_max) * 0.5;
    }

    let mut best_worst = count;
    let mut best_split = (fallback_min + fallback_max) * 0.5;
    let mut left_count = count;

    for (curve_offset, curve_index) in descending_band.iter().enumerate() {
        let split = descending_value(curve_extents[*curve_index]);
        let right_count = curve_offset + 1;
        while left_count > 0
            && ascending_value(curve_extents[ascending_band[left_count - 1]]) > split
        {
            left_count -= 1;
        }
        let worst = right_count.max(left_count);
        if worst < best_worst {
            best_worst = worst;
            best_split = split;
        }
    }

    best_split
}

fn curve_extents(curve: QuadraticCurve) -> CurveExtents {
    CurveExtents {
        min_x: curve.p0[0].min(curve.p1[0]).min(curve.p2[0]),
        max_x: curve.p0[0].max(curve.p1[0]).max(curve.p2[0]),
        min_y: curve.p0[1].min(curve.p1[1]).min(curve.p2[1]),
        max_y: curve.p0[1].max(curve.p1[1]).max(curve.p2[1]),
    }
}

fn compute_band_count(span: f32) -> u32 {
    ((span.max(1.0) / SLUG_BAND_SIZE_FONT_UNITS).ceil() as u32).clamp(1, 255)
}

fn compute_band_scale(min_value: f32, max_value: f32, band_count: u32) -> f32 {
    let _ = min_value;
    band_count.max(1) as f32 / (max_value - min_value).max(1.0)
}

fn compute_band_offset(min_value: f32, max_value: f32, band_count: u32) -> f32 {
    -(min_value * compute_band_scale(min_value, max_value, band_count))
}

fn band_index(value: f32, scale: f32, offset: f32, band_count: u32) -> u32 {
    ((value * scale) + offset)
        .trunc()
        .clamp(0.0, band_count.saturating_sub(1) as f32) as u32
}

fn collect_font_unicode_chars(face: &Face<'_>) -> Vec<char> {
    let mut chars = BTreeSet::new();
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            if !subtable.is_unicode() {
                continue;
            }
            subtable.codepoints(|codepoint| {
                if let Some(character) = char::from_u32(codepoint)
                    && face.glyph_index(character).is_some()
                {
                    chars.insert(character);
                }
            });
        }
    }
    chars.into_iter().collect()
}

#[derive(Default)]
struct QuadraticCurveBuilder {
    current: [f32; 2],
    start: [f32; 2],
    contours: usize,
    curves: Vec<QuadraticCurve>,
}

impl QuadraticCurveBuilder {
    fn line_to_curve(&mut self, x: f32, y: f32) {
        let midpoint = [(self.current[0] + x) * 0.5, (self.current[1] + y) * 0.5];
        self.curves.push(QuadraticCurve {
            p0: self.current,
            p1: midpoint,
            p2: [x, y],
        });
        self.current = [x, y];
    }

    fn close_contour(&mut self) {
        if self.current != self.start {
            self.line_to_curve(self.start[0], self.start[1]);
        }
        self.contours += 1;
    }
}

impl OutlineBuilder for QuadraticCurveBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.contours == 0 && !self.curves.is_empty() {
            self.close_contour();
        }
        self.current = [x, y];
        self.start = [x, y];
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.line_to_curve(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.curves.push(QuadraticCurve {
            p0: self.current,
            p1: [x1, y1],
            p2: [x, y],
        });
        self.current = [x, y];
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.current;
        let c1 = [x1, y1];
        let c2 = [x2, y2];
        let p3 = [x, y];

        let q0 = [
            p0[0] + ((c1[0] - p0[0]) * (2.0 / 3.0)),
            p0[1] + ((c1[1] - p0[1]) * (2.0 / 3.0)),
        ];
        let q2 = [
            p3[0] + ((c2[0] - p3[0]) * (2.0 / 3.0)),
            p3[1] + ((c2[1] - p3[1]) * (2.0 / 3.0)),
        ];
        let midpoint = [(q0[0] + q2[0]) * 0.5, (q0[1] + q2[1]) * 0.5];

        self.curves.push(QuadraticCurve {
            p0,
            p1: q0,
            p2: midpoint,
        });
        self.curves.push(QuadraticCurve {
            p0: midpoint,
            p1: q2,
            p2: p3,
        });
        self.current = p3;
    }

    fn close(&mut self) {
        self.close_contour();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_terminal_slug_atlas, terminal_font_layout_snapshot, terminal_font_unicode_chars,
    };

    #[test]
    fn terminal_font_layout_snapshot_is_populated() -> eyre::Result<()> {
        let snapshot = terminal_font_layout_snapshot()?;
        assert!(snapshot.cell_advance > 0.0);
        assert!(snapshot.ascender > snapshot.descender);
        assert!(snapshot.glyphs.contains_key(&'A'));
        Ok(())
    }

    #[test]
    fn terminal_font_unicode_chars_is_not_empty() -> eyre::Result<()> {
        let characters = terminal_font_unicode_chars()?;
        assert!(!characters.is_empty());
        assert!(characters.contains(&'A'));
        Ok(())
    }

    #[test]
    fn terminal_slug_atlas_builds_curve_and_band_data() -> eyre::Result<()> {
        let atlas = build_terminal_slug_atlas(&['A', 'b', '?'])?;
        assert_eq!(atlas.glyphs.len(), 3);
        assert!(!atlas.curve_data.is_empty());
        assert!(!atlas.band_data.is_empty());
        assert!(atlas.glyphs.contains_key(&'A'));
        Ok(())
    }
}
