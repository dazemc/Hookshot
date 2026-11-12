// The Keyboard face of the button prompts (host/prompts.c, nx2/prompt_keys.py, builder/rust/src/prompts/{faces,keys}.rs):
// Auto picks it when no controller is there and the build made it, a Steam Controller gets the Deck's face, and every face
// keeps its pad and d-pad icons. Game-free: the host's face_of_name/auto_face are compiled and run, and the sources are
// read for the faces' order, the code points and the menu row. No game, dump or GPU is needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A C function's or enum's braces, matched, from its first line.
fn block(text: &str, marker: &str) -> String {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + text[start..].find('{').expect("opening brace") + 1;
    let bytes = text.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    text[start..end].to_string()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hookshot-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

fn compiler() -> Option<String> {
    let candidates =
        std::env::var("CC")
            .ok()
            .into_iter()
            .chain(["cc".into(), "gcc".into(), "clang".into()]);
    for candidate in candidates {
        if Command::new(&candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

fn number_after(text: &str, marker: &str) -> u64 {
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let rest = text[at + marker.len()..].trim_start();
    if let Some(hex) = rest.strip_prefix("0x") {
        let digits: String = hex
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        u64::from_str_radix(&digits, 16).unwrap_or_else(|_| panic!("{marker}: {digits}"))
    } else {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits
            .parse()
            .unwrap_or_else(|_| panic!("{marker}: {digits}"))
    }
}

fn define(text: &str, name: &str) -> String {
    text.lines()
        .find(|line| line.starts_with(&format!("#define {name} ")))
        .unwrap_or_else(|| panic!("missing #define {name}"))
        .to_owned()
}

fn bracket(text: &str, marker: &str, open: char, close: char) -> String {
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    // (a Rust type's own brackets come before the value's: start after the `=` when there is one)
    let from = text[at..]
        .find('=')
        .map(|offset| at + offset)
        .unwrap_or(at);
    let start = from + text[from..].find(open).expect("initializer") + open.len_utf8();
    let end = start + text[start..].find(close).expect("initializer end");
    text[start..end].to_owned()
}

fn initializer(text: &str, marker: &str) -> String {
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let start = at + text[at..].find('{').expect("initializer") + 1;
    let end = start + text[start..].find('}').expect("initializer end");
    text[start..end].to_owned()
}

const NAMES: &[(&str, &str)] = &[
    ("", "HOS_PROMPT_SWITCH"),
    ("Steam Controller", "HOS_PROMPT_STEAMDECK"),
    ("Steam Deck", "HOS_PROMPT_STEAMDECK"),
    ("Steam Deck Controller", "HOS_PROMPT_STEAMDECK"),
    ("Steam Virtual Gamepad", "HOS_PROMPT_XBOX"),
    ("DualSense Wireless Controller", "HOS_PROMPT_PS5"),
    ("PS4 Controller", "HOS_PROMPT_PS4"),
    ("Nintendo Switch Pro Controller", "HOS_PROMPT_SWITCH"),
    ("Xbox 360 Controller", "HOS_PROMPT_XBOX"),
    ("8BitDo Ultimate 2C", "HOS_PROMPT_XBOX"),
];

#[test]
fn auto_picks_the_keyboard_and_each_controller_keeps_its_face() {
    let prompts = source("host/prompts.c");
    let api = source("gpu/vk/vk_api.h");
    let menu = source("gpu/vk/vk_menu.cpp");
    let options = source("gpu/vk/vk_menu.h");
    let faces = source("builder/rust/src/prompts/faces.rs");
    let keys = source("builder/rust/src/prompts/keys.rs");
    let python_keys = source("nx2/prompt_keys.py");

    // Auto asks face_of_name; choose_face and live_face ask Auto, so a new face or new controls show while the game runs.
    assert!(block(&prompts, "static int auto_face(").contains("face_of_name(name)"));
    assert!(block(&prompts, "static int choose_face(").contains("auto_face(name)"));
    assert!(block(&prompts, "static int live_face(").contains("auto_face(name)"));

    // faces[] is HOS_PROMPT_* order: the Switch, the controllers, then the keyboard.
    let names: Vec<String> = initializer(&prompts, "faces[HOS_PROMPT_N]")
        .split(',')
        .filter_map(|part| {
            Some(
                part.trim()
                    .strip_prefix('"')?
                    .strip_suffix('"')?
                    .to_owned(),
            )
        })
        .collect();
    assert_eq!(
        names,
        ["switch", "xbox", "ps4", "ps5", "steamdeck", "keyboard"]
    );
    let ids: Vec<String> = initializer(&api, "enum { HOS_PROMPT_SWITCH")
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    assert_eq!(
        ids,
        [
            "HOS_PROMPT_SWITCH",
            "HOS_PROMPT_XBOX",
            "HOS_PROMPT_PS4",
            "HOS_PROMPT_PS5",
            "HOS_PROMPT_STEAMDECK",
            "HOS_PROMPT_KEYBOARD",
            "HOS_PROMPT_N",
        ]
    );
    assert!(faces.contains(
        "pub const NAMES: [&str; 6] = [\"switch\", \"xbox\", \"ps4\", \"ps5\", \"steamdeck\", \"keyboard\"];"
    ));

    // The host's defaults and code points are the Rust builder's (the keyboard face is built from them).
    let pads: Vec<String> = initializer(&prompts, "switch_pad[HOS_SW_N]")
        .split(',')
        .filter_map(|part| {
            let name = part.trim().strip_prefix("HOS_PAD_")?;
            Some(name.to_ascii_lowercase())
        })
        .collect();
    let rust_defaults = bracket(&faces, "pub const DEFAULT:", '[', ']');
    let rust_defaults: Vec<String> = rust_defaults
        .split(',')
        .filter_map(|part| part.trim().strip_prefix('"')?.strip_suffix('"'))
        .map(str::to_owned)
        .collect();
    assert_eq!(pads, rust_defaults, "host/prompts.c switch_pad vs faces.rs DEFAULT");

    let statics: Vec<u64> = initializer(&prompts, "static_code[3]")
        .split(',')
        .filter_map(|part| u64::from_str_radix(part.trim().trim_start_matches("0x"), 16).ok())
        .collect();
    let rust_statics: Vec<u64> = bracket(&faces, "pub const STATIC:", '[', ']')
        .split(',')
        .filter_map(|part| {
            let value = part.trim().strip_prefix('(')?;
            u64::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
        })
        .collect();
    assert_eq!(statics, rust_statics, "host/prompts.c static_code vs faces.rs STATIC");

    let sticks: Vec<u64> = initializer(&prompts, "stick_code[15]")
        .split(',')
        .filter_map(|part| u64::from_str_radix(part.trim().trim_start_matches("0x"), 16).ok())
        .collect();
    let rust_sticks: Vec<u64> = keys
        .match_indices("code: 0x")
        .map(|(at, _)| number_after(&keys[at..], "code: "))
        .collect();
    assert_eq!(sticks, rust_sticks, "host/prompts.c stick_code vs keys.rs STICKS");

    // The keyboard face's code points: the host and the legacy Python data agree with the Rust builder.
    assert_eq!(number_after(&prompts, "#define PAD_BASE "), 0xF700);
    assert_eq!(number_after(&prompts, "#define FACE_CODES "), 32);
    assert_eq!(number_after(&prompts, "#define STATIC_AT "), 24);
    assert_eq!(number_after(&prompts, "#define BIND_BASE "), 0xF000);
    assert_eq!(number_after(&faces, "pub const BIND_BASE: u16 = "), 0xF000);
    assert_eq!(number_after(&keys, "pub const KEY_BASE: u16 = "), 0xF7C0);
    assert_eq!(number_after(&keys, "pub const SLOT_BASE: u16 = "), 0xEEC0);
    assert_eq!(number_after(&keys, "pub const STICK_KEYS: u16 = "), 0xF8C0);
    assert_eq!(number_after(&keys, "pub const STICK_GAME: u16 = "), 0xF8D0);
    assert_eq!(number_after(&python_keys, "KEY_BASE = "), 0xF7C0);
    assert_eq!(number_after(&python_keys, "SLOT_BASE, SLOTS = "), 0xEEC0);
    assert_eq!(number_after(&python_keys, "STICK_KEYS, STICK_GAME = "), 0xF8C0);
    assert!(prompts.contains("#define KEY_BASE (PAD_BASE + FACE_CODES * HOS_PROMPT_N)"));
    assert!(keys.contains("pub const SLOTS: usize = 320;"));
    assert!(prompts.contains("#define SLOTS 320"));

    // The menu's row, its saved option and its hint name the keyboard's face.
    assert!(options.contains("int prompts = 0;"));
    assert!(api.contains("enum { HOS_OPT_PROMPTS = 46 };"));
    assert!(menu.contains("\"Button prompts\", &opts.prompts"));
    assert!(menu.contains("const char* tx_prompts(int v)"));
    assert!(menu.contains("A Steam Deck's."));
    assert!(menu.contains("The keys and mouse buttons your controls put each action on."));

    let Some(cc) = compiler() else {
        eprintln!("skipping the prompt face fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let harness = format!(
        r#"#include <ctype.h>
#include <stdio.h>
#include <string.h>
#include "gpu/vk/vk_api.h"
static int pads_xinput;
static int hos_xinput_pads(void) {{ return pads_xinput; }}
static int keyboard_built;
{contains}
{face_of_name}
{auto_face}
int main(void) {{
    static const struct {{ const char *name; int face; }} names[] = {{
{names}
    }};
    for (unsigned i = 0; i < sizeof names / sizeof names[0]; i++) {{
        const int got = face_of_name(names[i].name);
        if (got != names[i].face) {{
            printf("FAIL \"%s\": face %d, not %d\n", names[i].name, got, names[i].face);
            return 1;
        }}
    }}
    keyboard_built = 1;
    if (auto_face("") != HOS_PROMPT_KEYBOARD) {{ puts("FAIL no controller, keyboard built"); return 2; }}
    if (auto_face("Xbox 360 Controller") != HOS_PROMPT_XBOX) {{ puts("FAIL a controller's name"); return 3; }}
    pads_xinput = 1;
    if (auto_face("") != HOS_PROMPT_SWITCH) {{ puts("FAIL XInput with no SDL name"); return 4; }}
    pads_xinput = 0, keyboard_built = 0;
    if (auto_face("") != HOS_PROMPT_SWITCH) {{ puts("FAIL no controller, no keyboard face"); return 5; }}
    puts("ok");
    return 0;
}}
"#,
        contains = block(&prompts, "static int contains("),
        face_of_name = block(&prompts, "static int face_of_name("),
        auto_face = block(&prompts, "static int auto_face("),
        names = NAMES
            .iter()
            .map(|(name, face)| format!("        {{\"{name}\", {face}}},"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let dir = scratch("prompt-face");
    let fixture = dir.join("face.c");
    fs::write(&fixture, harness).expect("write the face fixture");
    let exe = dir.join(if cfg!(windows) { "face.exe" } else { "face" });
    let build = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", root().display()))
        .arg(&fixture)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the face fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}

/// `cmap4` as nx2/prompt_assets.py and builder/rust/src/prompts/cmap.rs write it: a format 4 subtable where the codes in
/// `single` get a segment of their own and consecutive codes with consecutive glyphs share one.
fn cmap4(mapping: &[(u16, u16)], single: &[u16]) -> Vec<u8> {
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

#[test]
fn the_keyboard_face_points_each_button_at_its_first_key() {
    // (the host's cmap4_bind on a cmap made as the build makes it: a bound Switch button gets its first key's icon, an
    // unbound one its default pad input's, a paddle without artwork its default's, the d-pad the face's, the sticks the
    // keyboard's or the game's own)
    let prompts = source("host/prompts.c");
    let faces = source("builder/rust/src/prompts/faces.rs");
    let keys = source("builder/rust/src/prompts/keys.rs");

    let switch: Vec<u16> = initializer(&prompts, "switch_code[HOS_SW_N]")
        .split(',')
        .filter_map(|part| {
            u16::from_str_radix(part.trim().trim_start_matches("0x"), 16).ok()
        })
        .collect();
    let statics: Vec<u16> = initializer(&prompts, "static_code[3]")
        .split(',')
        .filter_map(|part| {
            u16::from_str_radix(part.trim().trim_start_matches("0x"), 16).ok()
        })
        .collect();
    let sticks: Vec<u16> = initializer(&prompts, "stick_code[15]")
        .split(',')
        .filter_map(|part| {
            u16::from_str_radix(part.trim().trim_start_matches("0x"), 16).ok()
        })
        .collect();
    let faces_n: u16 = bracket(&faces, "pub const NAMES:", '[', ']')
        .split(',')
        .filter(|part| !part.trim().is_empty())
        .count() as u16;
    let pads_n: u16 = bracket(&faces, "pub const PADS:", '[', ']')
        .split(',')
        .filter(|part| !part.trim().is_empty())
        .count() as u16;
    assert_eq!(switch.len(), 16);
    assert_eq!(statics.len(), 3);
    assert_eq!(sticks.len(), 15);
    assert_eq!(faces_n, 6);
    assert_eq!(pads_n, 17);

    let pad_base = 0xF700u16;
    let face_codes = 32u16;
    let static_at = 24u16;
    let key_base = number_after(&keys, "pub const KEY_BASE: u16 = ") as u16;
    let stick_keys = number_after(&keys, "pub const STICK_KEYS: u16 = ") as u16;
    let stick_game = number_after(&keys, "pub const STICK_GAME: u16 = ") as u16;
    let mut mapping: Vec<(u16, u16)> = Vec::new();
    let mut single: Vec<u16> = Vec::new();
    for (b, code) in switch.iter().enumerate() {
        mapping.push((*code, 100 + b as u16));
        single.push(*code);
    }
    for (k, code) in statics.iter().enumerate() {
        mapping.push((*code, 150 + k as u16));
        single.push(*code);
    }
    for f in 0..faces_n {
        let base = pad_base + face_codes * f;
        for i in 0..pads_n {
            mapping.push((base + i, 200 + 100 * f + i));
        }
        for k in 0..3 {
            mapping.push((base + static_at + k, 290 + 100 * f + k));
        }
    }
    for key in [0x45u16, 0x01] {
        mapping.push((key_base + key, 900 + key));
    }
    for (i, code) in sticks.iter().enumerate() {
        mapping.push((*code, 1400 + i as u16));
        single.push(*code);
        mapping.push((stick_keys + i as u16, 1200 + i as u16));
        mapping.push((stick_game + i as u16, 1300 + i as u16));
    }
    let cmap = cmap4(&mapping, &single);

    let harness = format!(
        r#"#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "gpu/vk/vk_api.h"
{defines}
static int pad_bind[HOS_SW_N], key_bind[HOS_SW_N][2];
static int hos_binding(int set, int button, int slot) {{
    return set == HOS_BIND_KEY ? key_bind[button][slot] : pad_bind[button];
}}
{first_key}
{switch_code}
{switch_pad}
{static_code}
{stick_code}
{rd16}
{wr16}
{cmap4_glyph}
{cmap4_point}
{cmap4_bind}
#define CHECK(x) do {{ if (!(x)) {{ fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #x); return 1; }} }} while (0)
int main(int argc, char **argv) {{
    CHECK(argc == 2);
    FILE *f = fopen(argv[1], "rb"); CHECK(f);
    static unsigned char st[65536];
    const size_t n = fread(st, 1, sizeof st, f); fclose(f);
    for (int b = 0; b < HOS_SW_N; b++) pad_bind[b] = -1;
    pad_bind[HOS_SW_A] = HOS_PAD_EAST;
    pad_bind[HOS_SW_B] = HOS_PAD_RSHOULDER;
    pad_bind[HOS_SW_Y] = HOS_PAD_RPADDLE1;   /* (a paddle has no artwork: its default's icon) */
    key_bind[HOS_SW_A][0] = 0x45;
    key_bind[HOS_SW_B][1] = 0x01;
    key_bind[HOS_SW_X][0] = 0x07;   /* (a key the font has no icon for) */
    for (int face = 0; face < HOS_PROMPT_N; face++) {{
        const int done = cmap4_bind(st, n, face);
        if (done != 2) {{ fprintf(stderr, "face %d done %d\n", face, done); }}
        CHECK(done == 2);
        for (int b = 0; b < HOS_SW_N; b++) {{
            unsigned want;
            const int key = first_key(b);
            if (face == HOS_PROMPT_KEYBOARD && (b == HOS_SW_A || b == HOS_SW_B))
                want = 900 + key;
            else {{
                const int pad = pad_bind[b];
                const int art = pad >= 0 && pad < HOS_PAD_ART_N;
                want = 200 + 100 * face + (art ? pad : switch_pad[b]);
            }}
            CHECK(cmap4_glyph(st, n, switch_code[b]) == want);
        }}
        for (int k = 0; k < 3; k++)
            CHECK(cmap4_glyph(st, n, static_code[k]) == (unsigned)(290 + 100 * face + k));
        for (int i = 0; i < 15; i++)
            CHECK(cmap4_glyph(st, n, stick_code[i]) == (unsigned)((face == HOS_PROMPT_KEYBOARD ? 1200 : 1300) + i));
        for (int off = 0; off < FACE_CODES; off++) {{
            const unsigned want = off < 17 ? 200 + 100 * face + off
                                 : off >= STATIC_AT && off < STATIC_AT + 3 ? 290 + 100 * face + off - STATIC_AT
                                                                           : 0;
            const unsigned got = cmap4_glyph(st, n, PAD_BASE + FACE_CODES * face + off);
            if (got != want) {{ fprintf(stderr, "face %d off %d got %u want %u\n", face, off, got, want); }}
            CHECK(got == want);
        }}
    }}
    puts("ok");
    return 0;
}}
"#,
        defines = [
            "PAD_BASE", "FACE_CODES", "STATIC_AT", "KEY_BASE", "STICK_KEYS", "STICK_GAME"
        ]
        .iter()
        .map(|name| define(&prompts, name))
        .collect::<Vec<_>>()
        .join("\n"),
        first_key = block(&prompts, "static int first_key("),
        switch_code = format!(
            "static const uint16_t switch_code[HOS_SW_N] = {{{}}};",
            initializer(&prompts, "switch_code[HOS_SW_N]")
        ),
        switch_pad = format!(
            "static const unsigned char switch_pad[HOS_SW_N] = {{{}}};",
            initializer(&prompts, "switch_pad[HOS_SW_N]")
        ),
        static_code = format!(
            "static const uint16_t static_code[3] = {{{}}};",
            initializer(&prompts, "static_code[3]")
        ),
        stick_code = format!(
            "static const uint16_t stick_code[15] = {{{}}};",
            initializer(&prompts, "stick_code[15]")
        ),
        rd16 = block(&prompts, "static unsigned rd16("),
        wr16 = block(&prompts, "static void wr16("),
        cmap4_glyph = block(&prompts, "static unsigned cmap4_glyph("),
        cmap4_point = block(&prompts, "static int cmap4_point("),
        cmap4_bind = block(&prompts, "static int cmap4_bind("),
    );

    let Some(cc) = compiler() else {
        eprintln!("skipping the prompt binding fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = scratch("prompt-bind");
    let fixture = dir.join("bind.c");
    let cmap_path = dir.join("cmap.bin");
    fs::write(&fixture, harness).expect("write the binding fixture");
    // (the cmap table's header and its one record, then the format 4 subtable cmap4_bind takes)
    fs::write(&cmap_path, &cmap[20..]).expect("write the cmap");
    let exe = dir.join(if cfg!(windows) { "bind.exe" } else { "bind" });
    let build = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", root().display()))
        .arg(&fixture)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe)
        .arg(&cmap_path)
        .output()
        .expect("run the binding fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}
