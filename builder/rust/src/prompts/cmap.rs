//! The cmap the written font carries: one format 4 subtable for Unicode BMP,
//! made exactly as `cmap4` in `nx2/prompt_assets.py`.

/// A cmap table of one format 4 subtable (platform 0/3, Unicode BMP) from
/// `{code: glyph id}`. Runs of codes with consecutive glyphs share a segment;
/// every code in `single` gets a segment of its own. Every segment is written
/// by idDelta alone.
pub fn cmap4(mapping: &[(u16, u16)], single: &[u16]) -> Vec<u8> {
    let mut sorted = mapping.to_vec();
    sorted.sort_by_key(|(code, _)| *code);
    let mut segs: Vec<[u16; 3]> = Vec::new();
    for (code, glyph) in sorted {
        let own = single.contains(&code);
        if let Some(last) = segs.last_mut()
            && !own
            && last[1] != u16::MAX
            && last[1] + 1 == code
            && !single.contains(&last[1])
            && last[2].wrapping_add(code) == glyph
        {
            last[1] = code;
        } else {
            segs.push([code, code, glyph.wrapping_sub(code)]);
        }
    }
    segs.push([0xFFFF, 0xFFFF, 1]);
    let n = segs.len() as u16;
    let search: u16 = 2u16 << (15 - n.leading_zeros() as u16);
    let mut out = Vec::new();
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&(16 + 8 * n).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(2 * n).to_be_bytes());
    out.extend_from_slice(&search.to_be_bytes());
    out.extend_from_slice(&((search / 2).ilog2() as u16).to_be_bytes());
    out.extend_from_slice(&(2 * n - search).to_be_bytes());
    for seg in &segs {
        out.extend_from_slice(&seg[1].to_be_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    for seg in &segs {
        out.extend_from_slice(&seg[0].to_be_bytes());
    }
    for seg in &segs {
        out.extend_from_slice(&seg[2].to_be_bytes());
    }
    out.resize(out.len() + 2 * n as usize, 0);
    // (the table record: platforms 0/3, one shared subtable at offset 20)
    let mut table = Vec::with_capacity(20 + out.len());
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&2u16.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&3u16.to_be_bytes());
    table.extend_from_slice(&20u32.to_be_bytes());
    table.extend_from_slice(&3u16.to_be_bytes());
    table.extend_from_slice(&1u16.to_be_bytes());
    table.extend_from_slice(&20u32.to_be_bytes());
    table.extend_from_slice(&out);
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_codes_share_a_segment() {
        let bytes = cmap4(&[(0x20, 10), (0x21, 11), (0x22, 12), (0x41, 20)], &[]);
        // subtable: format 4, length 16 + 8 * 2, 2 segments
        assert_eq!(&bytes[20..24], &[0, 4, 0, 40]);
        assert_eq!(&bytes[24..26], &0u16.to_be_bytes()); // language
        assert_eq!(&bytes[26..28], &6u16.to_be_bytes()); // segCountX2
        // the runs: {0x20..0x22 by delta -22}, {0x41 by delta -45}, terminator
        assert_eq!(&bytes[34..40], &[0x00, 0x22, 0x00, 0x41, 0xFF, 0xFF]);
    }

    #[test]
    fn a_single_code_keeps_its_own_segment() {
        let bytes = cmap4(&[(0xE040, 100), (0xE041, 101)], &[0xE041]);
        assert_eq!(&bytes[26..28], &6u16.to_be_bytes()); // 3 segments
    }

    #[test]
    fn the_expected_bytes_of_a_small_mapping() {
        // (the bytes Python's cmap4 gives for the same mapping)
        let bytes = cmap4(&[(65, 5), (66, 6), (0xF700, 200)], &[0xF700]);
        assert_eq!(
            bytes,
            [
                0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 20, 0, 3, 0, 1, 0, 0, 0, 20, 0, 4, 0, 40, 0, 0, 0,
                6, 0, 4, 0, 1, 0, 2, 0, 66, 0xF7, 0, 0xFF, 0xFF, 0, 0, 0, 65, 0xF7, 0, 0xFF, 0xFF,
                0xFF, 0xC4, 0x09, 0xC8, 0, 1, 0, 0, 0, 0, 0, 0,
            ]
        );
    }

    #[test]
    fn an_empty_mapping_is_the_terminator_only() {
        let bytes = cmap4(&[], &[]);
        assert_eq!(&bytes[26..28], &2u16.to_be_bytes());
        assert_eq!(&bytes[34..36], &[0xFF, 0xFF]);
    }
}
