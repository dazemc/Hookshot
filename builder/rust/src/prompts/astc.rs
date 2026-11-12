//! ASTC 4x4 compression for the button-note and controller textures, through
//! the vendored ARM encoder (`astcenc/`). The wrapper takes RGBA8 and writes
//! one 16-byte block per 4x4 tile.

unsafe extern "C" {
    fn hookshot_astc_compress(
        rgba: *const u8,
        width: i32,
        height: i32,
        srgb: i32,
        out: *mut u8,
        out_size: u64,
    ) -> i32;
}

/// Compress an RGBA8 image as ASTC 4x4, LDR or LDR-sRGB.
pub fn encode(rgba: &[u8], width: u32, height: u32, srgb: bool) -> Result<Vec<u8>, String> {
    let expected = (width as usize).div_ceil(4) * (height as usize).div_ceil(4) * 16;
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(format!(
            "astc: {} bytes for {}x{} RGBA",
            rgba.len(),
            width,
            height
        ));
    }
    let mut out = vec![0u8; expected];
    let code = unsafe {
        hookshot_astc_compress(
            rgba.as_ptr(),
            width as i32,
            height as i32,
            i32::from(srgb),
            out.as_mut_ptr(),
            out.len() as u64,
        )
    };
    if code != 0 {
        return Err(format!("astc: encoder returned {code}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solid_block_compresses_to_sixteen_nonzero_bytes() {
        let red = [255u8, 0, 0, 255].repeat(16);
        let out = encode(&red, 4, 4, false).unwrap();
        assert_eq!(out.len(), 16);
        assert!(out.iter().any(|byte| *byte != 0));
        let srgb = encode(&red, 4, 4, true).unwrap();
        assert_eq!(srgb.len(), 16);
    }

    #[test]
    fn a_gradient_is_one_block_a_tile() {
        let mut rgba = Vec::new();
        for y in 0..8u32 {
            for x in 0..8u32 {
                rgba.extend_from_slice(&[(x * 31) as u8, (y * 31) as u8, 0, 255]);
            }
        }
        assert_eq!(encode(&rgba, 8, 8, false).unwrap().len(), 64);
        assert!(encode(&rgba[..12], 4, 4, false).is_err());
    }
}
