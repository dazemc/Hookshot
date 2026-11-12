// The host's input routing, game-free: tests/input_background_test.c with the production parser, LIVE reader, routing and
// Npad writer cut from host/nn_hid.c (and host/mkw_glue.c), against fake clocks, windows, keyboard, mouse and pads. The
// fixture predates the second gamepad binding and the ocarina's five note controls; it is run again after both merges, so
// the note layer's consumed keys and pads and the alternate gamepad slot are compiled with the rest. No game, dump or GPU
// is read, and no real device or window is opened.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
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

fn define(text: &str, name: &str) -> String {
    text.lines()
        .find(|line| line.starts_with(&format!("#define {name} ")))
        .unwrap_or_else(|| panic!("missing #define {name}"))
        .to_owned()
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

const CASES: &[&str] = &[
    "foreground",
    "script-background",
    "headless",
    "noactivate",
    "live-background",
    "no-script-background",
    "exclusive-foreground",
    "exclusive-background",
    "pads-env",
    "pads-menu",
    "pads-off",
    "frame-hold",
    "live-frame",
    "reuse-frame",
    "wall",
    "zero",
    "empty",
];

#[test]
fn scripted_and_background_input_still_reach_the_game() {
    let repo = root();
    let hid = read("host/nn_hid.c");
    let api = read("gpu/vk/vk_api.h");
    let glue = read("host/mkw_glue.c");

    // The constants the cut code shares with the game's headers.
    let mut constants = String::new();
    for (text, marker) in [
        (&hid, "enum { B_A"),
        (&api, "enum { HOS_SW_A"),
        (&api, "enum { HOS_KEY_MOVE_UP"),
        (&api, "enum { HOS_OPT_CHEAT_HEARTS"),
        (&api, "enum { HOS_BIND_PAD"),
    ] {
        constants.push_str(&block(text, marker));
        constants.push_str(";\n");
    }
    for name in ["STATE_SIZE", "NPAD_NO1", "ATTR_CONNECTED"] {
        constants.push_str(&define(&hid, name));
        constants.push('\n');
    }

    // The production path: the parser and LIVE reader, the routing, the binding table and the Npad writer. The spies in the
    // fixture stand in for the clock, the window, the keyboard, the mouse and the pads; the cheat and host-menu hooks are
    // no-ops here.
    let start = hid
        .find("typedef struct { int64_t start_ms, end_ms;")
        .expect("the Press struct");
    let scripted = block(&hid, "static void scripted_input(");
    let end = hid.find(&scripted).expect("scripted_input") + scripted.len();
    let cheat_state = "static uint64_t flight_input;\nstatic uint64_t free_camera_axes;\nstatic int free_camera_height;\nstatic int move_stick_held;";
    assert!(
        hid.contains(cheat_state),
        "the cheat state the fixture declares moved"
    );
    let cheat_spies = r#"
volatile int hos_oot_live_action;
int hos_oot_flight_shortcut_ready(void) { CHECK(!"unexpected cheat readiness query"); return 0; }
void rx_option_set(int id, int value) { (void)id; (void)value; CHECK(!"unexpected cheat option change"); }
"#;
    let host_spies = r#"
static void host_taps(void) {}
static void host_stick_read(int32_t stick[4]) { (void)stick; }
static void host_drag_read(int32_t stick[4]) { (void)stick; }
static void player_used(void) {}
"#;
    let mut actual = String::new();
    actual.push_str(cheat_state);
    actual.push_str(cheat_spies);
    actual.push_str(host_spies);
    actual.push_str(&hid[start..end]);
    actual.push('\n');
    for (text, marker) in [
        (&hid, "static int key("),
        (&hid, "static int kmap_down("),
        (&hid, "static int game_in_front("),
        (&hid, "static void stick_max("),
        (&hid, "static int pads_in_background("),
        (&hid, "static int mouse_vk_slot("),
    ] {
        actual.push_str(&block(text, marker));
        actual.push('\n');
        if marker == "static int mouse_vk_slot(" {
            actual.push_str(
                hid.lines()
                    .find(|line| line.starts_with("static int mouse_armed[5];"))
                    .expect("the mouse's armed buttons"),
            );
            actual.push('\n');
        }
    }
    for marker in [
        "static void mouse_buttons_arm(",
        "static int bound_down(",
        "static void oc_notes(",
        "static uint64_t oc_pad_word(",
        "static int flight_jump_shortcut(",
        "static void read_input(",
        "static void npad_states(",
    ] {
        actual.push_str(&block(&hid, marker));
        actual.push('\n');
    }
    for name in ["hos_title_input", "hos_frame_step_buttons"] {
        actual.push_str(&block(&glue, &format!("uint64_t {name}(")));
        actual.push('\n');
    }

    let Some(cc) = compiler() else {
        eprintln!("skipping the input routing fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = scratch("input-background");
    fs::write(dir.join("input_constants.inc"), constants).expect("write the constants");
    fs::write(dir.join("input_actual.inc"), actual).expect("write the production path");
    let exe = dir.join(if cfg!(windows) { "fixture.exe" } else { "fixture" });
    let build = Command::new(&cc)
        .args([
            "-std=c11",
            "-include",
            repo.join("gpu/vk/compat/hookshot_env.h").to_str().unwrap(),
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Wno-missing-field-initializers",
            "-D_CRT_SECURE_NO_WARNINGS",
        ])
        .arg(format!("-I{}", dir.display()))
        .arg(repo.join("tests/input_background_test.c"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    for case in CASES {
        let mut run = Command::new(&exe);
        for name in [
            "HOS_INPUT_SCRIPT",
            "HOS_INPUT_LIVE",
            "HOS_INPUT_FRAME_CLOCK",
            "HOS_INPUT_EXCLUSIVE",
            "HOS_INPUT_BACKGROUND",
            "HOS_INPUT_SCRIPT_BACKGROUND",
            "HOS_HEADLESS",
            "HOS_WINDOW_NOACTIVATE",
            "HOS_MOUSE_NATIVE",
        ] {
            run.env_remove(name);
        }
        if !matches!(
            *case,
            "live-background" | "no-script-background" | "live-frame" | "reuse-frame"
        ) {
            run.env("HOS_INPUT_SCRIPT", "0:A+LUP/200");
        }
        match *case {
            "frame-hold" | "live-frame" | "reuse-frame" => {
                run.env("HOS_INPUT_FRAME_CLOCK", "1");
            }
            "zero" => {
                run.env("HOS_INPUT_FRAME_CLOCK", "0");
            }
            "empty" => {
                run.env("HOS_INPUT_FRAME_CLOCK", "");
            }
            _ => {}
        }
        if case.starts_with("exclusive-") || matches!(*case, "frame-hold" | "wall" | "zero" | "empty") {
            run.env("HOS_INPUT_EXCLUSIVE", "1");
        }
        match *case {
            "pads-env" => {
                run.env("HOS_INPUT_BACKGROUND", "1");
            }
            "pads-off" => {
                run.env("HOS_INPUT_BACKGROUND", "0");
            }
            "headless" => {
                run.env("HOS_HEADLESS", "1");
            }
            "noactivate" => {
                run.env("HOS_WINDOW_NOACTIVATE", "1");
            }
            _ => {}
        }
        if matches!(*case, "live-background" | "live-frame") {
            let live = dir.join(format!("{case}.txt"));
            fs::write(&live, b"").expect("write the live input file");
            run.env("HOS_INPUT_LIVE", &live);
        }
        let output = run.arg(case).output().expect("run the input fixture");
        assert!(
            output.status.success(),
            "{case}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("PASS:"),
            "{case}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    let _ = fs::remove_dir_all(dir);
}
