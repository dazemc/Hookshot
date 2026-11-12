//! Per-face `textures.bin`: the blocks written over the game's button-icon
//! textures (`host/prompts.c` applies the runs the controls choose).
//!
//! The BC4 letters, the ASTC ocarina notes, each non-Switch face's drawn
//! controller picture with the page's tinted copy and shadow, and the pause
//! menu's leader lines are all made here, as `nx2/prompt_assets.py` does.
//! Pillow-compatible shapes, blur and resampling are in `raster`; glyph
//! silhouettes are scanline-rasterized, so their antialiased edges can differ
//! from FreeType's (the real pictures differ from the Python reference on
//! 0.06% of RGBA bytes for the Steam Deck to 0.46% for the Xbox).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ttf_parser::{Face, GlyphId};

use super::astc;
use super::bntx::{self, Texture};
use super::guide;
use super::raster::{self, Filter, Mask, Rgba};
use super::{faces, zs};

/// The pause menu's controller picture and its tinted copy.
const PICTURES: [&str; 2] = ["Controller_Pro", "Controller_Pro1"];
/// The picture's shadow (`nx2/prompt_assets.py`'s `PICTURE_SHADOW`).
const PICTURE_SHADOW: &str = "ControllerSh_Pro1";
/// The drawing's colours and scale (`nx2/prompt_draw.py`).
const BODY: [u8; 4] = [51, 51, 51, 255];
const PART: [u8; 4] = [100, 100, 100, 255];
const INK: [u8; 4] = [205, 205, 205, 255];
const EDGE: [u8; 4] = [0, 0, 0, 255];
const SCREEN: [u8; 4] = [27, 27, 27, 255];
const SCREEN_INNER: [u8; 4] = [77, 77, 77, 255];
/// The Pro Controller's stick and d-pad (centre, half size in its pixels).
const STICK: (u32, u32, u32) = (347, 228, 74);
const DPAD: (u32, u32, u32) = (453, 348, 70);
/// The box a Kenney body is fitted in, and the Steam Deck's own box.
const BOX: (f64, f64, f64, f64) = (151.0, 43.0, 1009.0, 651.0);
const DECK_BOX: (f64, f64, f64, f64) = (70.0, 105.0, 1088.0, 573.0);
/// The drawing's scale before the picture's size.
const SS: u32 = 4;
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
            let mask = mark.at(x, y);
            if mask == 0 {
                continue;
            }
            let px = x0 + x;
            let py = y0 + y;
            if px < width && py < height {
                let at = ((py * width + px) * 4) as usize;
                for channel in 0..4 {
                    let mut channel_mask = mask;
                    if channel != 3 && channel_mask != 0 {
                        channel_mask =
                            (255 - (255 - channel_mask) * (1 - (255 - rgba[at + 3]) / 255)) as u8;
                    }
                    rgba[at + channel] =
                        raster::blend(channel_mask, rgba[at + channel], INK[channel]);
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

/// A face's icon for each pad input (`prompt_assets.Icons`): the Kenney glyphs
/// of that face, or the game's own icon font for the Switch.
struct Icons<'a> {
    font: &'a Face<'a>,
    codes: BTreeMap<&'static str, GlyphId>,
}

impl<'a> Icons<'a> {
    fn for_face(font: &'a Face<'a>, spec: &faces::Face) -> Self {
        let mut codes = BTreeMap::new();
        for pad in faces::PADS {
            if let Some(gid) = icon_glyph(font, spec, pad) {
                codes.insert(pad, gid);
            }
        }
        Self { font, codes }
    }

    fn for_game(font: &'a Face<'a>) -> Self {
        let mut codes = BTreeMap::new();
        for (pad, code) in faces::SWITCH_ICON {
            if let Some(character) = char::from_u32(u32::from(code))
                && let Some(gid) = font.glyph_index(character)
            {
                codes.insert(pad, gid);
            }
        }
        Self { font, codes }
    }

    /// `Icons.masks`: (the icon, its silhouette with holes filled, the holes).
    fn masks(&self, pad: &str, size: u32) -> Option<(Mask, Mask, Mask)> {
        let gid = *self.codes.get(pad)?;
        raster::icon_masks(self.font, gid, size).ok()
    }
}

fn sprite(pro: &Rgba, cx: u32, cy: u32, r: u32) -> Rgba {
    pro.crop(
        i64::from(cx) - i64::from(r),
        i64::from(cy) - i64::from(r),
        i64::from(cx) + i64::from(r) + 1,
        i64::from(cy) + i64::from(r) + 1,
    )
}

/// A button of the Pro, without its label (`prompt_draw.blank_disc`).
fn blank_disc(r: u32, k: u32) -> Rgba {
    let m = 3 * k;
    let size = 2 * r + 2 * m + 1;
    let mut im = Rgba::new(size, size);
    let (x0, x1) = (i64::from(m), i64::from(2 * r + m));
    im.fill_ellipse(x0, x0, x1, x1, PART);
    im.outline_ellipse(x0, x0, x1, x1, i64::from(m), EDGE);
    im
}

/// An L mask grown by `d` pixels all round (`prompt_draw.grown`).
fn grown(mask: &Mask, d: f64) -> Mask {
    mask.gaussian_blur(d / 2.0)
        .point(|value| if value > 6 { 255 } else { 0 })
}

fn silhouette(font: &Face, glyph: &str, h: u32) -> Option<Mask> {
    let gid = (0..font.number_of_glyphs())
        .map(GlyphId)
        .find(|gid| font.glyph_name(*gid) == Some(glyph))?;
    let mask = raster::glyph_pixels(font, gid, h * 2).ok()?;
    Some(
        mask.point(|value| if value > 127 { 255 } else { 0 })
            .shape(),
    )
}

/// The Steam Deck's body (`prompt_draw.deck`).
fn deck(w: u32) -> Mask {
    let h = raster::py_round(f64::from(w) / 2.55) as u32;
    let mut mask = Mask::new(w, h);
    let trunc = |value: f64| value as i64;
    mask.fill_rounded_rect(
        0.0,
        0.0,
        f64::from(w - 1),
        trunc(f64::from(h) * 0.92) as f64,
        trunc(f64::from(h) * 0.16) as f64,
        255,
    );
    mask.fill_ellipse(
        trunc(f64::from(w) * 0.01),
        trunc(f64::from(h) * 0.30),
        trunc(f64::from(w) * 0.27),
        i64::from(h) - 1,
        255,
    );
    mask.fill_ellipse(
        trunc(f64::from(w) * 0.73),
        trunc(f64::from(h) * 0.30),
        trunc(f64::from(w) * 0.99),
        i64::from(h) - 1,
        255,
    );
    mask.fill_rect(
        trunc(f64::from(w) * 0.14),
        trunc(f64::from(h) * 0.5),
        trunc(f64::from(w) * 0.86),
        trunc(f64::from(h) * 0.92),
        255,
    );
    mask
}

/// The face's controller picture, drawn like the game's Pro Controller
/// (`prompt_draw.picture`), or None for a face without a drawing.
fn picture(spec: &faces::Face, pro: &Rgba, font: &Face, game_font: Option<&Face>) -> Option<Rgba> {
    picture_scaled(spec, pro, font, game_font, SS, None)
}

fn picture_scaled(
    spec: &faces::Face,
    pro: &Rgba,
    font: &Face,
    game_font: Option<&Face>,
    ss: u32,
    given_silhouette: Option<&Mask>,
) -> Option<Rgba> {
    let drawing = spec.drawing?;
    let (width, height) = (pro.width * ss, pro.height * ss);
    let stick = sprite(pro, STICK.0, STICK.1, STICK.2);
    let mut dpad = sprite(pro, DPAD.0, DPAD.1, DPAD.2);
    let mut cross = Mask::new(dpad.width, dpad.height);
    cross.fill_rect(0, 42, 140, 98, 255);
    cross.fill_rect(44, 0, 100, 140, 255);
    dpad.multiply_alpha(&cross);
    let sil = match given_silhouette {
        Some(mask) => mask.clone(),
        None => match (drawing.font, drawing.glyph) {
            (Some(_), Some(glyph)) => silhouette(font, glyph, 600 * ss)?,
            _ => deck(1600 * ss),
        },
    };
    let box_ = if drawing.font.is_some() {
        BOX
    } else {
        DECK_BOX
    };
    let box_ = (
        box_.0 * f64::from(ss),
        box_.1 * f64::from(ss),
        box_.2 * f64::from(ss),
        box_.3 * f64::from(ss),
    );
    let scale = ((box_.2 - box_.0) / f64::from(sil.width))
        .min((box_.3 - box_.1) / f64::from(sil.height))
        * 0.97;
    let sil = sil
        .resize(
            raster::py_round(f64::from(sil.width) * scale).max(1.0) as u32,
            raster::py_round(f64::from(sil.height) * scale).max(1.0) as u32,
            Filter::Lanczos,
        )
        .point(|value| if value > 127 { 255 } else { 0 });
    let ox = (i64::from(width) - i64::from(sil.width)) / 2;
    let oy = box_.1 as i64 + ((box_.3 - box_.1 - f64::from(sil.height)) / 2.0).floor() as i64;
    let mut mask = Mask::new(width, height);
    mask.paste(&sil, ox, oy);
    let mut im = Rgba::new(width, height);
    if drawing.font.is_some() {
        let mut up = Mask::new(width, height);
        up.paste(&mask, 0, -(18 * ss as i64));
        let mut top = Mask::new(width, height);
        for c in [0.25, 0.75] {
            top.fill_ellipse(
                ox + (f64::from(sil.width) * (c - 0.17)) as i64,
                oy - (f64::from(sil.height) * 0.12) as i64,
                ox + (f64::from(sil.width) * (c + 0.17)) as i64,
                oy + (f64::from(sil.height) * 0.16) as i64,
                255,
            );
        }
        for control in drawing.controls {
            if control.kind == faces::ControlKind::Pad {
                let width = f64::from(sil.width);
                let half = control.size.width() / 2.0;
                top.fill_rect(
                    ox + (width * (control.cx - half - 0.03)) as i64,
                    0,
                    ox + (width * (control.cx + half + 0.03)) as i64,
                    i64::from(height),
                    0,
                );
            }
        }
        let bump = up
            .multiply(&top)
            .gaussian_blur(f64::from(ss))
            .point(|value| if value > 127 { 255 } else { 0 });
        im.paste_color(EDGE, &grown(&bump, f64::from(3 * ss)), 0, 0);
        im.paste_color(PART, &bump, 0, 0);
    }
    im.paste_color(EDGE, &grown(&mask, f64::from(3 * ss)), 0, 0);
    im.paste_color(BODY, &mask, 0, 0);
    let icons = Icons::for_face(font, spec);
    let game_icons = game_font.map(Icons::for_game);
    for control in drawing.controls {
        let x = ox as f64 + control.cx * f64::from(sil.width);
        let y = oy as f64 + control.cy * f64::from(sil.height);
        match control.kind {
            faces::ControlKind::Stick | faces::ControlKind::Dpad => {
                let r = raster::py_round(control.size.width() * f64::from(sil.width) / 2.0);
                let r = r.max(0.0) as i64;
                let source = if control.kind == faces::ControlKind::Stick {
                    &stick
                } else {
                    &dpad
                };
                let resized =
                    source.resize((2 * r + 1) as u32, (2 * r + 1) as u32, Filter::Lanczos);
                im.alpha_composite(
                    &resized,
                    raster::py_round(x) as i64 - r,
                    raster::py_round(y) as i64 - r,
                );
            }
            faces::ControlKind::Screen => {
                let w2 = control.size.width() * f64::from(sil.width) / 2.0;
                let h2 = control.size.height() * f64::from(sil.height) / 2.0;
                im.fill_rounded_rect(x - w2, y - h2, x + w2, y + h2, f64::from(8 * ss), SCREEN);
                im.outline_rounded_rect(
                    x - w2,
                    y - h2,
                    x + w2,
                    y + h2,
                    f64::from(8 * ss),
                    i64::from(3 * ss),
                    EDGE,
                );
                let inner = f64::from(22 * ss);
                im.fill_rect(
                    (x - w2 + inner) as i64,
                    (y - h2 + inner) as i64,
                    (x + w2 - inner) as i64,
                    (y + h2 - inner) as i64,
                    SCREEN_INNER,
                );
                im.outline_rect(
                    (x - w2 + inner) as i64,
                    (y - h2 + inner) as i64,
                    (x + w2 - inner) as i64,
                    (y + h2 - inner) as i64,
                    i64::from(2 * ss),
                    EDGE,
                );
            }
            faces::ControlKind::Pad => {
                let w2 = control.size.width() * f64::from(sil.width) / 2.0;
                let h2 = control.size.height() * f64::from(sil.height) / 2.0;
                let radius = raster::py_round(w2.min(h2) * 0.35);
                im.fill_rounded_rect(x - w2, y - h2, x + w2, y + h2, radius, PART);
                im.outline_rounded_rect(
                    x - w2,
                    y - h2,
                    x + w2,
                    y + h2,
                    radius,
                    i64::from(3 * ss),
                    EDGE,
                );
            }
            faces::ControlKind::Face | faces::ControlKind::Small => {
                let r = raster::py_round(control.size.width() * f64::from(sil.width) / 2.0).max(0.0)
                    as u32;
                let disc = blank_disc(r, ss);
                im.alpha_composite(
                    &disc,
                    raster::py_round(x) as i64 - i64::from(r) - i64::from(3 * ss),
                    raster::py_round(y) as i64 - i64::from(r) - i64::from(3 * ss),
                );
                let Some(what) = control.what else {
                    continue;
                };
                let set = if what == "guide" {
                    game_icons.as_ref()
                } else {
                    Some(&icons)
                };
                let Some(set) = set else {
                    continue;
                };
                let factor = if control.kind == faces::ControlKind::Face {
                    0.92
                } else {
                    0.80
                };
                let n = (2.0 * f64::from(r) * factor) as u32;
                if n == 0 {
                    continue;
                }
                let Some((_, shape, holes)) = set.masks(what, n) else {
                    continue;
                };
                let total: u64 = shape.data.iter().map(|value| u64::from(*value)).sum();
                let full = total as f64 / 255.0 / f64::from(n) / f64::from(n) > 0.7;
                let mark = if full {
                    holes
                } else {
                    let small = if control.kind == faces::ControlKind::Face {
                        (f64::from(n) * 0.7) as u32
                    } else {
                        (f64::from(n) * 0.85) as u32
                    };
                    set.masks(what, small).map(|(icon, _, _)| icon)?
                };
                im.paste_color(
                    INK,
                    &mark,
                    raster::py_round(x) as i64 - i64::from(mark.width / 2),
                    raster::py_round(y) as i64 - i64::from(mark.height / 2),
                );
            }
        }
    }
    Some(im.resize(pro.width, pro.height, Filter::Box))
}

fn controller_layers(
    bytes: &[u8],
    textures: &BTreeMap<String, Texture>,
    font: &Face,
    spec: &faces::Face,
) -> Vec<(u8, u8, Vec<(u32, Vec<u8>)>)> {
    if spec.name == "switch" {
        return Vec::new();
    }
    let icons = Icons::for_face(font, spec);
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
            let size = ((2 * r) as f64 * 0.92) as u32;
            let Some((icon, shape, holes)) = icons.masks(pad, size.max(1)) else {
                continue;
            };
            let total: u64 = shape.data.iter().map(|value| u64::from(*value)).sum();
            let full = total as f64 / 255.0 / f64::from(size.max(1)) / f64::from(size.max(1)) > 0.7;
            let mark = if full {
                holes
            } else {
                icons
                    .masks(pad, ((size.max(1) as f64) * 0.7) as u32)
                    .map(|(icon, _, _)| icon)
                    .unwrap_or(icon)
            };
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

fn decode_rgba(bytes: &[u8], texture: &Texture) -> Option<Rgba> {
    let blocks = bntx::blocks_linear(bytes, texture);
    let rgba = astc::decode(
        &blocks,
        texture.width,
        texture.height,
        (texture.format & 0xff) == 6,
    )
    .ok()?;
    Some(Rgba::from_rgba(rgba, texture.width, texture.height))
}

/// The page's tinted copy of a face's picture: each channel a line of the
/// picture's grey level, fitted on the game's own plain/tinted pair
/// (`prompt_assets.picture_layers`).
fn tint_layer(plain: &Rgba, tinted: &Rgba, picture: &Rgba) -> Option<Rgba> {
    if plain.width != tinted.width || plain.height != tinted.height {
        return None;
    }
    let mut count = 0.0f64;
    let mut scale_x = 0.0f64;
    let mut sum_x = 0.0f64;
    let mut sums = [(0.0f64, 0.0f64); 3];
    for y in 0..plain.height {
        for x in 0..plain.width {
            let pixel = plain.at(x, y);
            if pixel[3] <= 250 {
                continue;
            }
            let grey = f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2]);
            let target = tinted.at(x, y);
            count += 1.0;
            scale_x += grey * grey;
            sum_x += grey;
            for channel in 0..3 {
                sums[channel].0 += grey * f64::from(target[channel]);
                sums[channel].1 += f64::from(target[channel]);
            }
        }
    }
    if count < 2.0 {
        return None;
    }
    let scale_x = scale_x.sqrt();
    let scale_one = count.sqrt();
    if scale_x == 0.0 {
        return None;
    }
    let a01 = sum_x / (scale_x * scale_one);
    let determinant = 1.0 - a01 * a01;
    if determinant.abs() < 1e-18 {
        return None;
    }
    let mut lines = [(0.0f64, 0.0f64); 3];
    for (channel, (sum_xy, sum_y)) in sums.iter().enumerate() {
        let rhs_x = sum_xy / scale_x;
        let rhs_one = sum_y / scale_one;
        let a = (rhs_x - rhs_one * a01) / determinant;
        let b = (rhs_one - a01 * rhs_x) / determinant;
        lines[channel] = (a / scale_x, b / scale_one);
    }
    let mut out = Rgba::new(picture.width, picture.height);
    for y in 0..picture.height {
        for x in 0..picture.width {
            let pixel = picture.at(x, y);
            let level = f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2]);
            let mut value = [0u8; 4];
            for channel in 0..3 {
                let (slope, intercept) = lines[channel];
                value[channel] = (slope * level + intercept).clamp(0.0, 255.0) as u8;
            }
            value[3] = pixel[3];
            out.set(x, y, value);
        }
    }
    Some(out)
}

/// The page's shadow: the outline's left part squeezed into the shadow texture
/// and blurred, fitted on the game's (`prompt_assets.picture_layers`).
fn shadow_layer(picture: &Rgba, width: u32, height: u32) -> Mask {
    let alpha = picture.alpha();
    let cropped = alpha.crop(40, 0, 480, 680);
    let resized = cropped.resize(width, height, Filter::Bicubic);
    resized
        .gaussian_blur(24.0)
        .point(|value| raster::py_round(f64::from(value) * 0.7).clamp(0.0, 255.0) as u8)
}

fn picture_layers(
    bytes: &[u8],
    textures: &BTreeMap<String, Texture>,
    spec: &faces::Face,
    font: &Face,
    game_font: Option<&Face>,
) -> Option<Vec<(u8, u8, Vec<(u32, Vec<u8>)>)>> {
    if spec.drawing.is_none() {
        return None;
    }
    let plain_texture = textures.get(PICTURES[0])?;
    let tinted_texture = textures.get(PICTURES[1])?;
    let shadow_texture = textures.get(PICTURE_SHADOW)?;
    if !plain_texture.is_astc() || !tinted_texture.is_astc() || shadow_texture.is_astc() {
        return None;
    }
    let plain = decode_rgba(bytes, plain_texture)?;
    let tinted = decode_rgba(bytes, tinted_texture)?;
    let own = picture(spec, &plain, font, game_font)?;
    picture_entries(
        plain_texture,
        tinted_texture,
        shadow_texture,
        &plain,
        &tinted,
        &own,
    )
}

/// The three layers a drawn picture writes, in `textures.bin`'s order: the
/// picture, the page's tinted copy, then its shadow.
fn picture_entries(
    plain_texture: &Texture,
    tinted_texture: &Texture,
    shadow_texture: &Texture,
    plain: &Rgba,
    tinted: &Rgba,
    own: &Rgba,
) -> Option<Vec<(u8, u8, Vec<(u32, Vec<u8>)>)>> {
    let plain_blocks = astc::encode(
        &own.data,
        own.width,
        own.height,
        (plain_texture.format & 0xff) == 6,
    )
    .ok()?;
    let tint = tint_layer(plain, tinted, own)?;
    let tint_blocks = astc::encode(
        &tint.data,
        tint.width,
        tint.height,
        (tinted_texture.format & 0xff) == 6,
    )
    .ok()?;
    let shadow = shadow_layer(own, shadow_texture.width, shadow_texture.height);
    Some(vec![
        (0xff, 0, runs_blocks(plain_texture, &plain_blocks)),
        (0xff, 0, runs_blocks(tinted_texture, &tint_blocks)),
        (0xff, 0, runs_blocks(shadow_texture, &encode_bc4(&shadow))),
    ])
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
pub fn build(
    romfs: &Path,
    out: &Path,
    dictionary: &[u8],
    kenney: &Path,
    game_otf: &[u8],
) -> Result<usize, String> {
    let bytes = zs::read_zs(romfs, bntx::TEXTURES, dictionary)?;
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let common = out.join("Common.bntx");
    fs::write(&common, &bytes).map_err(|e| format!("{}: {e}", common.display()))?;
    let textures = bntx::parse(&bytes)?;
    let game_font = if game_otf.is_empty() {
        None
    } else {
        Face::parse(game_otf, 0).ok()
    };
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
        if spec.name != "switch" {
            match picture_layers(&bytes, &textures, spec, &font, game_font.as_ref()) {
                Some(layers) => {
                    written += layers.len();
                    entries.extend(layers);
                }
                None => entries.extend(controller_layers(&bytes, &textures, &font, spec)),
            }
        }
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

    fn fnv(data: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in data {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        hash
    }

    fn synthetic_pro() -> Rgba {
        let mut pro = Rgba::new(1160, 680);
        for y in 0..680 {
            for x in 0..1160 {
                pro.set(
                    x,
                    y,
                    [
                        ((x * 7 + y * 13) & 0xff) as u8,
                        ((x * 17 + y * 3) & 0xff) as u8,
                        ((x * 5 + y * 29) & 0xff) as u8,
                        255,
                    ],
                );
            }
        }
        pro
    }

    static TEST_CONTROLS: [faces::Control; 6] = [
        faces::Control {
            kind: faces::ControlKind::Stick,
            cx: 0.215,
            cy: 0.262,
            size: faces::Size::One(0.158),
            what: None,
        },
        faces::Control {
            kind: faces::ControlKind::Dpad,
            cx: 0.345,
            cy: 0.530,
            size: faces::Size::One(0.160),
            what: None,
        },
        faces::Control {
            kind: faces::ControlKind::Pad,
            cx: 0.6,
            cy: 0.2,
            size: faces::Size::Two(0.2, 0.15),
            what: None,
        },
        faces::Control {
            kind: faces::ControlKind::Screen,
            cx: 0.5,
            cy: 0.5,
            size: faces::Size::Two(0.4, 0.5),
            what: None,
        },
        faces::Control {
            kind: faces::ControlKind::Face,
            cx: 0.8,
            cy: 0.3,
            size: faces::Size::One(0.08),
            what: None,
        },
        faces::Control {
            kind: faces::ControlKind::Small,
            cx: 0.2,
            cy: 0.1,
            size: faces::Size::One(0.05),
            what: None,
        },
    ];
    static TEST_DRAWING: faces::Drawing = faces::Drawing {
        font: Some("kenney_input_xbox_series.otf"),
        glyph: Some("controller_xboxseries"),
        controls: &TEST_CONTROLS,
    };
    static TEST_FACE: faces::Face = faces::Face {
        name: "xbox",
        font: "kenney_input_xbox_series.otf",
        prefix: "xbox_",
        icons: &[],
        drawing: Some(&TEST_DRAWING),
    };

    #[test]
    fn the_body_drawing_matches_the_reference_on_a_synthetic_mask() {
        let pro = synthetic_pro();
        let mut sil = Mask::new(400, 300);
        sil.fill_ellipse(20, 20, 379, 279, 255);
        sil.fill_rect(150, 0, 250, 80, 0);
        let font_bytes = std::fs::read(
            crate::kit::root().join("third_party/kenney_input/kenney_input_xbox_series.otf"),
        )
        .unwrap();
        let font = Face::parse(&font_bytes, 0).unwrap();
        let picture = picture_scaled(&TEST_FACE, &pro, &font, None, 1, Some(&sil)).unwrap();
        // (Pillow 13.0.0, prompt_draw.picture at SS=1 with the same synthetic
        // silhouette and pro: shapes, bumpers, sprites and layer order match;
        // only glyph antialiasing differs elsewhere)
        assert_eq!(fnv(&picture.data), 0x28c5_746b_9721_1e07);
    }

    #[test]
    fn grown_keeps_the_references_threshold_and_foot() {
        let mut mask = Mask::new(17, 13);
        for y in 0..13 {
            for x in 0..17 {
                if (x * 7 + y * 5) % 11 == 0 {
                    mask.set(x, y, 255);
                }
            }
        }
        let grown = grown(&mask, 3.0);
        // (Pillow: GaussianBlur(1.5), then v > 6 becomes 255)
        assert_eq!(fnv(&grown.data), 0xb774_390e_f58c_2612);
        assert!(grown.data.iter().all(|value| *value == 0 || *value == 255));
    }

    #[test]
    fn the_tint_follows_the_games_linear_fit() {
        let mut plain = Rgba::new(16, 12);
        let mut tinted = Rgba::new(16, 12);
        for y in 0..12 {
            for x in 0..16 {
                let alpha = if (x + y) % 7 == 0 { 0 } else { 255 };
                let pixel = [
                    ((x * 13 + y * 7) & 0xff) as u8,
                    ((x * 3 + y * 19) & 0xff) as u8,
                    ((x * 11 + y * 5) & 0xff) as u8,
                    alpha,
                ];
                plain.set(x, y, pixel);
                let level = f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2]);
                let value = |slope: f64, intercept: f64| {
                    (slope * level + intercept).clamp(0.0, 255.0) as u8
                };
                tinted.set(
                    x,
                    y,
                    [
                        value(0.31, 12.0),
                        value(0.27, 31.0),
                        value(0.23, 47.0),
                        alpha,
                    ],
                );
            }
        }
        let mut picture = Rgba::new(16, 12);
        for y in 0..12 {
            for x in 0..16 {
                picture.set(
                    x,
                    y,
                    [
                        ((x * 21 + y * 9) & 0xff) as u8,
                        ((x * 5 + y * 27) & 0xff) as u8,
                        ((x * 17 + y * 3) & 0xff) as u8,
                        ((x * 31 + y * 11) & 0xff) as u8,
                    ],
                );
            }
        }
        let tint = tint_layer(&plain, &tinted, &picture).unwrap();
        // (numpy.polyfit/np.polyval on the same arrays)
        assert_eq!(fnv(&tint.data), 0xf242_40ca_3c2c_9f95);
    }

    #[test]
    fn the_shadow_is_the_cropped_alpha_squeezed_and_blurred() {
        let mut alpha = Rgba::new(500, 700);
        for y in 0..700 {
            for x in 0..500 {
                let value = if (x / 40 + y / 30) % 2 == 0 { 255 } else { 0 };
                alpha.set(x, y, [0, 0, 0, value]);
            }
        }
        let shadow = shadow_layer(&alpha, 60, 40);
        assert_eq!(fnv(&shadow.data), 0xd022_e350_a659_000f);
    }

    #[test]
    fn picture_layers_are_plain_then_tinted_then_shadow() {
        let mut plain = Rgba::new(8, 8);
        let mut tinted = Rgba::new(8, 8);
        let mut own = Rgba::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let pixel = [(x * 10) as u8, (y * 10) as u8, ((x + y) * 5) as u8, 255];
                plain.set(x, y, pixel);
                tinted.set(
                    x,
                    y,
                    [pixel[0] / 2 + 20, pixel[1] / 2 + 30, pixel[2] / 2 + 40, 255],
                );
                own.set(x, y, [40 + (x * 3) as u8, 50 + (y * 3) as u8, 60, 255]);
            }
        }
        let mut plain_texture = test_texture(8, 8);
        plain_texture.format = 0x2d06;
        plain_texture.data = 0x100;
        let mut tinted_texture = test_texture(8, 8);
        tinted_texture.format = 0x2d06;
        tinted_texture.data = 0x200;
        let mut shadow_texture = test_texture(8, 8);
        shadow_texture.format = 0x1d01;
        shadow_texture.data = 0x300;
        let entries = picture_entries(
            &plain_texture,
            &tinted_texture,
            &shadow_texture,
            &plain,
            &tinted,
            &own,
        )
        .unwrap();
        assert_eq!(entries.len(), 3);
        assert!(
            entries
                .iter()
                .all(|(button, pad, runs)| *button == 0xff && *pad == 0 && !runs.is_empty())
        );
        // (the three textures have the same block count; each entry writes all
        // four blocks in one run)
        let written = |entry: &(u8, u8, Vec<(u32, Vec<u8>)>)| {
            entry.2.iter().map(|(_, bytes)| bytes.len()).sum::<usize>()
        };
        assert_eq!(written(&entries[0]), 4 * 16);
        assert_eq!(written(&entries[1]), 4 * 16);
        assert_eq!(written(&entries[2]), 4 * 8);
    }

    #[test]
    fn no_game_asset_is_committed_for_the_prompt_faces() {
        let root = crate::kit::root();
        let overrides = root.join("nx2/prompt_faces");
        if let Ok(entries) = std::fs::read_dir(&overrides) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                assert!(
                    name == "README.md" || name == "." || name == "..",
                    "a game-derived prompt override is committed: nx2/prompt_faces/{name}"
                );
            }
        }
        for generated in [
            "Controller_Pro.png",
            "Controller_Pro1.png",
            "ControllerSh_Pro1.png",
            "Common.bntx",
            "External.bfotf",
            "textures.bin",
        ] {
            assert!(
                !root.join(generated).exists(),
                "a generated prompt asset is committed at the repository root: {generated}"
            );
        }
    }
}
