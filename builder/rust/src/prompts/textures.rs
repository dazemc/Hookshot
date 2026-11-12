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
use super::guide;
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
            let t = (f64::from(*value - lo) * 7.0 / f64::from(hi - lo)).round_ties_even() as u64;
            match t {
                7 => 0,
                0 => 1,
                other => 8 - other,
            }
        } else {
            0
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

/// The file's bytes for the blocks of `mask` inside `bbox` (pixels; all of it
/// by default), or those with any of `cover` in them, as (offset, bytes) runs
/// (`runs` of `nx2/prompt_assets.py`).
fn runs_area(
    texture: &Texture,
    mask: &Mask,
    bbox: Option<guide::Bbox>,
    cover: Option<&Mask>,
) -> Vec<(u32, Vec<u8>)> {
    let (bw, bh) = texture.blocks();
    let size = texture.bytes_per_block();
    if size != 8 {
        return Vec::new();
    }
    if cover.is_some_and(|cover| cover.bbox().is_none()) {
        return Vec::new();
    }
    let area = bbox.or_else(|| cover.and_then(Mask::bbox)).unwrap_or((
        0,
        0,
        texture.width,
        texture.height,
    ));
    let bx0 = (area.0 / 4).min(bw);
    let by0 = (area.1 / 4).min(bh);
    let bx1 = (area.2.saturating_add(3) / 4).min(bw);
    let by1 = (area.3.saturating_add(3) / 4).min(bh);
    if bx1 <= bx0 || by1 <= by0 {
        return Vec::new();
    }
    let pw = bx1 - bx0;
    let mut part = Mask::new(pw * 4, (by1 - by0) * 4);
    for y in 0..part.height {
        for x in 0..part.width {
            let px = bx0 * 4 + x;
            let py = by0 * 4 + y;
            if px < mask.width && py < mask.height {
                part.set(x, y, mask.at(px, py));
            }
        }
    }
    let blocks = encode_bc4(&part);
    let mut pieces = Vec::new();
    for y in by0..by1 {
        for x in bx0..bx1 {
            if let Some(cover) = cover {
                let mut covered = false;
                'block: for py in y * 4..y * 4 + 4 {
                    for px in x * 4..x * 4 + 4 {
                        if px < cover.width && py < cover.height && cover.at(px, py) != 0 {
                            covered = true;
                            break 'block;
                        }
                    }
                }
                if !covered {
                    continue;
                }
            }
            let at = texture.data
                + bntx::gob_offset(
                    u64::from(x) * size as u64,
                    u64::from(y),
                    u64::from(texture.gobs),
                    u64::from(texture.pitch()),
                );
            let from = (((y - by0) * pw + (x - bx0)) as usize) * size;
            pieces.push((at, blocks[from..from + size].to_vec()));
        }
    }
    pieces.sort_by_key(|(at, _)| *at);
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

/// Each face's leader-line entries, in `textures.bin`'s order: the lines that
/// stay first (Switch button 0xff), then each moving line over them.
fn guide_entries(
    bytes: &[u8],
    textures: &BTreeMap<String, Texture>,
    face: &str,
) -> Vec<(u8, u8, Vec<(u32, Vec<u8>)>)> {
    let Some(first) = textures.get(guide::GUIDES[0]) else {
        return Vec::new();
    };
    let Some(second) = textures.get(guide::GUIDES[1]) else {
        return Vec::new();
    };
    if first.is_astc() || second.is_astc() {
        return Vec::new();
    }
    let game = [
        guide::decode_bc4(bytes, first),
        guide::decode_bc4(bytes, second),
    ];
    if game[0].width != game[1].width || game[0].height != game[1].height {
        return Vec::new();
    }
    let textures = [first, second];
    let mut keep: [Option<Mask>; 2] = [None, None];
    let mut entries = Vec::new();
    for layer in guide::layers([&game[0], &game[1]], face) {
        match layer {
            guide::Layer::Keep {
                texture,
                mask,
                bbox,
            } => {
                entries.push((0xff, 0, runs_area(textures[texture], &mask, bbox, None)));
                keep[texture] = Some(mask);
            }
            guide::Layer::Over {
                texture,
                button,
                pad,
                mask,
            } => {
                let under = keep[texture].as_ref().unwrap_or(&game[texture]);
                let combined = under.lighter(&mask);
                entries.push((
                    button,
                    pad,
                    runs_area(textures[texture], &combined, None, Some(&mask)),
                ));
            }
        }
    }
    entries
}

fn draw_button(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    cx: u32,
    cy: u32,
    r: u32,
    mark: &Mask,
) -> Option<(u32, u32, u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut changed: Option<(u32, u32, u32, u32)> = None;
    let mut include = |x0: u32, y0: u32, x1: u32, y1: u32| {
        let x0 = x0.min(width);
        let y0 = y0.min(height);
        let x1 = x1.min(width);
        let y1 = y1.min(height);
        if x1 > x0 && y1 > y0 {
            changed = Some(match changed {
                None => (x0, y0, x1, y1),
                Some((ax0, ay0, ax1, ay1)) => (ax0.min(x0), ay0.min(y0), ax1.max(x1), ay1.max(y1)),
            });
        }
    };
    include(
        cx.saturating_sub(r),
        cy.saturating_sub(r),
        cx.saturating_add(r).saturating_add(1),
        cy.saturating_add(r).saturating_add(1),
    );
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
    include(
        x0,
        y0,
        x0.saturating_add(mark.width),
        y0.saturating_add(mark.height),
    );
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
    changed
}

/// The source blocks with only the changed pixels re-encoded. ASTC blocks are
/// independent, so the rest of the game's picture keeps its exact blocks;
/// re-encoding the whole 1160x680 texture for four small button marks was most
/// of the prompt build's texture time.
fn encode_changed(
    rgba: &[u8],
    width: u32,
    height: u32,
    srgb: bool,
    mut blocks: Vec<u8>,
    changed: Option<(u32, u32, u32, u32)>,
) -> Result<Vec<u8>, String> {
    let Some((x0, y0, x1, y1)) = changed else {
        return Ok(blocks);
    };
    let bw = width.div_ceil(4);
    let bh = height.div_ceil(4);
    let bx0 = x0 / 4;
    let by0 = y0 / 4;
    let bx1 = x1.div_ceil(4).min(bw);
    let by1 = y1.div_ceil(4).min(bh);
    if bx0 >= bx1 || by0 >= by1 {
        return Ok(blocks);
    }
    let px0 = bx0 * 4;
    let py0 = by0 * 4;
    let px1 = (bx1 * 4).min(width);
    let py1 = (by1 * 4).min(height);
    let cw = px1 - px0;
    let ch = py1 - py0;
    let mut crop = Vec::with_capacity((cw * ch * 4) as usize);
    for y in py0..py1 {
        let at = ((y * width + px0) * 4) as usize;
        crop.extend_from_slice(&rgba[at..at + (cw * 4) as usize]);
    }
    let part = astc::encode(&crop, cw, ch, srgb)?;
    let cbw = cw.div_ceil(4);
    let cbh = ch.div_ceil(4);
    for cy in 0..cbh {
        for cx in 0..cbw {
            let from = ((cy * cbw + cx) as usize) * 16;
            let to = (((by0 + cy) * bw + bx0 + cx) as usize) * 16;
            blocks[to..to + 16].copy_from_slice(&part[from..from + 16]);
        }
    }
    Ok(blocks)
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
        let mut changed: Option<(u32, u32, u32, u32)> = None;
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
            if let Some(rect) =
                draw_button(&mut rgba, texture.width, texture.height, cx, cy, r, &mark)
            {
                changed = Some(match changed {
                    None => rect,
                    Some((ax0, ay0, ax1, ay1)) => (
                        ax0.min(rect.0),
                        ay0.min(rect.1),
                        ax1.max(rect.2),
                        ay1.max(rect.3),
                    ),
                });
            }
        }
        let Ok(blocks) = encode_changed(
            &rgba,
            texture.width,
            texture.height,
            (texture.format & 0xff) == 6,
            blocks,
            changed,
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
        let guides = guide_entries(&bytes, &textures, spec.name);
        written += guides
            .iter()
            .filter(|(button, _, _)| *button != 0xff)
            .count();
        entries.extend(guides);
        if entries.is_empty() {
            continue;
        }
        let dir = out.join(spec.name);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write_textures(&dir.join("textures.bin"), &entries)?;
    }
    let switch_guides = guide_entries(&bytes, &textures, "switch");
    if !switch_guides.is_empty() {
        written += switch_guides
            .iter()
            .filter(|(button, _, _)| *button != 0xff)
            .count();
        let dir = out.join("switch");
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write_textures(&dir.join("textures.bin"), &switch_guides)?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc4_blocks_are_six_levels_between_the_extremes() {
        assert_eq!(encode_bc4_block(&[0; 16]), [0, 0, 0, 0, 0, 0, 0, 0]);
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
    fn only_the_changed_blocks_are_encoded_again() {
        let (width, height) = (38u32, 22u32);
        let mut rgba: Vec<u8> = (0..width * height * 4)
            .map(|i| ((i * 37 + i / 7) & 0xff) as u8)
            .collect();
        let source = astc::encode(&rgba, width, height, false).unwrap();
        assert_eq!(
            encode_changed(&rgba, width, height, false, source.clone(), None).unwrap(),
            source
        );
        for y in 8..20 {
            for x in 8..32 {
                let at = ((y * width + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[255, 0, 128, 255]);
            }
        }
        let full = astc::encode(&rgba, width, height, false).unwrap();
        let spliced =
            encode_changed(&rgba, width, height, false, source, Some((8, 8, 32, 20))).unwrap();
        // (ASTC blocks are independent: re-encoding the changed region over
        // the source's blocks is the whole image's encoding)
        assert_eq!(spliced, full);
    }

    #[test]
    fn bc4_rounding_matches_the_reference() {
        let cases: [([u8; 16], [u8; 8]); 7] = [
            (
                [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 14],
                [14, 0, 201, 109, 151, 228, 36, 1],
            ),
            (
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 14],
                [14, 0, 73, 146, 36, 73, 146, 4],
            ),
            (
                [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
                [15, 0, 201, 111, 183, 228, 38, 1],
            ),
            (
                [
                    10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
                ],
                [25, 10, 201, 111, 183, 228, 38, 1],
            ),
            (
                [0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30],
                [30, 0, 201, 111, 183, 228, 38, 1],
            ),
            (
                [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3],
                [3, 0, 73, 98, 219, 219, 6, 0],
            ),
            (
                [5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5],
                [5, 5, 0, 0, 0, 0, 0, 0],
            ),
        ];
        for (values, expected) in cases {
            assert_eq!(encode_bc4_block(&values), expected, "{values:?}");
        }
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

    fn test_texture(width: u32, height: u32) -> Texture {
        Texture {
            name: "ControllerGuide_Pro_00".to_owned(),
            record: 0,
            format: 0,
            width,
            height,
            gobs: 1,
            data: 0,
        }
    }

    #[test]
    fn runs_take_the_blocks_in_the_box_or_under_the_mask() {
        let texture = test_texture(64, 64);
        let mut mask = Mask::new(64, 64);
        for y in 0..4 {
            for x in 0..8 {
                mask.set(x, y, 255);
            }
        }
        // (two blocks next to each other in the file: one run)
        let runs = runs_area(&texture, &mask, Some((0, 0, 8, 4)), None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, 0);
        assert_eq!(runs[0].1.len(), 16);

        let mut boxed = Mask::new(64, 64);
        for y in 0..16 {
            for x in 0..16 {
                boxed.set(x, y, 255);
            }
        }
        let runs = runs_area(&texture, &boxed, Some((0, 0, 16, 16)), None);
        assert_eq!(
            runs.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
            128
        );
        assert!(runs.windows(2).all(|pair| pair[0].0 < pair[1].0));

        // (only the block the line reaches is written, not the rest of its box)
        let mut cover = Mask::new(64, 64);
        for y in 32..36 {
            for x in 32..36 {
                cover.set(x, y, 255);
            }
        }
        let runs = runs_area(&texture, &cover, None, Some(&cover));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, bntx::gob_offset(64, 8, 1, 2) as u32);
        assert_eq!(runs[0].1.len(), 8);

        let empty = Mask::new(64, 64);
        assert!(runs_area(&texture, &empty, None, Some(&empty)).is_empty());
    }

    #[test]
    fn entries_are_written_in_the_order_the_controls_need() {
        let base = std::env::temp_dir().join(format!("hookshot-guides-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let path = base.join("textures.bin");
        write_textures(
            &path,
            &[
                (0xff, 0, vec![(0x100, vec![1, 2])]),
                (8, 7, vec![(0x200, vec![3, 4, 5]), (0x300, vec![6])]),
            ],
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"HPTX");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
        // (the lines that stay first, Switch button 0xff, pad 0)
        assert_eq!(bytes[8], 0xff);
        assert_eq!(bytes[9], 0);
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 0x100);
        assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 2);
        assert_eq!(&bytes[24..26], &[1, 2]);
        // (then the moving line: Switch button 8, pad input 7)
        assert_eq!(bytes[26], 8);
        assert_eq!(bytes[27], 7);
        assert_eq!(u32::from_le_bytes(bytes[30..34].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[34..38].try_into().unwrap()), 0x200);
        assert_eq!(u32::from_le_bytes(bytes[38..42].try_into().unwrap()), 3);
        assert_eq!(&bytes[42..45], &[3, 4, 5]);
        assert_eq!(u32::from_le_bytes(bytes[45..49].try_into().unwrap()), 0x300);
        assert_eq!(u32::from_le_bytes(bytes[49..53].try_into().unwrap()), 1);
        assert_eq!(bytes[53], 6);
        let _ = fs::remove_dir_all(base);
    }
}
