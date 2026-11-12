//! The game's `External` font with every face's icons: the OpenType tables
//! the build changes (CFF, cmap, hmtx, hhea, head, maxp) around the font's
//! own tables, which are copied untouched.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use ttf_parser::{Face, GlyphId, OutlineBuilder};

use super::cff;
use super::cmap;
use super::faces::{self, BIND_BASE, FACE_CODES, PAD_BASE, STATIC_AT};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub xmin: f64,
    pub ymin: f64,
    pub xmax: f64,
    pub ymax: f64,
}

impl Bounds {
    fn union(self, other: Bounds) -> Bounds {
        Bounds {
            xmin: self.xmin.min(other.xmin),
            ymin: self.ymin.min(other.ymin),
            xmax: self.xmax.max(other.xmax),
            ymax: self.ymax.max(other.ymax),
        }
    }

    fn int_rect(self) -> [i32; 4] {
        [
            self.xmin.floor() as i32,
            self.ymin.floor() as i32,
            self.xmax.ceil() as i32,
            self.ymax.ceil() as i32,
        ]
    }

    fn width(self) -> i32 {
        self.xmax.ceil() as i32 - self.xmin.floor() as i32
    }
}

#[derive(Clone, Copy, Debug)]
enum Command {
    Move(f64, f64),
    Line(f64, f64),
    Curve(f64, f64, f64, f64, f64, f64),
    Quad(f64, f64, f64, f64),
    Close,
}

#[derive(Clone, Default)]
struct Outline {
    commands: Vec<Command>,
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(Command::Move(x as f64, y as f64));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(Command::Line(x as f64, y as f64));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.commands
            .push(Command::Quad(x1 as f64, y1 as f64, x as f64, y as f64));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.commands.push(Command::Curve(
            x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64, y as f64,
        ));
    }

    fn close(&mut self) {
        self.commands.push(Command::Close);
    }
}

impl Outline {
    fn bounds(&self) -> Option<Bounds> {
        bounds_of(&self.commands)
    }
}

fn extend(bounds: &mut Option<Bounds>, x: f64, y: f64) {
    let point = Bounds {
        xmin: x,
        ymin: y,
        xmax: x,
        ymax: y,
    };
    *bounds = Some(match *bounds {
        Some(old) => old.union(point),
        None => point,
    });
}

fn bounds_of(commands: &[Command]) -> Option<Bounds> {
    let mut bounds: Option<Bounds> = None;
    let mut current = (0.0, 0.0);
    for command in commands {
        match *command {
            Command::Move(x, y) | Command::Line(x, y) => {
                extend(&mut bounds, x, y);
                current = (x, y);
            }
            Command::Curve(x1, y1, x2, y2, x, y) => {
                extend(&mut bounds, x, y);
                if let Some(curve) = curve_bounds(current, (x1, y1), (x2, y2), (x, y)) {
                    bounds = Some(bounds.unwrap().union(curve));
                }
                current = (x, y);
            }
            Command::Quad(x1, y1, x, y) => {
                extend(&mut bounds, x, y);
                if let Some(curve) = quad_bounds(current, (x1, y1), (x, y)) {
                    bounds = Some(bounds.unwrap().union(curve));
                }
                current = (x, y);
            }
            Command::Close => {}
        }
    }
    bounds
}

fn point_bounds(point: (f64, f64)) -> Bounds {
    Bounds {
        xmin: point.0,
        ymin: point.1,
        xmax: point.0,
        ymax: point.1,
    }
}

fn curve_bounds(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) -> Option<Bounds> {
    let p = [[p0.0, p0.1], [p1.0, p1.1], [p2.0, p2.1], [p3.0, p3.1]];
    let mut values = vec![p0, p3];
    for axis in 0..2 {
        let a = -p[0][axis] + 3.0 * p[1][axis] - 3.0 * p[2][axis] + p[3][axis];
        let b = 2.0 * (p[0][axis] - 2.0 * p[1][axis] + p[2][axis]);
        let c = p[1][axis] - p[0][axis];
        for t in quadratic_roots(a, b, c) {
            let u = 1.0 - t;
            let value = u * u * u * p[0][axis]
                + 3.0 * u * u * t * p[1][axis]
                + 3.0 * u * t * t * p[2][axis]
                + t * t * t * p[3][axis];
            let mut point = p0;
            if axis == 0 {
                point.0 = value;
            } else {
                point.1 = value;
            }
            values.push(point);
        }
    }
    let mut bounds = point_bounds(values[0]);
    for point in values {
        bounds = bounds.union(point_bounds(point));
    }
    Some(bounds)
}

fn quad_bounds(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) -> Option<Bounds> {
    // (a Type 2 charstring has cubics; this keeps the builder honest)
    let c1 = (
        p0.0 + 2.0 / 3.0 * (p1.0 - p0.0),
        p0.1 + 2.0 / 3.0 * (p1.1 - p0.1),
    );
    let c2 = (
        p2.0 + 2.0 / 3.0 * (p1.0 - p2.0),
        p2.1 + 2.0 / 3.0 * (p1.1 - p2.1),
    );
    curve_bounds(p0, c1, c2, p2)
}

fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    let mut out = Vec::new();
    if a.abs() < 1e-12 {
        if b.abs() > 1e-12 {
            let t = -c / b;
            if (0.0..1.0).contains(&t) {
                out.push(t);
            }
        }
        return out;
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return out;
    }
    let root = discriminant.sqrt();
    for t in [(-b + root) / (2.0 * a), (-b - root) / (2.0 * a)] {
        if (0.0..1.0).contains(&t) && !out.iter().any(|old| (*old - t).abs() < 1e-12) {
            out.push(t);
        }
    }
    out
}

fn ot_round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}

fn py_round(value: f64) -> f64 {
    value.round_ties_even()
}

struct Sfnt {
    version: [u8; 4],
    tables: Vec<([u8; 4], Vec<u8>)>,
}

impl Sfnt {
    fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 12 {
            return Err("font: not an sfnt".into());
        }
        let version = data[0..4].try_into().unwrap();
        let count = u16::from_be_bytes(data[4..6].try_into().unwrap()) as usize;
        let mut tables = Vec::with_capacity(count);
        for i in 0..count {
            let at = 12 + 16 * i;
            let record = data
                .get(at..at + 16)
                .ok_or_else(|| "font: the table directory runs past the file".to_string())?;
            let tag: [u8; 4] = record[0..4].try_into().unwrap();
            let offset = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
            let length = u32::from_be_bytes(record[12..16].try_into().unwrap()) as usize;
            let bytes = data
                .get(offset..offset + length)
                .ok_or_else(|| {
                    format!("font: {} runs past the file", String::from_utf8_lossy(&tag))
                })?
                .to_vec();
            tables.push((tag, bytes));
        }
        Ok(Sfnt { version, tables })
    }

    fn get(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.tables
            .iter()
            .find(|(name, _)| name == tag)
            .map(|(_, data)| data.as_slice())
    }

    fn set(&mut self, tag: &[u8; 4], data: Vec<u8>) {
        if let Some((_, old)) = self.tables.iter_mut().find(|(name, _)| name == tag) {
            *old = data;
        } else {
            self.tables.push((*tag, data));
        }
    }

    fn write(&self) -> Result<Vec<u8>, String> {
        let mut tables = self.tables.clone();
        tables.sort_by_key(|(tag, _)| *tag);
        let count = tables.len();
        if count == 0 || count > 0xFFFF {
            return Err("font: no tables".into());
        }
        let entry_selector = (31 - (count as u32).leading_zeros()) as u16;
        let search_range = (16u32 << entry_selector) as u16;
        // (head's checksum is taken with checkSumAdjustment zero)
        if let Some((_, data)) = tables.iter_mut().find(|(tag, _)| tag == b"head")
            && data.len() >= 12
        {
            data[8..12].fill(0);
        }
        let directory = 12 + 16 * count;
        let mut offset = directory;
        let mut records = Vec::with_capacity(count);
        for (tag, data) in &tables {
            offset = offset.div_ceil(4) * 4;
            records.push((*tag, offset, data.len(), checksum(data)));
            offset += data.len();
        }
        let total = offset.div_ceil(4) * 4;
        let mut out = vec![0u8; total];
        out[0..4].copy_from_slice(&self.version);
        out[4..6].copy_from_slice(&(count as u16).to_be_bytes());
        out[6..8].copy_from_slice(&search_range.to_be_bytes());
        out[8..10].copy_from_slice(&entry_selector.to_be_bytes());
        out[10..12].copy_from_slice(&(count as u16 * 16 - search_range).to_be_bytes());
        for (i, (tag, at, length, sum)) in records.iter().enumerate() {
            let record = 12 + 16 * i;
            out[record..record + 4].copy_from_slice(tag);
            out[record + 4..record + 8].copy_from_slice(&sum.to_be_bytes());
            out[record + 8..record + 12].copy_from_slice(&(*at as u32).to_be_bytes());
            out[record + 12..record + 16].copy_from_slice(&(*length as u32).to_be_bytes());
        }
        for ((_, at, length, _), (_, data)) in records.iter().zip(tables.iter()) {
            out[*at..*at + *length].copy_from_slice(data);
        }
        let whole = checksum(&out);
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(whole);
        let head_at = records
            .iter()
            .find(|(tag, _, _, _)| tag == b"head")
            .map(|(_, at, _, _)| *at)
            .ok_or_else(|| "font: no head table".to_string())?;
        out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
        Ok(out)
    }
}

fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

fn patch_head(data: &mut [u8], bbox: [i32; 4], modified: u64) -> Result<(), String> {
    if data.len() < 54 {
        return Err("font: a short head table".into());
    }
    data[28..36].copy_from_slice(&modified.to_be_bytes());
    for (i, value) in bbox.iter().enumerate() {
        let at = 36 + 2 * i;
        data[at..at + 2].copy_from_slice(&(*value as i16).to_be_bytes());
    }
    Ok(())
}

fn timestamp_now() -> u64 {
    let since_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    since_unix + 2_082_844_800 // (1904 to 1970, as OpenType counts seconds)
}

struct GameFont {
    glyphs: usize,
    outlines: Vec<Outline>,
    names: Vec<String>,
    advances: Vec<u16>,
    bearings: Vec<i16>,
    cmap: Vec<(u16, u16)>,
    cff: cff::Cff,
}

fn game_font(face: &Face, cff_data: &[u8]) -> Result<GameFont, String> {
    let glyphs = face.number_of_glyphs() as usize;
    let mut outlines = Vec::with_capacity(glyphs);
    for gid in 0..glyphs {
        let mut outline = Outline::default();
        face.outline_glyph(GlyphId(gid as u16), &mut outline);
        outlines.push(outline);
    }
    let cff = cff::parse(cff_data)?;
    if cff.charstrings.len() != glyphs {
        return Err(format!(
            "font: the CFF has {} charstrings, the font {glyphs} glyphs",
            cff.charstrings.len()
        ));
    }
    let names = (0..glyphs)
        .map(|gid| {
            face.glyph_name(GlyphId(gid as u16))
                .map(str::to_owned)
                .unwrap_or_else(|| format!("gid{gid}"))
        })
        .collect::<Vec<_>>();
    let advances = (0..glyphs)
        .map(|gid| face.glyph_hor_advance(GlyphId(gid as u16)).unwrap_or(0))
        .collect::<Vec<_>>();
    let bearings = (0..glyphs)
        .map(|gid| {
            face.glyph_hor_side_bearing(GlyphId(gid as u16))
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    Ok(GameFont {
        glyphs,
        outlines,
        names,
        advances,
        bearings,
        cmap: best_cmap(face),
        cff,
    })
}

fn best_cmap(face: &Face) -> Vec<(u16, u16)> {
    let Some(table) = face.tables().cmap else {
        return Vec::new();
    };
    for (platform, encoding) in [
        (3u16, 10u16),
        (0, 6),
        (0, 4),
        (3, 1),
        (0, 3),
        (0, 2),
        (0, 1),
        (0, 0),
    ] {
        for index in 0..table.subtables.len() {
            let Some(subtable) = table.subtables.get(index) else {
                continue;
            };
            let subtable_platform = match subtable.platform_id {
                ttf_parser::PlatformId::Unicode => 0,
                ttf_parser::PlatformId::Macintosh => 1,
                ttf_parser::PlatformId::Iso => 2,
                ttf_parser::PlatformId::Windows => 3,
                ttf_parser::PlatformId::Custom => 4,
            };
            if subtable_platform != platform || subtable.encoding_id != encoding {
                continue;
            }
            let mut mapping = Vec::new();
            // (0xFFFF is the format 4 terminator, not a character)
            for code in 0..0xFFFFu32 {
                if let Some(gid) = subtable.glyph_index(code) {
                    mapping.push((code as u16, gid.0));
                }
            }
            if !mapping.is_empty() {
                return mapping;
            }
        }
    }
    Vec::new()
}

struct NewGlyph {
    name: String,
    commands: Vec<Command>,
    width: u16,
    bounds: Bounds,
}

fn transformed(target: &Outline, kenney: &Outline) -> Result<((f64, f64), f64, i32), String> {
    let gb = target
        .bounds()
        .ok_or_else(|| "font: a target glyph has no outline".to_string())?;
    let kb = kenney
        .bounds()
        .ok_or_else(|| "font: a Kenney glyph has no outline".to_string())?;
    let side = (gb.xmax - gb.xmin).max(gb.ymax - gb.ymin);
    let kw = kb.xmax - kb.xmin;
    let kh = kb.ymax - kb.ymin;
    if kw <= 0.0 || kh <= 0.0 {
        return Err("font: a Kenney glyph has no width".into());
    }
    let scale = (side * faces::WIDE / kw).min(side / kh);
    let over = (scale * kw - side).max(0.0);
    let dx = (gb.xmin + gb.xmax + over) / 2.0 - scale * (kb.xmin + kb.xmax) / 2.0;
    let dy = (gb.ymin + gb.ymax) / 2.0 - scale * (kb.ymin + kb.ymax) / 2.0;
    Ok(((scale, dx), dy, py_round(over) as i32))
}

fn rounded_outline(commands: &[Command], scale: f64, dx: f64, dy: f64) -> Vec<Command> {
    let mut out = Vec::with_capacity(commands.len());
    let mut current = (0.0, 0.0);
    for command in commands {
        match *command {
            Command::Move(x, y) => {
                let point = (
                    ot_round(x * scale + dx) as f64,
                    ot_round(y * scale + dy) as f64,
                );
                out.push(Command::Move(point.0, point.1));
                current = point;
            }
            Command::Line(x, y) => {
                let point = (
                    ot_round(x * scale + dx) as f64,
                    ot_round(y * scale + dy) as f64,
                );
                if point != current {
                    out.push(Command::Line(point.0, point.1));
                    current = point;
                }
            }
            Command::Curve(x1, y1, x2, y2, x, y) => {
                let p1 = (
                    ot_round(x1 * scale + dx) as f64,
                    ot_round(y1 * scale + dy) as f64,
                );
                let p2 = (
                    ot_round(x2 * scale + dx) as f64,
                    ot_round(y2 * scale + dy) as f64,
                );
                let p3 = (
                    ot_round(x * scale + dx) as f64,
                    ot_round(y * scale + dy) as f64,
                );
                // (fontTools' T2 pen drops a segment that rounds to nothing)
                if !(p1 == current && p2 == current && p3 == current) {
                    out.push(Command::Curve(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1));
                    current = p3;
                }
            }
            Command::Quad(x1, y1, x, y) => {
                let p1 = (
                    ot_round(x1 * scale + dx) as f64,
                    ot_round(y1 * scale + dy) as f64,
                );
                let p2 = (
                    ot_round(x * scale + dx) as f64,
                    ot_round(y * scale + dy) as f64,
                );
                if !(p1 == current && p2 == current) {
                    out.push(Command::Quad(p1.0, p1.1, p2.0, p2.1));
                    current = p2;
                }
            }
            Command::Close => out.push(Command::Close),
        }
    }
    out
}

/// FontTools' Type 2 pen merges adjacent horizontal (or vertical) line
/// segments into one operator; its geometry is the same, and so is the
/// recording a reader sees. The rounding can leave a run of them.
fn merge_axis_lines(commands: Vec<Command>) -> Vec<Command> {
    let mut out: Vec<Command> = Vec::with_capacity(commands.len());
    let mut current = (0.0, 0.0);
    let mut line_from = (0.0, 0.0);
    for command in commands {
        match command {
            Command::Line(x, y) => {
                let point = (x, y);
                let same_axis =
                    (line_from.0 == current.0 && current.0 == point.0 && line_from.1 != current.1)
                        || (line_from.1 == current.1
                            && current.1 == point.1
                            && line_from.0 != current.0);
                if let (true, Some(Command::Line(_, _))) = (same_axis, out.last()) {
                    out.pop();
                    out.push(Command::Line(point.0, point.1));
                } else {
                    out.push(Command::Line(point.0, point.1));
                    line_from = current;
                }
                current = point;
            }
            other => {
                out.push(other);
                match other {
                    Command::Move(x, y) | Command::Quad(_, _, x, y) => current = (x, y),
                    Command::Curve(_, _, _, _, x, y) => current = (x, y),
                    _ => {}
                }
                line_from = current;
            }
        }
    }
    out
}

fn t2_number(out: &mut Vec<u8>, value: i32) {
    match value {
        v @ -107..=107 => out.push((v + 139) as u8),
        v @ 108..=1131 => {
            let v = v - 108;
            out.push((247 + (v >> 8)) as u8);
            out.push((v & 0xFF) as u8);
        }
        v @ -1131..=-108 => {
            let v = -v - 108;
            out.push((251 + (v >> 8)) as u8);
            out.push((v & 0xFF) as u8);
        }
        v @ -32768..=32767 => {
            out.push(28);
            out.extend_from_slice(&(v as i16).to_be_bytes());
        }
        v => {
            out.push(29);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
}

fn charstring(commands: &[Command], width: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut current = (0i32, 0i32);
    let mut width_left = true;
    for command in commands {
        match *command {
            Command::Move(x, y) => {
                let point = (x as i32, y as i32);
                if width_left {
                    t2_number(&mut out, width);
                }
                t2_number(&mut out, point.0 - current.0);
                t2_number(&mut out, point.1 - current.1);
                out.push(21);
                current = point;
            }
            Command::Line(x, y) => {
                let point = (x as i32, y as i32);
                t2_number(&mut out, point.0 - current.0);
                t2_number(&mut out, point.1 - current.1);
                out.push(5);
                current = point;
            }
            Command::Curve(x1, y1, x2, y2, x, y) => {
                let p1 = (x1 as i32, y1 as i32);
                let p2 = (x2 as i32, y2 as i32);
                let p3 = (x as i32, y as i32);
                t2_number(&mut out, p1.0 - current.0);
                t2_number(&mut out, p1.1 - current.1);
                t2_number(&mut out, p2.0 - p1.0);
                t2_number(&mut out, p2.1 - p1.1);
                t2_number(&mut out, p3.0 - p2.0);
                t2_number(&mut out, p3.1 - p2.1);
                out.push(8);
                current = p3;
            }
            Command::Quad(x1, y1, x, y) => {
                // (the cubic a Type 2 charstring holds; no CFF source emits
                // a quadratic, this is for safety)
                let p1 = (
                    ot_round(current.0 as f64 + 2.0 / 3.0 * (x1 - current.0 as f64)),
                    ot_round(current.1 as f64 + 2.0 / 3.0 * (y1 - current.1 as f64)),
                );
                let p2 = (
                    ot_round(x + 2.0 / 3.0 * (x1 - x)),
                    ot_round(y + 2.0 / 3.0 * (y1 - y)),
                );
                let p3 = (x as i32, y as i32);
                t2_number(&mut out, p1.0 - current.0);
                t2_number(&mut out, p1.1 - current.1);
                t2_number(&mut out, p2.0 - p1.0);
                t2_number(&mut out, p2.1 - p1.1);
                t2_number(&mut out, p3.0 - p2.0);
                t2_number(&mut out, p3.1 - p2.1);
                out.push(8);
                current = p3;
            }
            Command::Close => {}
        }
        width_left = false;
    }
    out.push(14);
    out
}

fn kenney_glyph(map: &HashMap<String, GlyphId>, face: &faces::Face, pad: &str) -> Option<GlyphId> {
    let raw = face
        .icons
        .iter()
        .find(|(input, _)| *input == pad)
        .map(|(_, name)| *name)?;
    let name = match raw.strip_prefix('/') {
        Some(name) => name.to_owned(),
        None => format!("{}{raw}", face.prefix),
    };
    map.get(&name).copied()
}

/// The written font, and each face's advances for its pad icons.
pub fn build(otf: &[u8], kenney: &Path) -> Result<(Vec<u8>, Vec<(String, Vec<u16>)>), String> {
    let face = Face::parse(otf, 0).map_err(|e| format!("font: {e:?}"))?;
    let cff_data = face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"CFF "))
        .ok_or_else(|| "font: no CFF table".to_string())?;
    let mut game = game_font(&face, cff_data)?;
    let old_glyphs = game.glyphs;
    let mut new_glyphs: Vec<NewGlyph> = Vec::new();
    let mut placed: BTreeMap<u16, u16> = BTreeMap::new();
    let mut face_advances: Vec<(String, Vec<u16>)> = Vec::new();

    // (the Switch face: the game's own icons)
    let switch_base = PAD_BASE;
    let mut switch_widths = Vec::new();
    for (i, pad) in faces::PADS.iter().enumerate() {
        let code = faces::switch_icon(pad).unwrap();
        let gid = game
            .cmap
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, gid)| *gid)
            .ok_or_else(|| format!("font: the game's cmap has no U+{code:04X}"))?;
        placed.insert(switch_base + i as u16, gid);
        switch_widths.push(game.advances[gid as usize]);
    }
    for (k, (code, _)) in faces::STATIC.iter().enumerate() {
        if let Some((_, gid)) = game.cmap.iter().find(|(c, _)| c == code) {
            placed.insert(switch_base + STATIC_AT + k as u16, *gid);
        }
    }
    place_bindings(&mut placed, 0, switch_base);
    face_advances.push(("switch".to_owned(), switch_widths));

    for (n, spec) in faces::FACES.iter().enumerate() {
        let path = kenney.join(spec.font);
        let kenney_bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let kenney_face =
            Face::parse(&kenney_bytes, 0).map_err(|e| format!("{}: {e:?}", path.display()))?;
        let mut koutlines: Vec<Outline> = Vec::new();
        for gid in 0..kenney_face.number_of_glyphs() {
            let mut outline = Outline::default();
            kenney_face.outline_glyph(GlyphId(gid), &mut outline);
            koutlines.push(outline);
        }
        let kmap: HashMap<String, GlyphId> = (0..kenney_face.number_of_glyphs())
            .filter_map(|gid| {
                kenney_face
                    .glyph_name(GlyphId(gid))
                    .map(|name| (name.to_owned(), GlyphId(gid)))
            })
            .collect();
        let base = PAD_BASE + FACE_CODES * (n as u16 + 1);
        let mut widths = Vec::new();
        for (i, pad) in faces::PADS.iter().enumerate() {
            let target_code = faces::switch_icon(pad).unwrap();
            let target_gid = game
                .cmap
                .iter()
                .find(|(code, _)| *code == target_code)
                .map(|(_, gid)| *gid)
                .ok_or_else(|| format!("font: the game's cmap has no U+{target_code:04X}"))?;
            let gid = match kenney_glyph(&kmap, spec, pad) {
                Some(kgid) => {
                    let target = &game.outlines[target_gid as usize];
                    let koutline = &koutlines[kgid.0 as usize];
                    let ((scale, dx), dy, over) = transformed(target, koutline)?;
                    let commands =
                        merge_axis_lines(rounded_outline(&koutline.commands, scale, dx, dy));
                    let width = game.advances[target_gid as usize] as i32 + over;
                    let name = format!("pad_{}_{}", spec.name, pad);
                    let bounds = bounds_of(&commands)
                        .ok_or_else(|| format!("font: {name} has no outline"))?;
                    let gid = (old_glyphs + new_glyphs.len()) as u16;
                    new_glyphs.push(NewGlyph {
                        name,
                        commands,
                        width: width as u16,
                        bounds,
                    });
                    gid
                }
                None => target_gid,
            };
            placed.insert(base + i as u16, gid);
            widths.push(advance(gid, old_glyphs, &game.advances, &new_glyphs));
        }
        for (k, (code, static_name)) in faces::STATIC.iter().enumerate() {
            let Some(target_gid) = game.cmap.iter().find(|(c, _)| c == code).map(|(_, g)| *g)
            else {
                continue;
            };
            let glyph = kmap
                .get(&format!("{}{}", spec.prefix, static_name))
                .copied();
            let gid = match glyph {
                Some(kgid) => {
                    let target = &game.outlines[target_gid as usize];
                    let koutline = &koutlines[kgid.0 as usize];
                    let ((scale, dx), dy, over) = transformed(target, koutline)?;
                    let commands =
                        merge_axis_lines(rounded_outline(&koutline.commands, scale, dx, dy));
                    let width = game.advances[target_gid as usize] as i32 + over;
                    let name = format!("static_{}_{:04x}", spec.name, code);
                    let bounds = bounds_of(&commands)
                        .ok_or_else(|| format!("font: {name} has no outline"))?;
                    let gid = (old_glyphs + new_glyphs.len()) as u16;
                    new_glyphs.push(NewGlyph {
                        name,
                        commands,
                        width: width as u16,
                        bounds,
                    });
                    gid
                }
                None => target_gid,
            };
            placed.insert(base + STATIC_AT + k as u16, gid);
        }
        place_bindings(&mut placed, n as u16 + 1, base);
        face_advances.push((spec.name.to_owned(), widths));
    }

    let bbox = game
        .outlines
        .iter()
        .filter_map(Outline::bounds)
        .chain(new_glyphs.iter().map(|glyph| glyph.bounds))
        .reduce(Bounds::union);
    let bbox_ints = bbox.map(Bounds::int_rect).unwrap_or(game.cff.top.font_bbox);
    game.cff.top.font_bbox = bbox_ints;
    for glyph in &new_glyphs {
        game.cff
            .add_glyph(&glyph.name, charstring(&glyph.commands, glyph.width as i32));
        game.names.push(glyph.name.clone());
        game.advances.push(glyph.width);
        game.bearings.push(0);
    }
    let cff_bytes = game.cff.write()?;
    let mut sfnt = Sfnt::parse(otf)?;
    sfnt.set(b"CFF ", cff_bytes);
    let cmap = cmap_bytes(&game.cmap, &placed);
    sfnt.set(b"cmap", cmap);
    let count = old_glyphs + new_glyphs.len();
    sfnt.set(
        b"maxp",
        maxp(
            sfnt.get(b"maxp")
                .ok_or_else(|| "font: no maxp table".to_string())?,
            count,
        ),
    );
    sfnt.set(b"hmtx", hmtx(&game.advances, &game.bearings));
    sfnt.set(
        b"hhea",
        hhea(
            sfnt.get(b"hhea")
                .ok_or_else(|| "font: no hhea table".to_string())?,
            &game.advances,
            &game.bearings,
            &game.outlines,
            &new_glyphs,
        )?,
    );
    let mut head = sfnt
        .get(b"head")
        .ok_or_else(|| "font: no head table".to_string())?
        .to_vec();
    patch_head(&mut head, bbox_ints, timestamp_now())?;
    sfnt.set(b"head", head);
    Ok((sfnt.write()?, face_advances))
}

fn place_bindings(placed: &mut BTreeMap<u16, u16>, face: u16, base: u16) {
    for button in 0..faces::SWITCH.len() as u16 {
        for (input, _) in faces::PADS.iter().enumerate() {
            let code = BIND_BASE + (face * faces::SWITCH.len() as u16 + button) * 17 + input as u16;
            if let Some(gid) = placed.get(&(base + input as u16)).copied() {
                placed.insert(code, gid);
            }
        }
    }
}

fn advance(gid: u16, old_glyphs: usize, old: &[u16], new: &[NewGlyph]) -> u16 {
    if (gid as usize) < old_glyphs {
        old[gid as usize]
    } else {
        new[gid as usize - old_glyphs].width
    }
}

fn cmap_bytes(old: &[(u16, u16)], placed: &BTreeMap<u16, u16>) -> Vec<u8> {
    let mut mapping: BTreeMap<u16, u16> = old.iter().copied().collect();
    for (code, gid) in placed {
        mapping.insert(*code, *gid);
    }
    let single = faces::SWITCH
        .iter()
        .copied()
        .chain(faces::STATIC.iter().map(|(code, _)| *code))
        .collect::<Vec<_>>();
    cmap::cmap4(&mapping.into_iter().collect::<Vec<_>>(), &single)
}

fn maxp(old: &[u8], glyphs: usize) -> Vec<u8> {
    let mut out = old.to_vec();
    if out.len() >= 6 {
        out[4..6].copy_from_slice(&(glyphs as u16).to_be_bytes());
    }
    out
}

fn metric_count(advances: &[u16]) -> usize {
    let mut number = advances.len().max(1);
    let last = advances.last().copied().unwrap_or(0);
    while number > 1 && advances[number - 2] == last {
        number -= 1;
    }
    number
}

fn hmtx(advances: &[u16], bearings: &[i16]) -> Vec<u8> {
    let number = metric_count(advances);
    let mut out = Vec::with_capacity(number * 4 + (advances.len() - number) * 2);
    for i in 0..number {
        out.extend_from_slice(&advances[i].to_be_bytes());
        out.extend_from_slice(&bearings[i].to_be_bytes());
    }
    for i in number..advances.len() {
        out.extend_from_slice(&bearings[i].to_be_bytes());
    }
    out
}

fn hhea(
    old: &[u8],
    advances: &[u16],
    bearings: &[i16],
    outlines: &[Outline],
    new_glyphs: &[NewGlyph],
) -> Result<Vec<u8>, String> {
    if old.len() < 36 {
        return Err("font: a short hhea table".into());
    }
    let mut out = old.to_vec();
    out[34..36].copy_from_slice(&(metric_count(advances) as u16).to_be_bytes());
    out[10..12].copy_from_slice(&advances.iter().copied().max().unwrap_or(0).to_be_bytes());
    let bounds = outlines
        .iter()
        .map(Outline::bounds)
        .chain(new_glyphs.iter().map(|glyph| Some(glyph.bounds)));
    let mut min_lsb = f64::INFINITY;
    let mut min_rsb = f64::INFINITY;
    let mut max_extent = f64::NEG_INFINITY;
    let mut any = false;
    for (gid, bound) in bounds.enumerate() {
        let Some(bound) = bound else { continue };
        any = true;
        let width = bound.width() as f64;
        let advance = advances[gid] as f64;
        let lsb = bearings[gid] as f64;
        min_lsb = min_lsb.min(lsb);
        min_rsb = min_rsb.min(advance - lsb - width);
        max_extent = max_extent.max(lsb + width);
    }
    if any {
        out[12..14].copy_from_slice(&(min_lsb as i16).to_be_bytes());
        out[14..16].copy_from_slice(&(min_rsb as i16).to_be_bytes());
        out[16..18].copy_from_slice(&(max_extent as i16).to_be_bytes());
    } else {
        out[12..18].fill(0);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict_number(out: &mut Vec<u8>, value: i32) {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn fixture() -> Vec<u8> {
        // (a small CFF font whose cmap has the game's own button codes: the
        // builder needs nothing from the game's files)
        let mut codes = faces::SWITCH_ICON
            .iter()
            .map(|(_, code)| *code)
            .chain(faces::STATIC.iter().map(|(code, _)| *code))
            .collect::<Vec<_>>();
        codes.sort_unstable();
        let glyphs = codes.len() + 1;
        let names = (1..glyphs)
            .map(|gid| format!("gid{gid}"))
            .collect::<Vec<_>>();
        let strings = cff::write_index(
            &names
                .iter()
                .map(|name| name.as_bytes().to_vec())
                .collect::<Vec<_>>(),
        );
        let mut charstrings = Vec::new();
        let commands = [
            Command::Move(50.0, 50.0),
            Command::Line(200.0, 50.0),
            Command::Line(200.0, 200.0),
            Command::Line(50.0, 200.0),
            Command::Line(50.0, 50.0),
            Command::Close,
        ];
        charstrings.push(charstring(&[], 500));
        for _ in 1..glyphs {
            charstrings.push(charstring(&commands, 500));
        }
        let charstrings = cff::write_index(&charstrings);
        let mut charset = vec![0u8];
        for sid in 391..391 + glyphs as u16 - 1 {
            charset.extend_from_slice(&sid.to_be_bytes());
        }
        let name_index = cff::write_index(&[b"Test".to_vec()]);
        let global_subrs = cff::write_index(&[]);
        // The Top DICT's offsets are all longints, so its length is fixed.
        let mut top = Vec::new();
        for value in [0, 0, 500, 500] {
            dict_number(&mut top, value);
        }
        top.push(5); // FontBBox
        dict_number(&mut top, 0);
        top.push(15); // charset
        dict_number(&mut top, 0);
        top.push(17); // CharStrings
        let top_dict_len = top.len();
        let top_index_len = 2 + 1 + 2 * 4 + top_dict_len;
        let data_at = 4 + name_index.len() + top_index_len + strings.len() + global_subrs.len();
        let charset_at = data_at;
        let charstrings_at = charset_at + charset.len();
        top.clear();
        for value in [0, 0, 500, 500] {
            dict_number(&mut top, value);
        }
        top.push(5);
        dict_number(&mut top, charset_at as i32);
        top.push(15);
        dict_number(&mut top, charstrings_at as i32);
        top.push(17);
        let mut top_index = Vec::new();
        top_index.extend_from_slice(&1u16.to_be_bytes());
        top_index.push(4);
        top_index.extend_from_slice(&1u32.to_be_bytes());
        top_index.extend_from_slice(&(top.len() as u32 + 1).to_be_bytes());
        top_index.extend_from_slice(&top);
        let mut cff = Vec::new();
        cff.extend_from_slice(&[1, 0, 4, 4]);
        cff.extend_from_slice(&name_index);
        cff.extend_from_slice(&top_index);
        cff.extend_from_slice(&strings);
        cff.extend_from_slice(&global_subrs);
        cff.extend_from_slice(&charset);
        cff.extend_from_slice(&charstrings);

        let mut head = vec![0u8; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        for (i, value) in [0i16, 0, 500, 500].iter().enumerate() {
            head[36 + 2 * i..38 + 2 * i].copy_from_slice(&value.to_be_bytes());
        }
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        hhea[10..12].copy_from_slice(&500u16.to_be_bytes());
        hhea[34..36].copy_from_slice(&1u16.to_be_bytes());
        let mut maxp = vec![0u8; 6];
        maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&(glyphs as u16).to_be_bytes());
        let mut hmtx = Vec::new();
        hmtx.extend_from_slice(&500u16.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
        for _ in 1..glyphs {
            hmtx.extend_from_slice(&0i16.to_be_bytes());
        }
        let mapping = codes
            .iter()
            .enumerate()
            .map(|(index, code)| (*code, index as u16 + 1))
            .collect::<Vec<_>>();
        let cmap = cmap::cmap4(&mapping, &[]);
        let sfnt = Sfnt {
            version: *b"OTTO",
            tables: vec![
                (*b"CFF ", cff),
                (*b"cmap", cmap),
                (*b"head", head),
                (*b"hhea", hhea),
                (*b"hmtx", hmtx),
                (*b"maxp", maxp),
            ],
        };
        sfnt.write().unwrap()
    }

    #[test]
    fn a_synthetic_font_grows_with_every_face() {
        let otf = fixture();
        let kenney = crate::kit::root().join("third_party/kenney_input");
        let (font, widths) = build(&otf, &kenney).unwrap();
        assert_eq!(widths.len(), 5);
        assert_eq!(widths[0].0, "switch");
        assert_eq!(widths[0].1.len(), faces::PADS.len());
        let face = Face::parse(&font, 0).unwrap();
        assert!(face.number_of_glyphs() > 21);
        assert!(face.glyph_index('\u{F700}').is_some());
        assert!(face.glyph_index('\u{F000}').is_some());
        let cff = face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"CFF "))
            .unwrap();
        let parsed = cff::parse(cff).unwrap();
        assert_eq!(parsed.charstrings.len(), face.number_of_glyphs() as usize);
        assert!(
            parsed
                .strings
                .objects
                .iter()
                .any(|name| name == b"pad_xbox_south")
        );
    }

    /// A local check with the game's own font, for a development machine:
    /// `HOOKSHOT_TEST_ROMFS` is not set in CI.
    #[test]
    fn the_game_font_grows_when_a_romfs_is_given() {
        let Some(romfs) = std::env::var_os("HOOKSHOT_TEST_ROMFS") else {
            return;
        };
        let romfs = Path::new(&romfs);
        let dictionary = crate::prompts::zs::zs_dictionary(romfs).unwrap();
        let packed = crate::prompts::zs::read_zs(romfs, crate::prompts::FONT, &dictionary).unwrap();
        let (_, otf) = crate::prompts::bfotf::unwrap(&packed).unwrap();
        let kenney = crate::kit::root().join("third_party/kenney_input");
        let (font, widths) = build(&otf, &kenney).unwrap();
        assert!(font.len() > otf.len());
        assert_eq!(widths.len(), 5);
        assert_eq!(widths[0].0, "switch");
        let face = Face::parse(&font, 0).unwrap();
        assert!(face.number_of_glyphs() > 261);
        assert!(face.glyph_index('\u{F700}').is_some());
        let output = std::env::var_os("HOOKSHOT_TEST_FONT_OUT");
        if let Some(path) = output {
            std::fs::write(path, &font).unwrap();
        }
    }
}
