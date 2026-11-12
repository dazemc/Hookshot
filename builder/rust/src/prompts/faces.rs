//! The faces: the controllers whose button prompts the build draws, with
//! the Kenney glyph each pad input uses, and where each control is in the
//! pause menu's picture (`nx2/prompt_faces.py`). The picture's drawing data
//! (the body glyph or the Steam Deck's own shape, and each control) is here
//! too; `textures.rs` draws it.

use std::collections::BTreeMap;

pub const PADS: [&str; 17] = [
    "south",
    "east",
    "west",
    "north",
    "back",
    "guide",
    "start",
    "lstick",
    "rstick",
    "lshoulder",
    "rshoulder",
    "up",
    "down",
    "left",
    "right",
    "ltrigger",
    "rtrigger",
];

/// The Switch buttons, in HOS_SW_* order.
pub const SWITCH: [u16; 16] = [
    0xE040, 0xE041, 0xE042, 0xE043, 0xE056, 0xE057, 0xE044, 0xE045, 0xE046, 0xE047, 0xE04B, 0xE04C,
    0xE062, 0xE060, 0xE063, 0xE061,
];

/// The Switch buttons, by name, in the same order.
pub const SWITCH_NAMES: [&str; 16] = [
    "a", "b", "x", "y", "ls", "rs", "l", "r", "zl", "zr", "plus", "minus", "left", "up", "right",
    "down",
];

/// hookshot's default controls, by HOS_SW_* order (`host/nn_hid.c`).
#[allow(dead_code)] // (the textures' per-face pictures use it)
pub const DEFAULT: [&str; 16] = [
    "east",
    "south",
    "north",
    "west",
    "lstick",
    "rstick",
    "lshoulder",
    "rshoulder",
    "ltrigger",
    "rtrigger",
    "start",
    "back",
    "left",
    "up",
    "right",
    "down",
];

/// The game's own icon for each pad input: what the Switch face shows.
pub const SWITCH_ICON: [(&str, u16); 17] = [
    ("south", 0xE041),
    ("east", 0xE040),
    ("west", 0xE043),
    ("north", 0xE042),
    ("back", 0xE04C),
    ("guide", 0xE04A),
    ("start", 0xE04B),
    ("lstick", 0xE056),
    ("rstick", 0xE057),
    ("lshoulder", 0xE044),
    ("rshoulder", 0xE045),
    ("up", 0xE060),
    ("down", 0xE061),
    ("left", 0xE062),
    ("right", 0xE063),
    ("ltrigger", 0xE046),
    ("rtrigger", 0xE047),
];

/// The icons that do not follow the controls, by the Kenney glyph's name
/// after the face's prefix: the whole d-pad (the sticks keep the game's).
pub const STATIC: [(u16, &str); 3] = [
    (0xE049, "dpad"),
    (0xE064, "dpad_vertical_outline"),
    (0xE065, "dpad_horizontal_outline"),
];

pub const PAD_BASE: u16 = 0xF700;
pub const FACE_CODES: u16 = 32;
pub const STATIC_AT: u16 = 24;
pub const BIND_BASE: u16 = 0xF000;

/// The faces in HOS_PROMPT_* order: the Switch, the controllers, then the
/// keyboard (whose keys are not a Kenney controller font).
pub const NAMES: [&str; 6] = ["switch", "xbox", "ps4", "ps5", "steamdeck", "keyboard"];
pub const KEYBOARD: &str = "keyboard";

/// How much wider than the game's icon a wide Kenney icon may be.
pub const WIDE: f64 = 1.6;

pub struct Face {
    pub name: &'static str,
    pub font: &'static str,
    pub prefix: &'static str,
    /// Each pad input's glyph name after the prefix; `/name` is the whole
    /// name. An input left out keeps the game's icon.
    pub icons: &'static [(&'static str, &'static str)],
    /// The pause menu's controller picture (`nx2/prompt_faces.py`'s
    /// `drawing`): None for the Switch, whose picture is the game's.
    pub drawing: Option<&'static Drawing>,
}

/// A control of a drawn controller body: its kind, centre in the body's box
/// (0..1), size (a fraction of the body's width, or (width, height) for a
/// touchpad or a screen) and the pad input whose label it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlKind {
    Stick,
    Dpad,
    Face,
    Small,
    Pad,
    Screen,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    One(f64),
    Two(f64, f64),
}

impl Size {
    pub fn width(self) -> f64 {
        match self {
            Size::One(value) => value,
            Size::Two(width, _) => width,
        }
    }

    pub fn height(self) -> f64 {
        match self {
            Size::One(value) => value,
            Size::Two(_, height) => height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    pub kind: ControlKind,
    pub cx: f64,
    pub cy: f64,
    pub size: Size,
    pub what: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawing {
    /// The Kenney font and glyph of the body; None is the Steam Deck's own
    /// shape (`nx2/prompt_draw.py`'s `deck`).
    pub font: Option<&'static str>,
    pub glyph: Option<&'static str>,
    pub controls: &'static [Control],
}

const fn control(
    kind: ControlKind,
    cx: f64,
    cy: f64,
    size: Size,
    what: Option<&'static str>,
) -> Control {
    Control {
        kind,
        cx,
        cy,
        size,
        what,
    }
}

const XBOX_CONTROLS: [Control; 11] = [
    control(
        ControlKind::Stick,
        0.215,
        0.262,
        Size::One(0.158),
        Some("lstick"),
    ),
    control(ControlKind::Dpad, 0.345, 0.530, Size::One(0.160), None),
    control(
        ControlKind::Stick,
        0.640,
        0.495,
        Size::One(0.158),
        Some("rstick"),
    ),
    control(
        ControlKind::Face,
        0.700,
        0.275,
        Size::One(0.080),
        Some("west"),
    ),
    control(
        ControlKind::Face,
        0.775,
        0.375,
        Size::One(0.080),
        Some("south"),
    ),
    control(
        ControlKind::Face,
        0.775,
        0.175,
        Size::One(0.080),
        Some("north"),
    ),
    control(
        ControlKind::Face,
        0.850,
        0.275,
        Size::One(0.080),
        Some("east"),
    ),
    control(
        ControlKind::Small,
        0.498,
        0.120,
        Size::One(0.090),
        Some("guide"),
    ),
    control(
        ControlKind::Small,
        0.420,
        0.275,
        Size::One(0.052),
        Some("back"),
    ),
    control(
        ControlKind::Small,
        0.576,
        0.275,
        Size::One(0.052),
        Some("start"),
    ),
    control(ControlKind::Small, 0.498, 0.355, Size::One(0.044), None),
];

const PS4_CONTROLS: [Control; 11] = [
    control(ControlKind::Dpad, 0.195, 0.290, Size::One(0.180), None),
    control(
        ControlKind::Stick,
        0.340,
        0.520,
        Size::One(0.150),
        Some("lstick"),
    ),
    control(
        ControlKind::Stick,
        0.660,
        0.520,
        Size::One(0.150),
        Some("rstick"),
    ),
    control(
        ControlKind::Pad,
        0.499,
        0.200,
        Size::Two(0.320, 0.280),
        None,
    ),
    control(
        ControlKind::Face,
        0.742,
        0.285,
        Size::One(0.072),
        Some("west"),
    ),
    control(
        ControlKind::Face,
        0.814,
        0.175,
        Size::One(0.072),
        Some("north"),
    ),
    control(
        ControlKind::Face,
        0.814,
        0.395,
        Size::One(0.072),
        Some("south"),
    ),
    control(
        ControlKind::Face,
        0.886,
        0.285,
        Size::One(0.072),
        Some("east"),
    ),
    control(
        ControlKind::Small,
        0.298,
        0.130,
        Size::One(0.036),
        Some("back"),
    ),
    control(
        ControlKind::Small,
        0.702,
        0.130,
        Size::One(0.036),
        Some("start"),
    ),
    control(
        ControlKind::Small,
        0.500,
        0.510,
        Size::One(0.046),
        Some("guide"),
    ),
];

const PS5_CONTROLS: [Control; 12] = [
    control(ControlKind::Dpad, 0.170, 0.265, Size::One(0.190), None),
    control(
        ControlKind::Stick,
        0.320,
        0.490,
        Size::One(0.145),
        Some("lstick"),
    ),
    control(
        ControlKind::Stick,
        0.680,
        0.490,
        Size::One(0.145),
        Some("rstick"),
    ),
    control(
        ControlKind::Pad,
        0.498,
        0.175,
        Size::Two(0.400, 0.300),
        None,
    ),
    control(
        ControlKind::Face,
        0.765,
        0.265,
        Size::One(0.078),
        Some("west"),
    ),
    control(
        ControlKind::Face,
        0.840,
        0.150,
        Size::One(0.078),
        Some("north"),
    ),
    control(
        ControlKind::Face,
        0.840,
        0.380,
        Size::One(0.078),
        Some("south"),
    ),
    control(
        ControlKind::Face,
        0.915,
        0.265,
        Size::One(0.078),
        Some("east"),
    ),
    control(
        ControlKind::Small,
        0.248,
        0.080,
        Size::One(0.036),
        Some("back"),
    ),
    control(
        ControlKind::Small,
        0.750,
        0.080,
        Size::One(0.036),
        Some("start"),
    ),
    control(
        ControlKind::Small,
        0.496,
        0.480,
        Size::One(0.046),
        Some("guide"),
    ),
    control(ControlKind::Small, 0.496, 0.575, Size::One(0.030), None),
];

const STEAMDECK_CONTROLS: [Control; 14] = [
    control(
        ControlKind::Screen,
        0.500,
        0.465,
        Size::Two(0.580, 0.860),
        None,
    ),
    control(ControlKind::Dpad, 0.072, 0.200, Size::One(0.078), None),
    control(
        ControlKind::Stick,
        0.158,
        0.220,
        Size::One(0.085),
        Some("lstick"),
    ),
    control(
        ControlKind::Stick,
        0.842,
        0.220,
        Size::One(0.085),
        Some("rstick"),
    ),
    control(
        ControlKind::Pad,
        0.140,
        0.510,
        Size::Two(0.112, 0.270),
        None,
    ),
    control(
        ControlKind::Pad,
        0.860,
        0.510,
        Size::Two(0.112, 0.270),
        None,
    ),
    control(
        ControlKind::Face,
        0.935,
        0.090,
        Size::One(0.036),
        Some("north"),
    ),
    control(
        ControlKind::Face,
        0.935,
        0.270,
        Size::One(0.036),
        Some("south"),
    ),
    control(
        ControlKind::Face,
        0.900,
        0.180,
        Size::One(0.036),
        Some("west"),
    ),
    control(
        ControlKind::Face,
        0.970,
        0.180,
        Size::One(0.036),
        Some("east"),
    ),
    control(
        ControlKind::Small,
        0.125,
        0.075,
        Size::One(0.024),
        Some("back"),
    ),
    control(
        ControlKind::Small,
        0.875,
        0.075,
        Size::One(0.024),
        Some("start"),
    ),
    control(
        ControlKind::Small,
        0.150,
        0.755,
        Size::One(0.030),
        Some("guide"),
    ),
    control(ControlKind::Small, 0.850, 0.755, Size::One(0.030), None),
];

const XBOX_DRAWING: Drawing = Drawing {
    font: Some("kenney_input_xbox_series.otf"),
    glyph: Some("controller_xboxseries"),
    controls: &XBOX_CONTROLS,
};

const PS4_DRAWING: Drawing = Drawing {
    font: Some("kenney_input_playstation_series.otf"),
    glyph: Some("controller_playstation4"),
    controls: &PS4_CONTROLS,
};

const PS5_DRAWING: Drawing = Drawing {
    font: Some("kenney_input_playstation_series.otf"),
    glyph: Some("controller_playstation5"),
    controls: &PS5_CONTROLS,
};

const STEAMDECK_DRAWING: Drawing = Drawing {
    font: None,
    glyph: None,
    controls: &STEAMDECK_CONTROLS,
};

/// The faces after the Switch, in HOS_PROMPT_* order.
pub const FACES: [Face; 4] = [
    Face {
        name: "xbox",
        font: "kenney_input_xbox_series.otf",
        prefix: "xbox_",
        icons: &[
            ("south", "button_a"),
            ("east", "button_b"),
            ("west", "button_x"),
            ("north", "button_y"),
            ("back", "button_view"),
            ("start", "button_menu"),
            ("lshoulder", "lb"),
            ("rshoulder", "rb"),
            ("ltrigger", "lt"),
            ("rtrigger", "rt"),
            ("lstick", "ls"),
            ("rstick", "rs"),
            ("up", "dpad_up_outline"),
            ("down", "dpad_down_outline"),
            ("left", "dpad_left_outline"),
            ("right", "dpad_right_outline"),
        ],
        drawing: Some(&XBOX_DRAWING),
    },
    Face {
        name: "ps4",
        font: "kenney_input_playstation_series.otf",
        prefix: "playstation_",
        icons: &[
            ("south", "button_cross"),
            ("east", "button_circle"),
            ("west", "button_square"),
            ("north", "button_triangle"),
            ("back", "/playstation4_button_share"),
            ("start", "/playstation4_button_options"),
            ("lshoulder", "trigger_l1"),
            ("rshoulder", "trigger_r1"),
            ("ltrigger", "trigger_l2"),
            ("rtrigger", "trigger_r2"),
            ("lstick", "button_l3"),
            ("rstick", "button_r3"),
            ("up", "dpad_up_outline"),
            ("down", "dpad_down_outline"),
            ("left", "dpad_left_outline"),
            ("right", "dpad_right_outline"),
        ],
        drawing: Some(&PS4_DRAWING),
    },
    Face {
        name: "ps5",
        font: "kenney_input_playstation_series.otf",
        prefix: "playstation_",
        icons: &[
            ("south", "button_cross"),
            ("east", "button_circle"),
            ("west", "button_square"),
            ("north", "button_triangle"),
            ("back", "/playstation5_button_create"),
            ("start", "/playstation5_button_options"),
            ("lshoulder", "trigger_l1"),
            ("rshoulder", "trigger_r1"),
            ("ltrigger", "trigger_l2"),
            ("rtrigger", "trigger_r2"),
            ("lstick", "button_l3"),
            ("rstick", "button_r3"),
            ("up", "dpad_up_outline"),
            ("down", "dpad_down_outline"),
            ("left", "dpad_left_outline"),
            ("right", "dpad_right_outline"),
        ],
        drawing: Some(&PS5_DRAWING),
    },
    Face {
        name: "steamdeck",
        font: "kenney_input_steam_deck.otf",
        prefix: "steamdeck_",
        icons: &[
            ("south", "button_a"),
            ("east", "button_b"),
            ("west", "button_x"),
            ("north", "button_y"),
            ("back", "button_view"),
            ("start", "button_options"),
            ("lshoulder", "button_l1"),
            ("rshoulder", "button_r1"),
            ("ltrigger", "button_l2"),
            ("rtrigger", "button_r2"),
            ("up", "dpad_up_outline"),
            ("down", "dpad_down_outline"),
            ("left", "dpad_left_outline"),
            ("right", "dpad_right_outline"),
        ],
        drawing: Some(&STEAMDECK_DRAWING),
    },
];

/// A control's place in a face's pause-menu picture: the leader lines end on
/// it, and keep off it (`nx2/prompt_faces.py`'s `parts`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    /// A control's centre and radius, in picture pixels.
    Circle { x: f64, y: f64, r: f64 },
    /// A box the lines keep off: (x0, y0, x1, y1), in picture pixels.
    Box { x0: f64, y0: f64, x1: f64, y1: f64 },
}

pub type Parts = BTreeMap<&'static str, Part>;

fn circle(x: f64, y: f64, r: f64) -> Part {
    Part::Circle { x, y, r }
}

fn area(x0: f64, y0: f64, x1: f64, y1: f64) -> Part {
    Part::Box { x0, y0, x1, y1 }
}

/// `_parts` of `nx2/prompt_faces.py`: the picture's parts from one axis of
/// symmetry, its sticks, d-pad, face buttons, small buttons, Home, shoulders
/// and boxes. The right-hand controls are mirrored on `axis`.
#[allow(clippy::too_many_arguments)]
fn parts_of(
    axis: f64,
    sticks: ((f64, f64, f64), (f64, f64, f64)),
    dpad: (f64, f64, f64, f64),
    face: ((f64, f64), (f64, f64), (f64, f64), (f64, f64), f64),
    small: (f64, f64, f64),
    home: (f64, f64, f64),
    shoulders: ((f64, f64), (f64, f64)),
    boxes: &[(&'static str, (f64, f64, f64, f64))],
) -> Parts {
    let ((ls, rs), (dx, dy, arm, ar)) = (sticks, dpad);
    let ((n, w, e, s, fr), (bx, by, br)) = (face, small);
    let ((tx, ty), (sx, sy)) = shoulders;
    let mut out = Parts::new();
    out.insert("lstick", circle(ls.0, ls.1, ls.2));
    out.insert("rstick", circle(rs.0, rs.1, rs.2));
    out.insert("up", circle(dx, dy - arm, ar));
    out.insert("down", circle(dx, dy + arm, ar));
    out.insert("left", circle(dx - arm, dy, ar));
    out.insert("right", circle(dx + arm, dy, ar));
    out.insert("north", circle(n.0, n.1, fr));
    out.insert("west", circle(w.0, w.1, fr));
    out.insert("east", circle(e.0, e.1, fr));
    out.insert("south", circle(s.0, s.1, fr));
    out.insert("back", circle(bx, by, br));
    out.insert("start", circle(axis - bx, by, br));
    out.insert("guide", circle(home.0, home.1, home.2));
    out.insert("ltrigger", circle(tx, ty, 0.0));
    out.insert("lshoulder", circle(sx, sy, 0.0));
    out.insert("rtrigger", circle(axis - tx, ty, 0.0));
    out.insert("rshoulder", circle(axis - sx, sy, 0.0));
    for (name, (x0, y0, x1, y1)) in boxes {
        out.insert(name, area(*x0, *y0, *x1, *y1));
    }
    out
}

/// The game's own Pro Controller picture (`SWITCH_PARTS`).
fn switch_parts() -> Parts {
    parts_of(
        1159.0,
        ((348.0, 227.0, 48.0), (683.0, 348.0, 48.0)),
        (455.0, 348.0, 45.0, 17.0),
        (
            (800.0, 168.0),
            (731.0, 227.0),
            (869.0, 227.0),
            (800.0, 286.0),
            32.0,
        ),
        (477.0, 161.0, 19.0),
        (631.0, 227.0, 21.0),
        ((375.0, 39.0), (290.0, 60.0)),
        &[],
    )
}

/// The picture's parts for a face (`parts` of `nx2/prompt_faces.py`), or
/// None for a face without a picture.
pub fn parts(face: &str) -> Option<Parts> {
    let out = match face {
        "switch" => switch_parts(),
        "xbox" => parts_of(
            1156.0,
            ((343.0, 210.0, 43.0), (696.0, 344.0, 43.0)),
            (451.0, 364.0, 44.0, 16.0),
            (
                (809.0, 160.0),
                (746.0, 218.0),
                (871.0, 218.0),
                (809.0, 275.0),
                31.0,
            ),
            (513.0, 218.0, 19.0),
            (578.0, 129.0, 34.0),
            ((330.0, 59.0), (470.0, 57.0)),
            &[("share", (558.0, 243.0, 598.0, 283.0))],
        ),
        "ps4" => parts_of(
            1160.0,
            ((447.0, 357.0, 40.0), (713.0, 357.0, 40.0)),
            (326.0, 237.0, 49.0, 18.0),
            (
                (841.0, 177.0),
                (781.0, 234.0),
                (901.0, 234.0),
                (841.0, 292.0),
                28.0,
            ),
            (412.0, 153.0, 12.0),
            (580.0, 352.0, 16.0),
            ((320.0, 67.0), (408.0, 97.0)),
            &[("touchpad", (446.0, 117.0, 712.0, 263.0))],
        ),
        "ps5" => parts_of(
            1158.0,
            ((430.0, 341.0, 39.0), (730.0, 341.0, 39.0)),
            (305.0, 217.0, 51.0, 19.0),
            (
                (863.0, 153.0),
                (800.0, 217.0),
                (925.0, 217.0),
                (863.0, 281.0),
                30.0,
            ),
            (370.0, 114.0, 12.0),
            (577.0, 336.0, 16.0),
            ((321.0, 52.0), (383.0, 70.0)),
            &[
                ("touchpad", (412.0, 84.0, 744.0, 250.0)),
                ("mute", (563.0, 375.0, 591.0, 403.0)),
            ],
        ),
        "steamdeck" => parts_of(
            1159.0,
            ((242.0, 230.0, 27.0), (917.0, 230.0, 27.0)),
            (157.0, 222.0, 25.0, 9.0),
            (
                (1009.0, 180.0),
                (974.0, 215.0),
                (1043.0, 215.0),
                (1009.0, 249.0),
                16.0,
            ),
            (209.0, 174.0, 9.0),
            (234.0, 437.0, 12.0),
            ((115.0, 153.0), (165.0, 144.0)),
            &[
                ("screen", (294.0, 159.0, 866.0, 491.0)),
                ("ltrackpad", (169.0, 290.0, 279.0, 394.0)),
                ("rtrackpad", (880.0, 290.0, 990.0, 394.0)),
                ("more", (908.0, 420.0, 942.0, 454.0)),
            ],
        ),
        _ => return None,
    };
    Some(out)
}

pub fn switch_icon(pad: &str) -> Option<u16> {
    SWITCH_ICON
        .iter()
        .find(|(name, _)| *name == pad)
        .map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_faces_are_the_switch_then_the_controllers_then_the_keyboard() {
        assert_eq!(NAMES[0], "switch");
        assert_eq!(KEYBOARD, NAMES[NAMES.len() - 1]);
        let controllers = NAMES[1..NAMES.len() - 1]
            .iter()
            .map(|name| *name)
            .collect::<Vec<_>>();
        let expected = FACES.iter().map(|face| face.name).collect::<Vec<_>>();
        assert_eq!(controllers, expected);
        assert_eq!(
            NAMES,
            ["switch", "xbox", "ps4", "ps5", "steamdeck", "keyboard"]
        );
    }

    #[test]
    fn every_face_has_room_before_the_keyboard_keys() {
        // (host/prompts.c KEY_BASE: PAD_BASE + FACE_CODES x HOS_PROMPT_N)
        assert_eq!(
            PAD_BASE + FACE_CODES * NAMES.len() as u16,
            super::super::keys::KEY_BASE
        );
        assert!(BIND_BASE + NAMES.len() as u16 * SWITCH.len() as u16 * 17 <= PAD_BASE);
        assert!(STATIC_AT + STATIC.len() as u16 <= FACE_CODES);
        assert!((PADS.len() as u16) <= STATIC_AT);
    }

    #[test]
    fn every_face_has_a_drawing_like_the_reference() {
        for face in &FACES {
            let drawing = face.drawing.expect("a controller face has a drawing");
            assert!(!drawing.controls.is_empty());
            for control in drawing.controls {
                assert!(control.size.width() > 0.0 && control.size.height() > 0.0);
                assert!((0.0..=1.0).contains(&control.cx));
                assert!((0.0..=1.0).contains(&control.cy));
                if let Some(what) = control.what {
                    assert!(
                        PADS.contains(&what),
                        "{}: unknown pad input {what}",
                        face.name
                    );
                }
            }
        }
        let xbox = FACES.iter().find(|face| face.name == "xbox").unwrap();
        let drawing = xbox.drawing.unwrap();
        assert_eq!(drawing.font, Some("kenney_input_xbox_series.otf"));
        assert_eq!(drawing.glyph, Some("controller_xboxseries"));
        assert_eq!(drawing.controls.len(), 11);
        assert_eq!(drawing.controls[0].kind, ControlKind::Stick);
        assert_eq!(drawing.controls[0].what, Some("lstick"));
        let deck = FACES.iter().find(|face| face.name == "steamdeck").unwrap();
        let drawing = deck.drawing.unwrap();
        assert_eq!(drawing.font, None);
        assert_eq!(drawing.glyph, None);
        assert_eq!(drawing.controls.len(), 14);
        assert_eq!(drawing.controls[0].kind, ControlKind::Screen);
        assert_eq!(drawing.controls[0].size, Size::Two(0.580, 0.860));
    }

    #[test]
    fn the_switch_buttons_and_code_points_are_aligned() {
        assert_eq!(SWITCH.len(), DEFAULT.len());
        assert_eq!(
            SWITCH.len() + 1,
            SWITCH_ICON.len(),
            "the guide has an icon too"
        );
        for code in SWITCH {
            assert!(
                SWITCH_ICON.iter().any(|(_, icon)| *icon == code),
                "U+{code:04X} has a Switch icon"
            );
        }
        for (button, default) in DEFAULT.iter().enumerate() {
            assert!(
                PADS.contains(default),
                "button {button} defaults to a pad input"
            );
        }
        for pad in PADS {
            assert!(
                switch_icon(pad).is_some(),
                "the Switch has an icon for {pad}"
            );
        }
        assert_eq!(switch_icon("east"), Some(0xE040));
        assert_eq!(switch_icon("nope"), None);
    }
}
