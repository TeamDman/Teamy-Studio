use std::ops::Range;

use crate::SceneVertex;

#[must_use]
pub fn fragment_vertex_ranges(fragments: &[&[SceneVertex]]) -> Vec<Range<usize>> {
    let mut next_start = 0;
    let mut ranges = Vec::with_capacity(fragments.len());
    for fragment in fragments {
        let start = next_start;
        next_start += fragment.len();
        ranges.push(start..next_start);
    }
    ranges
}

#[must_use]
pub fn fragment_ranges_match(current: &[Range<usize>], next: &[Range<usize>]) -> bool {
    current.len() == next.len()
        && current
            .iter()
            .zip(next)
            .all(|(current_range, next_range)| current_range.len() == next_range.len())
}

#[must_use]
pub fn dirty_fragment_ranges(
    fragment_ranges: &[Range<usize>],
    fragments: &[&[SceneVertex]],
    fragment_reused: &[bool],
    cached_vertices: &mut [SceneVertex],
) -> Vec<Range<usize>> {
    debug_assert_eq!(fragment_ranges.len(), fragments.len());
    debug_assert_eq!(fragments.len(), fragment_reused.len());

    let mut dirty_ranges: Vec<Range<usize>> = Vec::new();

    for (index, fragment) in fragments.iter().enumerate() {
        if fragment_reused[index] {
            continue;
        }

        let range = fragment_ranges[index].clone();
        cached_vertices[range.clone()].copy_from_slice(fragment);
        if let Some(previous) = dirty_ranges.last_mut()
            && previous.end == range.start
        {
            previous.end = range.end;
            continue;
        }
        dirty_ranges.push(range);
    }

    dirty_ranges
}

#[cfg(test)]
mod tests {
    use super::{dirty_fragment_ranges, fragment_ranges_match, fragment_vertex_ranges};
    use crate::SceneVertex;

    fn vertex(x: f32) -> SceneVertex {
        SceneVertex {
            position: [x, 0.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
            uv: [0.0, 0.0],
            effect: 0.0,
            glyph: 0.0,
            glyph_data: [0.0; 4],
            banding: [0.0; 4],
            normal: [0.0; 2],
            jacobian: [0.0; 4],
            local_bounds: [0.0; 4],
            padding: [0.0; 2],
        }
    }

    #[test]
    fn fragment_vertex_ranges_follow_fragment_lengths() {
        let first = [vertex(0.0), vertex(1.0)];
        let second = [vertex(2.0)];
        let third = [vertex(3.0), vertex(4.0), vertex(5.0)];

        let ranges = fragment_vertex_ranges(&[&first, &second, &third]);

        assert_eq!(ranges, vec![0..2, 2..3, 3..6]);
    }

    #[test]
    fn fragment_ranges_match_compares_lengths_only() {
        assert!(fragment_ranges_match(&[0..3, 3..5], &[9..12, 40..42]));
        assert!(!fragment_ranges_match(&[0..3, 3..5], &[9..11, 40..42]));
        assert!(!fragment_ranges_match(&[0..3], &[0..3, 3..5]));
    }

    #[test]
    fn dirty_fragment_ranges_merges_adjacent_updated_fragments() {
        let first = [vertex(1.0), vertex(2.0)];
        let second = [vertex(3.0)];
        let third = [vertex(4.0), vertex(5.0)];
        let fragments: [&[SceneVertex]; 3] = [&first, &second, &third];
        let ranges = fragment_vertex_ranges(&fragments);
        let mut cached = vec![vertex(-1.0); 5];

        let dirty = dirty_fragment_ranges(&ranges, &fragments, &[false, false, true], &mut cached);

        assert_eq!(dirty, vec![0..3]);
        assert_eq!(cached[0].position[0], 1.0);
        assert_eq!(cached[2].position[0], 3.0);
        assert_eq!(cached[3].position[0], -1.0);
    }
}