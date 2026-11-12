//! Per-face `textures.bin`: BC4 blocks written over the game's button-icon
//! textures (`host/prompts.c` applies the runs the controls choose).
//!
//! This is the first half of `nx2/prompt_assets.py`'s texture pass: the
//! letters of the other faces' `KeyTexFontA/B/X/Y` textures. The ASTC note,
//! controller and guide-line pictures are still the game's for now.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ttf_parser::{Face, GlyphId};

use super::bntx::{self, Texture};
use super::raster::{self, Mask};
use super::{faces, zs};

/// `KeyTexFontA_00`, ..., and the Switch button whose line they label.
const LETTERS: [(&str, u8); 4] = [
    ("KeyTexFontA_00", 0),
    ("KeyTexFontB_00", 1),
    ("KeyTexFontX_00", 2),
    ("KeyTexFontY_00", 3),
];

/// BC4: a block's lightest and darkest, then the 16 three-bit levels between
/// (`nx2/prompt_assets.py`'s `encode_bc4`).
fn encode_bc4_block(values: &[u8; 16]) -> [u8; 8] {
    let hi = *values.iter().max().unwrap();
    let lo = *values.iter().min().unwrap();
    let mut index = 0u64;
    for (i, value) in values.iter().enumerate() {
        let code = if hi > lo {
            let t = ((u64::from(*value - lo)) * 7 + u64::from(hi - lo) / 2) / u64::from(hi - lo);
            match t {
                7 => 0,
                0 => 1,
                other => 8 - other,
            }
        } else {
            7
        };
        index |= code << (i * 3);
    }
    let mut out = [0u8; 8];
    out[0] = hi;
    out[1] = lo;
    out[2..].copy_from_slice(&index.to_le_bytes()[..6]);
    out
}

fn encode_bc4(mask: &Mask) -> Vec<u8> {
    let bw = mask.width.div_ceil(4);
    let bh = mask.height.div_ceil(4);
    let mut out = Vec::with_capacity((bw * bh * 8) as usize);
    for by in 0..bh {
        for bx in 0..bw {
            let mut values = [0u8; 16];
            for y in 0..4 {
                for x in 0..4 {
                    let px = bx * 4 + x;
                    let py = by * 4 + y;
                    values[(y * 4 + x) as usize] = if px < mask.width && py < mask.height {
                        mask.data[(py * mask.width + px) as usize]
                    } else {
                        0
                    };
                }
            }
            out.extend_from_slice(&encode_bc4_block(&values));
        }
    }
    out
}

/// The blocks of `mask` in the file's order, as (offset, bytes), adjacent
/// blocks joined into one run.
fn runs(texture: &Texture, mask: &Mask) -> Vec<(u32, Vec<u8>)> {
    let blocks = encode_bc4(mask);
    let (bw, bh) = texture.blocks();
    let pitch = texture.pitch();
    let size = texture.bytes_per_block();
    let mut pieces = Vec::new();
    for by in 0..bh {
        for bx in 0..bw {
            let at = texture.data
                + bntx::gob_offset(
                    u64::from(bx) * size as u64,
                    u64::from(by),
                    u64::from(texture.gobs),
                    u64::from(pitch),
                );
            let from = ((by * bw + bx) as usize) * size;
            pieces.push((at, blocks[from..from + size].to_vec()));
        }
    }
    let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
    for (at, piece) in pieces {
        if let Some(last) = out.last_mut()
            && last.0 as u64 + last.1.len() as u64 == at
        {
            last.1.extend_from_slice(&piece);
        } else {
            out.push((at as u32, piece));
        }
    }
    out
}

fn write_textures(path: &Path, entries: &[(u8, u8, Vec<(u32, Vec<u8>)>)]) -> Result<(), String> {
    let mut out = Vec::new();
    out.extend_from_slice(b"HPTX");
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (button, pad, runs) in entries {
        out.push(*button as u8);
        out.push(*pad);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(runs.len() as u32).to_le_bytes());
        for (offset, bytes) in runs {
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(bytes);
        }
    }
    fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

fn icon_glyph(font: &Face, spec: &faces::Face, pad: &str) -> Option<GlyphId> {
    let raw = spec.icons.iter().find(|(input, _)| *input == pad)?.1;
    let name = match raw.strip_prefix('/') {
        Some(name) => name.to_owned(),
        None => format!("{}{raw}", spec.prefix),
    };
    (0..font.number_of_glyphs())
        .map(GlyphId)
        .find(|gid| font.glyph_name(*gid) == Some(name.as_str()))
}

/// Write `Common.bntx` and each face's `textures.bin`; returns the entries
/// written.
pub fn build(romfs: &Path, out: &Path, dictionary: &[u8], kenney: &Path) -> Result<usize, String> {
    let bytes = zs::read_zs(romfs, bntx::TEXTURES, dictionary)?;
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let common = out.join("Common.bntx");
    fs::write(&common, &bytes).map_err(|e| format!("{}: {e}", common.display()))?;
    let textures = bntx::parse(&bytes)?;
    let mut written = 0usize;
    for spec in &faces::FACES {
        let path = kenney.join(spec.font);
        let font_bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let font = match Face::parse(&font_bytes, 0) {
            Ok(font) => font,
            Err(_) => continue,
        };
        let mut entries: Vec<(u8, u8, Vec<(u32, Vec<u8>)>)> = Vec::new();
        for (name, button) in LETTERS {
            let Some(texture) = textures.get(name) else {
                continue;
            };
            if texture.is_astc() {
                continue;
            }
            for (pad_index, pad) in faces::PADS.iter().enumerate() {
                let Some(gid) = icon_glyph(&font, spec, pad) else {
                    continue;
                };
                let size = ((texture.width.min(texture.height) as f64) * 0.9).round() as u32;
                let icon = match raster::glyph(&font, gid, size.max(1)) {
                    Ok(icon) if icon.any() => icon,
                    _ => continue,
                };
                let holes = icon.holes();
                let mark = if holes.any() { holes } else { icon };
                let mask = mark.place(texture.width, texture.height);
                entries.push((button, pad_index as u8, runs(texture, &mask)));
                written += 1;
            }
        }
        if entries.is_empty() {
            continue;
        }
        let dir = out.join(spec.name);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write_textures(&dir.join("textures.bin"), &entries)?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc4_blocks_are_six_levels_between_the_extremes() {
        assert_eq!(
            encode_bc4_block(&[0; 16]),
            [0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
        let mut values = [0u8; 16];
        for (i, value) in values.iter_mut().enumerate() {
            *value = i as u8;
        }
        assert_eq!(
            encode_bc4_block(&values),
            [15, 0, 201, 111, 183, 228, 38, 1]
        );
    }

    #[test]
    fn the_header_and_entries_round_trip() {
        let base = std::env::temp_dir().join(format!("hookshot-textures-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let path = base.join("textures.bin");
        let runs = vec![(0x100, vec![1, 2, 3]), (0x200, vec![4, 5])];
        write_textures(&path, &[(0, 3, runs)]).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"HPTX");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
        assert_eq!(bytes[8], 0);
        assert_eq!(bytes[9], 3);
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 0x100);
        assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 3);
        let _ = fs::remove_dir_all(base);
    }
}
