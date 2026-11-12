// The ocarina's five note controls (upstream NotRexed/Hookshot 2708dc61, #1930), game-free: the menu's rows and the
// ini lines are source-checked, and host/nn_hid.c's binding and note layers are cut out, compiled with a stub host and
// run against a fake keyboard and pad. A machine with no C compiler skips the compiled part.

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

/// A function's definition from a C source, braces matched.
fn function(source: &str, marker: &str) -> String {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + source[start..].find('{').expect("opening brace") + 1;
    let bytes = source.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    source[start..end].to_string()
}

/// From the line that holds `first` to the start of the block or statement that starts at `last`.
fn between(text: &str, first: &str, last: &str) -> String {
    let start = text[..text
        .find(first)
        .unwrap_or_else(|| panic!("missing {first}"))]
        .rfind('\n')
        .map(|at| at + 1)
        .unwrap_or(0);
    let end = text.find(last).unwrap_or_else(|| panic!("missing {last}"));
    text[start..end].to_string()
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

const PRELUDE: &str = r#"
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include "vk_api.h"
typedef long LONG;
typedef unsigned long DWORD;
#define VK_LBUTTON 0x01
#define VK_RBUTTON 0x02
#define VK_MBUTTON 0x04
#define VK_XBUTTON1 0x05
#define VK_XBUTTON2 0x06
#define VK_TAB 0x09
#define VK_RETURN 0x0D
#define VK_SHIFT 0x10
#define VK_CONTROL 0x11
#define VK_MENU 0x12
#define VK_SPACE 0x20
#define VK_LEFT 0x25
#define VK_UP 0x26
#define VK_RIGHT 0x27
#define VK_DOWN 0x28
#define VK_NUMPAD0 0x60
#define VK_NUMPAD2 0x62
#define VK_NUMPAD4 0x64
#define VK_NUMPAD6 0x66
#define VK_NUMPAD8 0x68
#define MAX_PATH 260
static LONG InterlockedExchange(volatile LONG *p, LONG v) { LONG o = *p; *p = v; return o; }
static LONG InterlockedCompareExchange(volatile LONG *p, LONG v, LONG c) { LONG o = *p; if (o == c) *p = v; return o; }
static void Sleep(int ms) { (void)ms; }
static void hos_log(const char *f, ...) { (void)f; }
static const char *ini_dir;
#ifdef _WIN32
#define EXE_SEP "\\"
#else
#define EXE_SEP "/"
#endif
static DWORD GetModuleFileNameA(void *m, char *b, DWORD n) { (void)m; return (DWORD)snprintf(b, n, "%s" EXE_SEP "hookshot.exe", ini_dir); }
#define hos_game_path_a(b,n) GetModuleFileNameA(0,b,n)
static int held[256];
static int key(int vk) { return held[vk & 255]; }
static int oc_menu, oc_front = 1;   /* (oc_pad_word shares read_input's gates: the Insert menu, the pads held after it, the window) */
int rx_option(int id, int otherwise) { return id == 1 ? oc_menu : otherwise; }
static int game_in_front(void) { return oc_front; }
static int64_t oc_now;
static int64_t hos_now_ns(void) { return oc_now; }
"#;

const MAIN: &str = r#"
static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL line %d: %s\n", __LINE__, #c); fails++; } } while (0)
static void release(void) { memset(held, 0, sizeof held); }
/* what read_input()'s Switch-key loop does with bound_down, and pads_read's with the pad table */
static uint64_t switch_bits(void) {
    uint64_t b = 0;
    for (int i = 0; i < HOS_SW_N; i++) if (bound_down(i, 0)) b |= 1ULL << i;
    return b;
}
static uint64_t pad_bits(uint64_t raw) {
    uint64_t b = 0;
    for (int i = 0; i < HOS_SW_N; i++) {
        const int a = pad_bind[i];
        if (a >= 0 && (raw >> a & 1) && !(oc_consumed_pads >> a & 1)) b |= 1ULL << i;
    }
    return b;
}
int main(int argc, char **argv) {
    (void)argc;
    ini_dir = argv[1];
    binds_load();
    /* (a) the ini: the note rows start unset, except the lines this file has (note_y's key, note_l's pad) */
    CHECK(hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_L, 0) == 0
          && hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_R, 0) == 0
          && hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 0) == 0
          && hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_X, 0) == 0);
    CHECK(hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_Y, 0) == 78);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_L, 0) == HOS_PAD_LSHOULDER);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_R, 0) == -1
          && hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_A, 0) == -1
          && hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_Y, 0) == -1
          && hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_X, 0) == -1);
    CHECK(hos_binding(HOS_BIND_KEY, HOS_KEY_MOVE_UP, 0) == 'Z');
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_A, 0) == HOS_PAD_GUIDE);
    /* (b) sharing: a normal row taking a note row's key leaves the note row holding it too... */
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 0, VK_NUMPAD4);
    hos_bind(HOS_BIND_KEY, HOS_SW_DOWN, 0, VK_NUMPAD4);
    CHECK(hos_binding(HOS_BIND_KEY, HOS_SW_DOWN, 0) == VK_NUMPAD4
          && hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 0) == VK_NUMPAD4);
    /* ...and a note row's own rebinding takes its key from nobody */
    hos_bind(HOS_BIND_KEY, HOS_SW_B, 0, 'V');
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 0, 'V');
    CHECK(hos_binding(HOS_BIND_KEY, HOS_SW_B, 0) == 'V' && hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 0) == 'V');
    /* a normal row still takes its key from the other normal rows (not from note rows) */
    hos_bind(HOS_BIND_KEY, HOS_SW_A, 0, 'E');
    CHECK(hos_binding(HOS_BIND_KEY, HOS_SW_B, 0) == 'V');
    /* (c) pad inputs: a normal row's bind swaps among the Switch rows and leaves the note rows alone... */
    hos_bind(HOS_BIND_PAD, HOS_KEY_OC_NOTE_R, 0, HOS_PAD_RSHOULDER);
    hos_bind(HOS_BIND_PAD, HOS_SW_B, 0, HOS_PAD_RSHOULDER);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 0) == HOS_PAD_RSHOULDER
          && hos_binding(HOS_BIND_PAD, HOS_SW_R, 0) == HOS_PAD_SOUTH
          && hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_R, 0) == HOS_PAD_RSHOULDER);
    /* ...and a note row can hold the same pad input as a Switch row, with no swap */
    hos_bind(HOS_BIND_PAD, HOS_KEY_OC_NOTE_R, 0, HOS_PAD_EAST);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_R, 0) == HOS_PAD_EAST
          && hos_binding(HOS_BIND_PAD, HOS_SW_A, 0) == HOS_PAD_GUIDE);
    /* the MOVE/CAM rows take no gamepad input */
    hos_bind(HOS_BIND_PAD, HOS_KEY_CAM_UP, 0, HOS_PAD_WEST);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_KEY_CAM_UP, 0) == -1);
    /* (d) a mouse button is never a note: a note row does not accept one */
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 1, VK_LBUTTON);
    CHECK(hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 1) == 0);
    hos_bind(HOS_BIND_KEY, HOS_SW_Y, 0, VK_LBUTTON);
    CHECK(hos_binding(HOS_BIND_KEY, HOS_SW_Y, 0) == VK_LBUTTON);
    /* Reset to defaults: the note rows come back unset */
    hos_bind(HOS_BIND_KEY, -1, 0, 0);
    CHECK(hos_binding(HOS_BIND_KEY, HOS_KEY_OC_NOTE_L, 0) == 0 && hos_binding(HOS_BIND_PAD, HOS_KEY_OC_NOTE_A, 0) == -1);

    /* (e) unset rows the player never bound change nothing: the game's own note buttons play on */
    hos_oc_wait = 1, hos_oc_beat++;
    release(), held['Q'] = held['E'] = 1;
    oc_notes();
    CHECK(hos_oc_note_bits == 0);
    const uint64_t own = switch_bits();
    CHECK((own & (1ULL << HOS_SW_L)) && (own & (1ULL << HOS_SW_A)));
    CHECK(oc_pad_word(own) == own);
    hos_oc_wait = 0;

    /* the rest of this test is a player who bound the rows: only then does the layer take the buttons over */
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_L, 0, VK_NUMPAD0);
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_R, 0, VK_NUMPAD2);
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_A, 0, VK_NUMPAD4);
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_Y, 0, VK_NUMPAD6);
    hos_bind(HOS_BIND_KEY, HOS_KEY_OC_NOTE_X, 0, VK_NUMPAD8);
    hos_bind(HOS_BIND_PAD, HOS_KEY_OC_NOTE_L, 0, HOS_PAD_LSHOULDER);
    /* (f) NumPad 4 is the (A) note and also bound to D-pad down: during the wait it is only the note */
    hos_bind(HOS_BIND_KEY, HOS_SW_DOWN, 0, VK_NUMPAD4);
    hos_oc_wait = 1, hos_oc_beat++;
    release(), held[VK_NUMPAD4] = held['2'] = 1;
    oc_notes();
    CHECK((hos_oc_note_bits & (1ULL << 2)) != 0);
    CHECK(!(switch_bits() & (1ULL << HOS_SW_DOWN)));
    CHECK((switch_bits() & (1ULL << HOS_SW_UP)) != 0);
    /* (g) a note key shared with a Move/Cam row: its stick contribution reads nothing during the wait */
    hos_bind(HOS_BIND_KEY, HOS_KEY_MOVE_LEFT, 0, VK_NUMPAD2);
    hos_oc_wait = 1, hos_oc_beat++;
    release(), held[VK_NUMPAD2] = 1;
    oc_notes();
    CHECK((hos_oc_note_bits & (1ULL << 1)) != 0);
    CHECK(!bound_down(HOS_KEY_MOVE_LEFT, 0));
    /* (h) the (L) note's pad input is the l row's (both HOS_PAD_LSHOULDER): during the wait the pad gives the
       note, not L, while l's own key 'Q' still gives L */
    hos_oc_wait = 1, hos_oc_beat++;
    release();
    pad_raw_now = 1 << HOS_PAD_LSHOULDER;
    oc_notes();
    CHECK((hos_oc_note_bits & 1ULL) != 0);
    CHECK((pad_bits(1ULL << HOS_PAD_LSHOULDER) & (1ULL << HOS_SW_L)) == 0);
    held['Q'] = 1;
    oc_notes();
    CHECK((switch_bits() & (1ULL << HOS_SW_L)) != 0);
    pad_raw_now = 0;
    /* (i) wait off: the note bits are clear and nothing is consumed */
    hos_oc_wait = 0;
    release(), held[VK_NUMPAD4] = 1;
    oc_notes();
    CHECK(hos_oc_note_bits == 0);
    CHECK((switch_bits() & (1ULL << HOS_SW_DOWN)) != 0);
    /* (j) the microphone wait: notes clear, nothing consumed, humming is the game's own path */
    hos_oc_wait = 2;
    oc_notes();
    CHECK(hos_oc_note_bits == 0);
    CHECK((switch_bits() & (1ULL << HOS_SW_DOWN)) != 0);
    /* (k) the word the game polls: during the button wait the five note buttons are the controls and nothing else */
    hos_oc_wait = 1, hos_oc_beat++;
    release();
    held[VK_NUMPAD2] = held['Q'] = held['E'] = 1;
    oc_notes();
    const uint64_t raww = switch_bits();
    CHECK((raww & (1ULL << HOS_SW_L)) && (raww & (1ULL << HOS_SW_A)));
    const uint64_t w = oc_pad_word(raww);
    CHECK((w & (1ULL << HOS_SW_R)) && !(w & (1ULL << HOS_SW_L)) && !(w & (1ULL << HOS_SW_A)));
    CHECK((w & (1ULL << HOS_SW_X)) == 0 && (w & (1ULL << HOS_SW_Y)) == 0);
    /* (l) mic wait: the word is the player's own, untouched */
    hos_oc_wait = 2;
    oc_notes();
    CHECK(oc_pad_word(raww) == raww);
    /* (m) the lesson kind (2) teaches the game's own note buttons: they stay, the remaps go over them */
    hos_oc_wait = 1, hos_oc_beat++;
    oc_notes();
    hos_oc_last_kind = 2;
    CHECK((oc_pad_word(raww) & (1ULL << HOS_SW_R)) != 0 && (oc_pad_word(raww) & (1ULL << HOS_SW_L)) != 0);
    hos_oc_last_kind = 1;
    CHECK((oc_pad_word(raww) & (1ULL << HOS_SW_L)) == 0);
    /* (n) the note window shares read_input's own gates: Insert menu open, pads held after it closed, or the game
       behind another window, and the word the game reads is the player's own */
    release(), held[VK_NUMPAD2] = held['Q'] = held['E'] = 1;
    hos_oc_wait = 1, hos_oc_beat++;
    oc_notes();
    const uint64_t raw2 = switch_bits();
    uint64_t w2 = oc_pad_word(raw2);
    CHECK((w2 & (1ULL << HOS_SW_R)) && !(w2 & (1ULL << HOS_SW_L)));
    oc_menu = 1;
    CHECK(oc_pad_word(raw2) == raw2);
    oc_menu = 0;
    pad_after_menu = 1;
    CHECK(oc_pad_word(raw2) == raw2);
    pad_after_menu = 0;
    oc_front = 0;
    CHECK(oc_pad_word(raw2) == raw2);
    oc_front = 1;
    CHECK((oc_pad_word(raw2) & (1ULL << HOS_SW_R)) && !(oc_pad_word(raw2) & (1ULL << HOS_SW_L)));
    /* (o) staleness: the wait only lives while the game's note action stamps its beat */
    hos_oc_beat++;
    oc_now = 1000000000LL;
    oc_notes();
    CHECK(hos_oc_wait == 1);
    CHECK((oc_pad_word(raw2) & (1ULL << HOS_SW_R)) && !(oc_pad_word(raw2) & (1ULL << HOS_SW_L)));
    oc_now = 1600000000LL;
    oc_notes();
    CHECK(hos_oc_wait == 0);
    CHECK(oc_pad_word(raw2) == raw2);
    CHECK((switch_bits() & (1ULL << HOS_SW_L)) && (switch_bits() & (1ULL << HOS_SW_A)));
    hos_oc_wait = 1, hos_oc_beat++;
    oc_notes();
    CHECK((oc_pad_word(raw2) & (1ULL << HOS_SW_R)) && !(oc_pad_word(raw2) & (1ULL << HOS_SW_L)));
    if (!fails) printf("PASS\n");
    return fails != 0;
}
"#;

#[test]
fn the_controls_page_and_the_api_carry_the_five_note_rows() {
    let api = source("gpu/vk/vk_api.h");
    let menu = source("gpu/vk/vk_menu.cpp");
    let host = source("host/nn_hid.c");
    let docs = source("docs/development.md");
    let readme = source("readme.md");

    assert!(api.contains("#define MKWVK_API_VERSION 19"));
    assert!(api.contains("HOS_KEY_OC_NOTE_L, HOS_KEY_OC_NOTE_R"));
    assert!(api.contains("HOS_KEY_OC_NOTE_A, HOS_KEY_OC_NOTE_Y, HOS_KEY_OC_NOTE_X, HOS_KEY_N"));
    assert!(api.contains("version 19: HOS_BIND_PAD takes the ocarina's note rows"));

    assert!(menu.contains("\"note_l\", \"note_r\", \"note_a\", \"note_y\", \"note_x\""));
    for label in [
        "Ocarina note (L)",
        "Ocarina note (R)",
        "Ocarina note (A)",
        "Ocarina note (Y)",
        "Ocarina note (X)",
    ] {
        assert!(
            menu.contains(&format!("\"{label}\"")),
            "no Controls row for {label}"
        );
    }
    assert!(
        menu.contains(
            "controls[i].button >= HOS_KEY_MOVE_UP && controls[i].button <= HOS_KEY_CAM_RIGHT"
        ),
        "keys_only does not leave the note rows a gamepad column"
    );
    assert!(
        menu.contains("i >= HOS_KEY_OC_NOTE_L && i <= HOS_KEY_OC_NOTE_X"),
        "the ini save writes no pad_ line for a note row"
    );
    assert!(
        menu.contains("Ocarina note keys are only notes while"),
        "the Controls hint does not explain the note keys"
    );

    assert!(host.contains("\"note_l\", \"note_r\", \"note_a\", \"note_y\", \"note_x\""));
    assert!(host.contains("static int is_note_row(int i)"));
    assert!(host.contains("static int has_pad(int i)"));
    assert!(host.contains("static void oc_notes(void)"));
    assert!(host.contains("static uint64_t oc_pad_word(uint64_t buttons)"));
    assert!(host.contains("G64(e + 8) = oc_pad_word(buttons);"));
    assert!(function(&host, "static void read_input(").contains("oc_notes();"));
    assert!(
        between(
            &host,
            "static void pads_read(",
            "static int pads_in_background("
        )
        .contains("oc_consumed_pads"),
        "the pad read does not consult the consumed pad mask"
    );

    assert!(docs.contains("`note_l`, `note_r`, `note_a`, `note_y`"));
    assert!(readme.contains("`Ocarina note (L)`"));
    assert!(readme.contains("unset by default"));
}

#[test]
fn the_note_layer_consumes_only_what_the_player_bound() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the ocarina note fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let host = source("host/nn_hid.c");
    let code = [
        function(&host, "static int kmap_down("),
        between(
            &host,
            "static const char *const bind_names[HOS_KEY_N]",
            "uint32_t hos_pad_raw(",
        ),
        between(&host, "static int mouse_vk_slot(", "static int bound_down("),
        function(&host, "static int bound_down("),
        function(&host, "static void oc_notes("),
        function(&host, "static uint64_t oc_pad_word("),
    ]
    .join("\n");

    let dir = std::env::temp_dir().join(format!(
        "hookshot-ocarina-notes-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    fs::write(
        dir.join("hookshot_options.ini"),
        "key_move_up=90,0\npad_a=5\nkey_note_y=78,0\npad_note_l=9\n",
    )
    .expect("ini");
    fs::write(dir.join("t.c"), format!("{PRELUDE}\n{code}\n{MAIN}")).expect("fixture");
    let binary = dir.join(if cfg!(windows) { "t.exe" } else { "t" });
    let build = Command::new(&cc)
        .args(["-std=gnu11", "-Wall", "-Wno-unused-function"])
        .arg("-I")
        .arg(repo.join("gpu/vk"))
        .arg("-I")
        .arg(repo.join("gpu"))
        .arg(dir.join("t.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&binary)
        .arg(&dir)
        .output()
        .expect("run the fixture");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "PASS");
}
