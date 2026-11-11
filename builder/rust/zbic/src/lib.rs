//! The two ZBIC entry points, exported for the engine to load.
use std::os::raw::{c_int, c_void};

unsafe extern "C" {
    fn zbic_decompress(dst: *mut c_void, capacity: usize, src: *const c_void, size: usize) -> i64;
}

/// Decompress one frame, returning the number of bytes written or -2.
///
/// # Safety
/// `src` must point to `size` readable bytes and `dst` to `capacity` writable ones.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hookshot_zbic_decompress(
    dst: *mut c_void,
    capacity: usize,
    src: *const c_void,
    size: usize,
) -> i64 {
    unsafe { zbic_decompress(dst, capacity, src, size) }
}

/// -1: the frame states no content size; -2: bad data. Kept for callers that
/// need the size before allocating the output.
#[unsafe(no_mangle)]
pub extern "C" fn hookshot_zbic_content_size(_src: *const c_void, _size: usize) -> i64 {
    // The engine always knows a segment's uncompressed size from its NSO
    // header; a separate size call would need a second zstd symbol and is not
    // exported here yet.
    -1 as c_int as i64
}
