//! The `.bfotf` wrapper: an OpenType font behind an 8-byte header and XORed
//! per little-endian word, as `nx2/prompt_assets.py` writes it.

pub const MAGIC: u32 = 0x1802_9A7F;

/// Split a wrapped font into its key and its OpenType bytes. The size in the
/// header is big-endian before the XOR (as the Python `wrap`).
pub fn unwrap(blob: &[u8]) -> Result<(u32, Vec<u8>), String> {
    if blob.len() < 8 || (blob.len() - 8) % 4 != 0 {
        return Err(
            "the font is not a .bfotf (its length is not 8 + a whole number of words)".into(),
        );
    }
    let key = u32::from_le_bytes(blob[0..4].try_into().unwrap()) ^ MAGIC;
    let size = (u32::from_le_bytes(blob[4..8].try_into().unwrap()) ^ key).swap_bytes() as usize;
    let mut body = Vec::with_capacity(blob.len() - 8);
    for word in blob[8..].chunks_exact(4) {
        body.extend_from_slice(&(u32::from_le_bytes(word.try_into().unwrap()) ^ key).to_le_bytes());
    }
    if size > body.len() {
        return Err(format!(
            "the .bfotf says its font is {size} bytes, the wrapper holds {}",
            body.len()
        ));
    }
    body.truncate(size);
    Ok((key, body))
}

/// Wrap OpenType bytes: the header's magic and size, then every word XORed
/// with `key`. The font is padded to a whole number of words first.
pub fn wrap(key: u32, otf: &[u8]) -> Vec<u8> {
    let mut body = otf.to_vec();
    body.resize(body.len().div_ceil(4) * 4, 0);
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&(MAGIC ^ key).to_le_bytes());
    let size = (body.len() as u32).swap_bytes() ^ key;
    out.extend_from_slice(&size.to_le_bytes());
    for word in body.chunks_exact(4) {
        out.extend_from_slice(&(u32::from_le_bytes(word.try_into().unwrap()) ^ key).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_survives_the_round_trip() {
        let key = 0x1234_5678;
        let otf = b"OTTO a small fake font";
        let wrapped = wrap(key, otf);
        assert_eq!(wrapped.len() % 4, 0);
        let (found, plain) = unwrap(&wrapped).unwrap();
        assert_eq!(found, key);
        // (as Python: the header's size counts the zero padding)
        assert_eq!(&plain[..otf.len()], otf);
        assert!(plain[otf.len()..].iter().all(|b| *b == 0));
    }

    #[test]
    fn the_header_is_the_magic_and_the_big_endian_size() {
        let key = 0xA1B2_C3D4;
        let wrapped = wrap(key, b"OTTO");
        assert_eq!(
            u32::from_le_bytes(wrapped[0..4].try_into().unwrap()),
            MAGIC ^ key
        );
        assert_eq!(
            u32::from_le_bytes(wrapped[4..8].try_into().unwrap()),
            (4u32).swap_bytes() ^ key
        );
    }

    #[test]
    fn padding_is_not_part_of_the_font() {
        let wrapped = wrap(0, b"OTTO12345");
        let (_, plain) = unwrap(&wrapped).unwrap();
        assert_eq!(&plain[..9], b"OTTO12345");
        assert_eq!(plain.len(), 12);
    }

    #[test]
    fn a_bad_length_is_an_error() {
        assert!(unwrap(b"short").is_err());
        assert!(unwrap(&[0; 11]).is_err());
    }
}
