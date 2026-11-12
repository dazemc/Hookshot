//! The keyboard face of the button prompts (ported from upstream's
//! `nx2/prompt_keys.py` and the keyboard half of `nx2/prompt_draw.py`):
//! which icon each key and the sticks show, and the outlines the keys drawn
//! by hand take.
//!
//! The font (`super::font`) carries each key's icon at `KEY_BASE` + its
//! Windows virtual-key code, the sticks' icons at `STICK_KEYS`/`STICK_GAME`,
//! and a placeholder for each `SLOT_BASE` code the live host gives a text.
//! Mouse buttons and the keys whose names are words are drawn here; the
//! others come from Kenney's Keyboard & Mouse font.

use ttf_parser::{Face, GlyphId, OutlineBuilder};

use super::font::{self, Bounds, Command};

/// The game's A, each key's box in the font.
pub const TARGET: u16 = 0xE040;
/// Past every face's code points (`PAD_BASE + FACE_CODES * HOS_PROMPT_N`,
/// where `HOS_PROMPT_N` counts the keyboard).
pub const KEY_BASE: u16 = 0xF7C0;
/// Each (Switch button, key) a text shows gets a code of its own, given out
/// by the host as they come (`host/prompts.c`).
pub const SLOT_BASE: u16 = 0xEEC0;
pub const SLOTS: usize = 320;
pub const STICK_KEYS: u16 = 0xF8C0;
pub const STICK_GAME: u16 = 0xF8D0;

/// The keyboard face's Kenney font (`third_party/kenney_input_keyboard`).
pub const FONT: &str = "kenney_input_keyboard_&_mouse.otf";
/// The game's own bold font, for the keys whose names are words.
pub const LABEL_FONT: &str = "Font2/Scalable/ClarimoUDPE-DemiBold.Nin_Ounce_NVN2.bfotf.zs";

/// How much wider than the game's icon a word key may be.
pub const MOST: f64 = 2.2;
/// A quarter circle's control points, as a fraction of its radius.
const K: f64 = 0.5523;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stick {
    Move,
    Cam,
}

#[derive(Clone, Copy, Debug)]
pub struct StickIcon {
    /// The game's own stick icon code point.
    pub code: u16,
    pub stick: Stick,
    /// The ways shown, in "up down left right" order.
    pub ways: &'static str,
}

/// `host/prompts.c` stick_code order: the left stick's, then the camera's.
pub const STICKS: [StickIcon; 15] = [
    StickIcon {
        code: 0xE050,
        stick: Stick::Move,
        ways: "d",
    },
    StickIcon {
        code: 0xE052,
        stick: Stick::Move,
        ways: "u",
    },
    StickIcon {
        code: 0xE054,
        stick: Stick::Move,
        ways: "lr",
    },
    StickIcon {
        code: 0xE058,
        stick: Stick::Move,
        ways: "r",
    },
    StickIcon {
        code: 0xE05A,
        stick: Stick::Move,
        ways: "l",
    },
    StickIcon {
        code: 0xE05C,
        stick: Stick::Move,
        ways: "ud",
    },
    StickIcon {
        code: 0xE05E,
        stick: Stick::Move,
        ways: "uldr",
    },
    StickIcon {
        code: 0xE051,
        stick: Stick::Cam,
        ways: "d",
    },
    StickIcon {
        code: 0xE053,
        stick: Stick::Cam,
        ways: "u",
    },
    StickIcon {
        code: 0xE055,
        stick: Stick::Cam,
        ways: "lr",
    },
    StickIcon {
        code: 0xE059,
        stick: Stick::Cam,
        ways: "r",
    },
    StickIcon {
        code: 0xE05B,
        stick: Stick::Cam,
        ways: "l",
    },
    StickIcon {
        code: 0xE05D,
        stick: Stick::Cam,
        ways: "ud",
    },
    StickIcon {
        code: 0xE05F,
        stick: Stick::Cam,
        ways: "uldr",
    },
    StickIcon {
        code: 0xE068,
        stick: Stick::Cam,
        ways: "uldr",
    },
];

/// The movement keys the single-way stick icons show (hookshot's defaults).
pub fn move_key(way: char) -> Option<u8> {
    match way {
        'u' => Some(0x57),
        'd' => Some(0x53),
        'l' => Some(0x41),
        'r' => Some(0x44),
        _ => None,
    }
}

/// A key's Kenney glyph name after the font's "keyboard_" prefix, when the
/// key has one: `/name` says the whole name.
pub fn key_name(code: u8) -> Option<&'static str> {
    let name = match code {
        0x01 => "/mouse_left",
        0x02 => "/mouse_right",
        0x04 => "/mouse_scroll",
        0x05 => "/mouse_side_back",
        0x06 => "/mouse_side_forward",
        0x08 => "backspace_icon",
        0x09 => "tab_icon",
        0x0D => "return",
        0x10 => "shift_icon",
        0x11 => "ctrl",
        0x12 => "alt",
        0x13 => "pause",
        0x14 => "capslock_icon",
        0x1B => "escape",
        0x20 => "space_icon",
        0x21 => "page_up",
        0x22 => "page_down",
        0x23 => "end",
        0x24 => "home",
        0x25 => "arrow_left",
        0x26 => "arrow_up",
        0x27 => "arrow_right",
        0x28 => "arrow_down",
        0x2C => "printscreen",
        0x2D => "insert",
        0x2E => "delete",
        0x5B | 0x5C => "win",
        0x6A => "asterisk",
        0x6B => "numpad_plus",
        0x6D => "minus",
        0x6E => "period",
        0x6F => "slash_forward",
        0x90 => "numlock",
        0x91 => "scroll_lock",
        0xA0 | 0xA1 => "shift_icon",
        0xA2 | 0xA3 => "ctrl",
        0xA4 | 0xA5 => "alt",
        0xBA => "semicolon",
        0xBB => "equals",
        0xBC => "comma",
        0xBD => "minus",
        0xBE => "period",
        0xBF => "slash_forward",
        0xC0 => "tilde",
        0xDB => "bracket_open",
        0xDC => "slash_back",
        0xDD => "bracket_close",
        0xDE => "apostrophe",
        0x30..=0x39 => return Some(digit_name(code - 0x30)),
        // (the numpad's digits: Kenney has none of their own)
        0x60..=0x69 => return Some(digit_name(code - 0x60)),
        0x41..=0x5A => return Some(letter_name(code - 0x41)),
        0x70..=0x7B => return Some(function_name(code - 0x70)),
        _ => return None,
    };
    Some(name)
}

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const LETTERS: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];
const FUNCTIONS: [&str; 12] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
];

fn digit_name(index: u8) -> &'static str {
    DIGITS[index as usize]
}
fn letter_name(index: u8) -> &'static str {
    LETTERS[index as usize]
}
fn function_name(index: u8) -> &'static str {
    FUNCTIONS[index as usize]
}

/// The key's name after the font's prefix ("keyboard_"), for the keys that
/// font has a glyph for.
pub fn kenney_name(code: u8) -> Option<String> {
    let name = key_name(code)?;
    Some(match name.strip_prefix('/') {
        Some(name) => name.to_owned(),
        None => format!("keyboard_{name}"),
    })
}

/// The keys whose names are words, drawn as wider keys with the word cut
/// out in the game's own bold font (Kenney's words are too small to read).
pub fn label(code: u8) -> Option<&'static str> {
    let name = match code {
        0x05 => "M4",
        0x06 => "M5",
        0x11 => "Ctrl",
        0x12 => "Alt",
        0x13 => "Pause",
        0x1B => "Esc",
        0x21 => "PgUp",
        0x22 => "PgDn",
        0x23 => "End",
        0x24 => "Home",
        0x2C => "PrtSc",
        0x2D => "Ins",
        0x2E => "Del",
        0x90 => "NumLk",
        0x91 => "ScrLk",
        0xA2 | 0xA3 => "Ctrl",
        0xA4 | 0xA5 => "Alt",
        // (only the numpad's digits are word keys: the top row's are Kenney's)
        0x60..=0x69 => return Some(numpad_name(code - 0x60)),
        _ => return None,
    };
    Some(name)
}

const NUMPAD: [&str; 10] = [
    "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
];

fn numpad_name(index: u8) -> &'static str {
    NUMPAD[index as usize]
}

/// The mouse buttons drawn as a mouse with that button left white.
pub fn mouse(code: u8) -> Option<&'static str> {
    match code {
        0x01 => Some("left"),
        0x02 => Some("right"),
        0x04 => Some("middle"),
        _ => None,
    }
}

/// A key drawn with the game's bold font whose outline comes from it.
pub struct LabelFont<'a> {
    face: Face<'a>,
}

impl<'a> LabelFont<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        Face::parse(bytes, 0)
            .map(|face| LabelFont { face })
            .map_err(|e| format!("prompts: the label font: {e:?}"))
    }

    fn glyph(&self, ch: char) -> Option<GlyphId> {
        self.face.glyph_index(ch)
    }

    fn outline(&self, ch: char) -> Option<Vec<Command>> {
        let gid = self.glyph(ch)?;
        let mut outline = Collector::default();
        self.face.outline_glyph(gid, &mut outline)?;
        Some(outline.commands)
    }

    fn advance(&self, ch: char) -> Option<u16> {
        self.face.glyph_hor_advance(self.glyph(ch)?)
    }

    fn cap_height(&self) -> Option<f64> {
        font::bounds_of(&self.outline('H')?).map(|bounds| bounds.ymax)
    }

    fn letters_are_counter_clockwise(&self) -> bool {
        self.outline('I')
            .map(|outline| area(&outline) > 0.0)
            .unwrap_or(false)
    }
}

#[derive(Default)]
struct Collector {
    commands: Vec<Command>,
}

impl OutlineBuilder for Collector {
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

fn add_area(area: &mut f64, from: (f64, f64), to: (f64, f64)) {
    *area += from.0 * to.1 - to.0 * from.1;
}

fn cubic_at(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    (
        u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0,
        u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1,
    )
}

fn quad_at(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    (
        u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
        u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
    )
}

/// The signed area of a set of contours (only its sign is used: which way a
/// contour winds). Curves are flattened, which is enough for a direction.
fn area(commands: &[Command]) -> f64 {
    let mut total = 0.0;
    let mut start = (0.0, 0.0);
    let mut current = (0.0, 0.0);
    for command in commands {
        match *command {
            Command::Move(x, y) => {
                start = (x, y);
                current = start;
            }
            Command::Line(x, y) => {
                add_area(&mut total, current, (x, y));
                current = (x, y);
            }
            Command::Curve(x1, y1, x2, y2, x, y) => {
                let p0 = current;
                let p1 = (x1, y1);
                let p2 = (x2, y2);
                let p3 = (x, y);
                for i in 1..=16 {
                    let point = cubic_at(p0, p1, p2, p3, i as f64 / 16.0);
                    add_area(&mut total, current, point);
                    current = point;
                }
            }
            Command::Quad(x1, y1, x, y) => {
                let p0 = current;
                let p1 = (x1, y1);
                let p2 = (x, y);
                for i in 1..=16 {
                    let point = quad_at(p0, p1, p2, i as f64 / 16.0);
                    add_area(&mut total, current, point);
                    current = point;
                }
            }
            Command::Close => {
                add_area(&mut total, current, start);
                current = start;
            }
        }
    }
    total / 2.0
}

fn reverse_contour(commands: &[Command]) -> Vec<Command> {
    // (the outlines drawn here are one closed contour each; keep a glyph's
    // contours apart all the same)
    let mut out = Vec::with_capacity(commands.len());
    let mut start = (0.0, 0.0);
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut segments: Vec<Command> = Vec::new();
    let flush =
        |out: &mut Vec<Command>, start: (f64, f64), points: &[(f64, f64)], segments: &[Command]| {
            if segments.len() < 2 {
                return;
            }
            out.push(Command::Move(points[0].0, points[0].1));
            // (a segment from points[i] to points[i + 1], walked backwards:
            // lines swap their ends, a cubic swaps its controls and ends)
            for i in (0..segments.len()).rev() {
                let from = points[i];
                match segments[i] {
                    Command::Line(_, _) => out.push(Command::Line(from.0, from.1)),
                    Command::Curve(x1, y1, x2, y2, _, _) => {
                        out.push(Command::Curve(x2, y2, x1, y1, from.0, from.1))
                    }
                    Command::Quad(x1, y1, _, _) => out.push(Command::Quad(x1, y1, from.0, from.1)),
                    _ => {}
                }
            }
            // (a contour that has no explicit close still ends at its start)
            if points.last().copied() != Some(start) {
                out.push(Command::Line(start.0, start.1));
            }
            out.push(Command::Close);
        };
    for command in commands {
        match *command {
            Command::Move(x, y) => {
                flush(&mut out, start, &points, &segments);
                start = (x, y);
                points.clear();
                segments.clear();
                points.push(start);
            }
            Command::Line(x, y) => {
                segments.push(Command::Line(x, y));
                points.push((x, y));
            }
            Command::Curve(x1, y1, x2, y2, x, y) => {
                segments.push(Command::Curve(x1, y1, x2, y2, x, y));
                points.push((x, y));
            }
            Command::Quad(x1, y1, x, y) => {
                segments.push(Command::Quad(x1, y1, x, y));
                points.push((x, y));
            }
            Command::Close => {
                flush(&mut out, start, &points, &segments);
                points.clear();
                segments.clear();
                points.push(start);
            }
        }
    }
    flush(&mut out, start, &points, &segments);
    out
}

fn push_contour(out: &mut Vec<Command>, sign: i32, contour: Vec<Command>) {
    if (area(&contour) > 0.0) == (sign > 0) {
        out.extend(contour);
    } else {
        out.extend(reverse_contour(&contour));
    }
}

fn transformed_contour(commands: &[Command], scale: f64, dx: f64, dy: f64) -> Vec<Command> {
    font::rounded_outline(commands, scale, dx, dy)
}

fn box_of(bounds: Bounds) -> (f64, f64, f64) {
    let side = (bounds.xmax - bounds.xmin).max(bounds.ymax - bounds.ymin);
    (
        side,
        (bounds.xmin + bounds.xmax) / 2.0,
        (bounds.ymin + bounds.ymax) / 2.0,
    )
}

fn rounded_rect(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> Vec<Command> {
    let r = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let k = K * r;
    vec![
        Command::Move(x0 + r, y0),
        Command::Line(x1 - r, y0),
        Command::Curve(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r),
        Command::Line(x1, y1 - r),
        Command::Curve(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1),
        Command::Line(x0 + r, y1),
        Command::Curve(x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r),
        Command::Line(x0, y0 + r),
        Command::Curve(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0),
        Command::Close,
    ]
}

fn mouse_button(a: f64, b: f64, y0: f64, top: f64, y1: f64, ry: f64, left: bool) -> Vec<Command> {
    let rx = b - a;
    let k = K * rx;
    let mut out = vec![Command::Move(a, y0), Command::Line(b, y0)];
    if left {
        out.push(Command::Line(b, y1));
        out.push(Command::Curve(b - k, y1, a, top + K * ry, a, top));
    } else {
        out.push(Command::Line(b, top));
        out.push(Command::Curve(b, top + K * ry, a + k, y1, a, y1));
    }
    out.push(Command::Close);
    out
}

pub fn mouse_glyph(which: &str, target: Bounds) -> Vec<Command> {
    let (side, cx, cy) = box_of(target);
    let x0 = cx - side / 2.0;
    let y0 = cy - side / 2.0;
    let mut out = Vec::new();
    push_contour(
        &mut out,
        1,
        rounded_rect(x0, y0, x0 + side, y0 + side, side * 0.2),
    );
    let mw = side * 0.52;
    let mh = side * 0.76;
    let mx0 = cx - mw / 2.0;
    let my0 = cy - mh / 2.0;
    let mx1 = cx + mw / 2.0;
    let my1 = cy + mh / 2.0;
    push_contour(&mut out, -1, rounded_rect(mx0, my0, mx1, my1, mw / 2.0));
    let g = side * 0.06;
    let ix0 = mx0 + g;
    let ix1 = mx1 - g;
    let iy1 = my1 - g;
    let ri = mw / 2.0 - g;
    let by = cy + side * 0.02;
    let top = iy1 - ri;
    match which {
        "left" | "right" => {
            let left = which == "left";
            let (a, b) = if left {
                (ix0, cx - g / 2.0)
            } else {
                (cx + g / 2.0, ix1)
            };
            push_contour(&mut out, 1, mouse_button(a, b, by, top, iy1, ri, left));
        }
        _ => {
            let ww = side * 0.13;
            push_contour(
                &mut out,
                1,
                rounded_rect(
                    cx - ww / 2.0,
                    by,
                    cx + ww / 2.0,
                    iy1 - side * 0.02,
                    ww / 2.0,
                ),
            );
        }
    }
    out
}

/// A key as high as the game's icon, as wide as the word needs, the word cut
/// out in the label font; how much wider than the icon it is.
pub fn word_key(text: &str, target: Bounds, font: &LabelFont) -> Option<(Vec<Command>, f64)> {
    let (side, cx, cy) = box_of(target);
    let mut names = Vec::new();
    let mut advance = 0.0;
    for ch in text.chars() {
        names.push((ch, font.advance(ch)? as f64));
        advance += font.advance(ch)? as f64;
    }
    let cap_height = font.cap_height()?;
    if cap_height <= 0.0 || advance <= 0.0 {
        return None;
    }
    let mut scale = side * 0.48 / cap_height;
    let pad = side * 0.20;
    let mut width = (advance * scale + 2.0 * pad).max(side);
    if width > side * MOST {
        scale *= (side * MOST - 2.0 * pad) / (advance * scale);
        width = side * MOST;
    }
    let x0 = cx - side / 2.0;
    let y0 = cy - side / 2.0;
    let mut out = Vec::new();
    let sign = if font.letters_are_counter_clockwise() {
        -1
    } else {
        1
    };
    push_contour(
        &mut out,
        sign,
        rounded_rect(x0, y0, x0 + width, y0 + side, side * 0.2),
    );
    let mut x = x0 + (width - advance * scale) / 2.0;
    let ty = cy - cap_height * scale / 2.0;
    for (ch, advance) in names {
        let outline = font.outline(ch)?;
        out.extend(transformed_contour(&outline, scale, x, ty));
        x += advance * scale;
    }
    Some((out, width - side))
}

/// Add a label font's advance for each key of a multi-way stick.
pub fn stick_label(
    font: &LabelFont,
    ways: &str,
    target: Bounds,
) -> Option<(String, Vec<Command>, f64)> {
    let text = ways
        .chars()
        .map(|way| move_key(way).map(|code| char::from(code)))
        .collect::<Option<String>>()?;
    let name = format!("stick_{}", text.to_lowercase());
    let (outline, over) = word_key(&text, target, font)?;
    Some((name, outline, over))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_the_icon_the_upstream_data_names() {
        assert_eq!(kenney_name(0x45).as_deref(), Some("keyboard_e"));
        assert_eq!(kenney_name(0x31).as_deref(), Some("keyboard_1"));
        assert_eq!(kenney_name(0x61).as_deref(), Some("keyboard_1"));
        assert_eq!(kenney_name(0x70).as_deref(), Some("keyboard_f1"));
        assert_eq!(kenney_name(0x01).as_deref(), Some("mouse_left"));
        assert_eq!(
            kenney_name(0x08).as_deref(),
            Some("keyboard_backspace_icon")
        );
        assert_eq!(kenney_name(0x07), None, "a key with no icon");
        assert_eq!(label(0x60).as_deref(), Some("Num0"));
        assert_eq!(mouse(0x04), Some("middle"));
    }

    #[test]
    fn the_sticks_are_the_game_codes_in_host_order() {
        assert_eq!(STICKS.len(), 15);
        assert_eq!(STICKS[0].code, 0xE050);
        assert_eq!(STICKS[14].code, 0xE068);
        assert_eq!(move_key('u'), Some(0x57));
        assert_eq!(move_key('x'), None);
    }

    #[test]
    fn the_keyboard_code_points_do_not_overlap() {
        assert!(KEY_BASE + 255 < STICK_KEYS, "keys before the sticks");
        assert!(
            STICK_KEYS + 15 < STICK_GAME,
            "the keyboard's sticks before the game's"
        );
        assert!(STICK_GAME + 15 <= 0xF8DF, "the sticks' range");
        assert!(
            SLOT_BASE + SLOTS as u16 <= 0xF000,
            "the live text slots before BIND_BASE"
        );
    }

    #[test]
    fn the_rounded_rect_has_each_turn_and_closes() {
        let rect = rounded_rect(0.0, 0.0, 100.0, 100.0, 20.0);
        assert!(matches!(rect[0], Command::Move(_, _)));
        assert!(matches!(rect.last(), Some(Command::Close)));
        assert!(area(&rect) > 0.0, "the rectangle winds counter-clockwise");
        assert!(!reverse_contour(&rect).is_empty());
    }
}
