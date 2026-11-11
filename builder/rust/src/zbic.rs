//! ZBIC decompression through the vendored zstd built by build.rs.
//! ZBIC is the Switch firmware 22.0.0+ zstd variant used by game modules.

use std::os::raw::c_void;

unsafe extern "C" {
    fn zbic_decompress(dst: *mut c_void, capacity: usize, src: *const c_void, size: usize) -> i64;
}

/// Decompress one ZBIC frame. `size` is the segment's uncompressed size.
pub fn decompress(data: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; size];
    let written = unsafe {
        zbic_decompress(
            out.as_mut_ptr().cast(),
            size,
            data.as_ptr().cast(),
            data.len(),
        )
    };
    if written != size as i64 {
        return Err(format!(
            "ZBIC: decompression failed ({} of {size} bytes)",
            written
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_data_that_is_not_a_frame() {
        assert!(decompress(b"not a zstd frame", 16).is_err());
    }
}
