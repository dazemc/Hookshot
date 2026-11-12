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

use super::astc;
use super::bntx::{self, Texture};
use super::raster::{self, Mask};
use super::{faces, zs};

/// The pause menu's controller picture and its tinted copy.
const PICTURES: [&str; 2] = ["Controller_Pro", "Controller_Pro1"];
/// The face buttons' centres and radius in the picture.
const PICTURE_BUTTONS: [(&str, u32, u32, u32); 4] = [
    ("east", 869, 227, 31),
    ("south", 800, 286, 31),
    ("north", 800, 168, 31),
    ("west", 732, 227, 31),
];

/// The note textures, and the Switch button whose line they label.
const NOTES: [(&str, u8); 6] = [
    ("OcarinaButtonA_00", 0),
    ("OcarinaButtonB_00", 1),
    ("OcarinaButtonX_00", 2),
    ("OcarinaButtonY_00", 3),
    ("OcarinaButtonL_00", 6),
    ("OcarinaButtonR_00", 7),
];

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

/// Encoded blocks in the file's order, as (offset, bytes), adjacent blocks
/// joined into one run.
fn runs_blocks(texture: &Texture, blocks: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let (bw, bh) = texture.blocks();
    let pitch = texture.pitch();
    let size = texture.bytes_per_block();
    if blocks.len() != (bw * bh) as usize * size {
        return Vec::new();
    }
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

fn runs(texture: &Texture, mask: &Mask) -> Vec<(u32, Vec<u8>)> {
    runs_blocks(texture, &encode_bc4(mask))
}

fn draw_button(rgba: &mut [u8], width: u32, height: u32, cx: u32, cy: u32, r: u32, mark: &Mask) {
    let sample_x = (cx + r).saturating_sub(6).min(width - 1);
    let sample_y = cy.min(height - 1);
    let at = ((sample_y * width + sample_x) * 4) as usize;
    let fill = [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]];
    for y in cy.saturating_sub(r)..=(cy + r).min(height - 1) {
        for x in cx.saturating_sub(r)..=(cx + r).min(width - 1) {
            let dx = x as i64 - cx as i64;
            let dy = y as i64 - cy as i64;
            if dx * dx + dy * dy <= (r * r) as i64 {
                let at = ((y * width + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&fill);
            }
        }
    }
    let x0 = cx.saturating_sub(mark.width / 2);
    let y0 = cy.saturating_sub(mark.height / 2);
    for y in 0..mark.height {
        for x in 0..mark.width {
            if mark.at(x, y) > 127 {
                let px = x0 + x;
                let py = y0 + y;
                if px < width && py < height {
                    let at = ((py * width + px) * 4) as usize;
                    rgba[at..at + 4].copy_from_slice(&[205, 205, 205, 255]);
                }
            }
        }
    }
}

fn controller_layers(
    bytes: &[u8],
    textures: &BTreeMap<String, Texture>,
    font: &Face,
    spec: &faces::Face,
) -> Vec<(u8, u8, Vec<(u32, Vec<u8>)>)> {
    let mut entries = Vec::new();
    for name in PICTURES {
        let Some(texture) = textures.get(name) else {
            continue;
        };
        if !texture.is_astc() {
            continue;
        }
        let blocks = bntx::blocks_linear(bytes, texture);
        let Ok(mut rgba) = astc::decode(
            &blocks,
            texture.width,
            texture.height,
            (texture.format & 0xff) == 6,
        ) else {
            continue;
        };
        for (pad, cx, cy, r) in PICTURE_BUTTONS {
            let Some(gid) = icon_glyph(font, spec, pad) else {
                continue;
            };
            let size = ((2 * r) as f64 * 0.92).round() as u32;
            let Ok(icon) = raster::glyph(font, gid, size.max(1)) else {
                continue;
            };
            let holes = icon.holes();
            let mark = if holes.any() { holes } else { icon };
            draw_button(&mut rgba, texture.width, texture.height, cx, cy, r, &mark);
        }
        let Ok(blocks) = astc::encode(
            &rgba,
            texture.width,
            texture.height,
            (texture.format & 0xff) == 6,
        ) else {
            continue;
        };
        entries.push((0xff, 0, runs_blocks(texture, &blocks)));
    }
    entries
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
        for (name, button) in NOTES {
            let Some(texture) = textures.get(name) else {
                continue;
            };
            if !texture.is_astc() {
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
                let shape = icon.shape();
                let shape = shape.place(texture.width, texture.height);
                let holes = holes.place(texture.width, texture.height);
                let mut rgba = vec![0u8; (texture.width * texture.height * 4) as usize];
                for y in 0..texture.height {
                    for x in 0..texture.width {
                        let at = ((y * texture.width + x) * 4) as usize;
                        if holes.at(x, y) > 127 {
                            rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
                        } else if shape.at(x, y) > 127 {
                            rgba[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                        }
                    }
                }
                let Ok(blocks) = astc::encode(
                    &rgba,
                    texture.width,
                    texture.height,
                    (texture.format & 0xff) == 6,
                ) else {
                    continue;
                };
                entries.push((button, pad_index as u8, runs_blocks(texture, &blocks)));
                written += 1;
            }
        }
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
        entries.extend(controller_layers(&bytes, &textures, &font, spec));
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
    fn a_button_gets_its_disc_and_label() {
        let mut rgba = vec![10u8; 32 * 32 * 4];
        let mut mark = Mask::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                mark.set(x, y, 255);
            }
        }
        draw_button(&mut rgba, 32, 32, 16, 16, 8, &mark);
        let at = (16 * 32 + 16) * 4;
        assert_eq!(&rgba[at..at + 4], &[205, 205, 205, 255]);
        let edge = (16 * 32 + 7) * 4; // (outside the radius)
        assert_eq!(&rgba[edge..edge + 3], &[10, 10, 10]);
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
