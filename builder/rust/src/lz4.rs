//! LZ4 block decompression (the format NSO segments use). Small enough to
//! keep in the engine: the block is one token stream, and the output size is
//! in the NSO header.

pub fn decompress(source: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(size);
    let mut at = 0usize;
    while at < source.len() {
        let token = source[at];
        at += 1;
        let mut literals = (token >> 4) as usize;
        if literals == 15 {
            loop {
                let byte = *source.get(at).ok_or("truncated LZ4 literal length")?;
                at += 1;
                literals += byte as usize;
                if byte != 255 {
                    break;
                }
            }
        }
        let end = at.checked_add(literals).ok_or("LZ4 block overflows")?;
        let bytes = source.get(at..end).ok_or("truncated LZ4 literals")?;
        out.extend_from_slice(bytes);
        at = end;
        if at >= source.len() {
            break;
        }
        let offset = u16::from_le_bytes(
            source
                .get(at..at + 2)
                .ok_or("truncated LZ4 offset")?
                .try_into()
                .unwrap(),
        ) as usize;
        at += 2;
        if offset == 0 || offset > out.len() {
            return Err("invalid LZ4 match offset".into());
        }
        let mut length = (token & 15) as usize;
        if length == 15 {
            loop {
                let byte = *source.get(at).ok_or("truncated LZ4 match length")?;
                at += 1;
                length += byte as usize;
                if byte != 255 {
                    break;
                }
            }
        }
        length += 4;
        let start = out.len() - offset;
        for i in 0..length {
            let byte = out[start + i];
            out.push(byte);
            if out.len() > size {
                return Err("LZ4 block grows past its size".into());
            }
        }
    }
    if out.len() != size {
        return Err(format!("LZ4 block is {} bytes, {size} expected", out.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_an_overlapping_match() {
        assert_eq!(decompress(&[0x30, b'a', b'b', b'c'], 3).unwrap(), b"abc");
        assert_eq!(
            decompress(&[0x40, b'a', b'b', b'c', b'd', 4, 0], 8).unwrap(),
            b"abcdabcd"
        );
        assert!(decompress(&[0x10, b'a', 9, 0], 8).is_err());
    }
}
