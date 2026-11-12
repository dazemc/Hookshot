//! The faces: the controllers whose button prompts the build draws, with
//! the Kenney glyph each pad input uses (`nx2/prompt_faces.py`).

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

/// How much wider than the game's icon a wide Kenney icon may be.
pub const WIDE: f64 = 1.6;

pub struct Face {
    pub name: &'static str,
    pub font: &'static str,
    pub prefix: &'static str,
    /// Each pad input's glyph name after the prefix; `/name` is the whole
    /// name. An input left out keeps the game's icon.
    pub icons: &'static [(&'static str, &'static str)],
}

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
    },
];

pub fn switch_icon(pad: &str) -> Option<u16> {
    SWITCH_ICON
        .iter()
        .find(|(name, _)| *name == pad)
        .map(|(_, code)| *code)
}
