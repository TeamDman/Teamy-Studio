use std::ops::Range;

use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

use crate::SceneVertex;

pub fn upload_cached_fragment_vertices(
    vertex_buffer: &ID3D12Resource,
    fragments: &[&[SceneVertex]],
) -> windows::core::Result<usize> {
    let vertex_count = fragments.iter().map(|fragment| fragment.len()).sum::<usize>();

    unsafe {
        let mut mapped = std::ptr::null_mut();
        vertex_buffer.Map(0, None, Some(&mut mapped))?;
        let mut write_ptr = mapped as *mut SceneVertex;
        for fragment in fragments {
            std::ptr::copy_nonoverlapping(fragment.as_ptr(), write_ptr, fragment.len());
            write_ptr = write_ptr.add(fragment.len());
        }
        vertex_buffer.Unmap(0, None);
    }

    Ok(vertex_count)
}

pub fn upload_vertex_ranges(
    vertex_buffer: &ID3D12Resource,
    vertices: &[SceneVertex],
    ranges: &[Range<usize>],
) -> windows::core::Result<()> {
    if ranges.is_empty() {
        return Ok(());
    }

    unsafe {
        let mut mapped = std::ptr::null_mut();
        vertex_buffer.Map(0, None, Some(&mut mapped))?;
        let base_ptr = mapped as *mut SceneVertex;
        for range in ranges {
            if range.is_empty() {
                continue;
            }

            std::ptr::copy_nonoverlapping(
                vertices[range.clone()].as_ptr(),
                base_ptr.add(range.start),
                range.len(),
            );
        }
        vertex_buffer.Unmap(0, None);
    }

    Ok(())
}

pub fn upload_curve_data(
    curve_buffer: &ID3D12Resource,
    curve_capacity: usize,
    curve_data: &[[f32; 4]],
) -> windows::core::Result<()> {
    unsafe {
        let mut mapped = std::ptr::null_mut();
        curve_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::write_bytes(mapped, 0, curve_capacity * std::mem::size_of::<[f32; 4]>());
        std::ptr::copy_nonoverlapping(
            curve_data.as_ptr(),
            mapped as *mut [f32; 4],
            curve_data.len(),
        );
        curve_buffer.Unmap(0, None);
    }

    Ok(())
}

pub fn upload_band_data(
    band_buffer: &ID3D12Resource,
    band_capacity: usize,
    band_data: &[u32],
) -> windows::core::Result<()> {
    unsafe {
        let mut mapped = std::ptr::null_mut();
        band_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::write_bytes(mapped, 0, band_capacity * std::mem::size_of::<u32>());
        std::ptr::copy_nonoverlapping(band_data.as_ptr(), mapped as *mut u32, band_data.len());
        band_buffer.Unmap(0, None);
    }

    Ok(())
}