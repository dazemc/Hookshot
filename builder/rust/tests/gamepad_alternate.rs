// A second gamepad binding for every Switch button (upstream NotRexed/Hookshot bf04924e, #1990): the Controls page's
// Input 1 and Input 2, the paddle inputs, and the options file's pad_<button>=<position>[,<position>]. Game-free: the
// sources are read for the wiring, host/nn_hid.c's binding table is compiled against a fake options file, and the
// Controls page's hit-test is compiled and run.

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

/// A C/C++ definition's braces, matched, from its first line.
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

/// From the start of the line that holds `first` to the definition at `last`.
fn between(text: &str, first: &str, last: &str) -> String {
    let at = text
        .find(first)
        .unwrap_or_else(|| panic!("missing {first}"));
    let start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text.find(last).unwrap_or_else(|| panic!("missing {last}"));
    format!("{}{}", &text[start..end], block(text, last))
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

fn cxx() -> Option<String> {
    let candidates =
        std::env::var("CXX")
            .ok()
            .into_iter()
            .chain(["c++".into(), "g++".into(), "clang++".into()]);
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

#[test]
fn a_second_gamepad_binding_is_wired_through_the_api_and_the_menu() {
    let api = source("gpu/vk/vk_api.h");
    let hit = source("gpu/vk/vk_menu_hit.h");
    let menu = source("gpu/vk/vk_menu.cpp");
    let hid = source("host/nn_hid.c");
    let pad = source("host/pad_sdl.c");
    let prompts = source("host/prompts.c");
    let assets = source("nx2/prompt_assets.py");
    let legacy = source("nx2/build.py");

    // The renderer ABI changed: slot 1 of HOS_BIND_PAD is a binding now, and the paddles have positions.
    assert!(
        api.contains("#define MKWVK_API_VERSION 20"),
        "the second gamepad binding changes MkwVkHost::binding and must bump the API version (20: the display modes' screen_size)"
    );
    assert!(api.contains("HOS_PAD_RTRIGGER, HOS_PAD_RPADDLE1, HOS_PAD_LPADDLE1,"));
    assert!(api.contains("HOS_PAD_RPADDLE2, HOS_PAD_LPADDLE2, HOS_PAD_N"));
    assert!(api.contains("enum { HOS_PAD_ART_N = HOS_PAD_RTRIGGER + 1 };"));
    assert!(
        api.contains("slots 0 and 1"),
        "the binding contract names both slots"
    );
    assert!(
        hit.contains("enum CtlColumn { kCtlPrimary, kCtlAlternate, kCtlKeyboard, kCtlColumns };")
    );

    // The Controls page draws and edits three columns; Input 2 is a gamepad cell, not a key cell.
    let draw = block(&menu, "void draw_controls(ui::Ctx& c)");
    assert!(
        draw.contains(
            "const char* titles[kCtlColumns] = {\"Input 1\", \"Input 2\", \"Keyboard\"};"
        )
    );
    assert!(draw.contains("col_x[kCtlAlternate]"));
    assert!(draw.contains("disabled = keys && !keyboard"));
    assert!(draw.contains("host->binding(HOS_BIND_PAD, ctl.button, col)"));
    assert!(draw.contains("hits.add(a, y0, b, y1, menu_hit::kCtlCell, i, 0, 0, 0, 0, col)"));
    assert!(menu.contains("int csel = 0, ccol = menu_hit::kCtlPrimary;"));
    let tick = block(&menu, "void menu_tick()");
    assert!(tick.contains("host->bind(HOS_BIND_PAD, controls[csel - kTop].button, ccol, bit);"));
    assert!(tick.contains("ccol = (ccol + kCtlColumns + (press[3] ? -1 : 1)) % kCtlColumns;"));
    assert!(tick.contains("const int col = keys_only(i) ? kCtlKeyboard : ccol;"));
    assert!(tick.contains("host->bind(HOS_BIND_PAD, controls[i].button, col, -1);"));

    // The options file keeps the alternate only when there is one, and the loader reads either slot.
    let save = block(&menu, "void save()");
    assert!(save.contains("const int alternate = host->binding(HOS_BIND_PAD, i, 1);"));
    assert!(
        save.contains(
            "fprintf(f, \"pad_%s=%d\", bind_names[i], host->binding(HOS_BIND_PAD, i, 0));"
        )
    );
    assert!(save.contains("if (alternate >= 0) fprintf(f, \",%d\", alternate);"));
    assert!(hid.contains("static int pad_bind[HOS_KEY_N][2];"));
    assert!(hid.contains("static int pad_code(const char *text)"));
    assert!(hid.contains("pad_bind[i][1] = comma ? pad_code(comma + 1) : -1;"));
    assert!(hid.contains("static uint64_t bound_pad_buttons(uint64_t raw)"));
    assert!(hid.contains("const uint64_t b = bound_pad_buttons(raw & ~oc_consumed_pads);"));

    // SDL's four paddles are bindable, and the prompts stop at the artwork the font has.
    assert!(pad.contains("BTN_RPADDLE1, BTN_LPADDLE1, BTN_RPADDLE2, BTN_LPADDLE2"));
    assert!(pad.contains("HOS_PAD_RPADDLE1 + k - BTN_RPADDLE1"));
    assert!(prompts.contains("bound = pad >= 0 && pad < HOS_PAD_ART_N"));
    assert!(prompts.contains("HOS_PROMPT_N * HOS_SW_N * HOS_PAD_ART_N"));
    assert!(prompts.contains("bind[b] >= 0 && bind[b] < HOS_PAD_ART_N"));
    assert!(prompts.contains("* HOS_PAD_ART_N + pad"));
    let start = assets.find("PADS = [").expect("prompt_assets PADS");
    let end = start + assets[start..].find(']').expect("PADS end");
    assert_eq!(
        assets[start..end].matches('"').count() / 2,
        17,
        "nx2/prompt_assets.py PADS is the 17 inputs with artwork"
    );

    // The legacy Python build must rebuild the host when vk_api.h changes on Windows too: it reads HOS_PAD_N.
    assert!(legacy.contains(
        "    host_deps += open(os.path.join(GPU, \"vk\", \"vk_api.h\"), \"rb\").read()   # (the renderer ABI used by host/rx_backend.c)"
    ));
}

const HID_FIXTURE: &str = r#"
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "vk_api.h"

typedef long LONG;
typedef unsigned long DWORD;
typedef int SRWLOCK;
#define SRWLOCK_INIT 0
static void AcquireSRWLockExclusive(SRWLOCK *p) { (void)p; }
static void ReleaseSRWLockExclusive(SRWLOCK *p) { (void)p; }
static void AcquireSRWLockShared(SRWLOCK *p) { (void)p; }
static void ReleaseSRWLockShared(SRWLOCK *p) { (void)p; }
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
static DWORD hos_game_path_a(char *b, DWORD n) { return (DWORD)snprintf(b, n, "%s" EXE_SEP "hookshot.exe", ini_dir); }
/* HOST_BINDINGS */
static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL line %d: %s\n", __LINE__, #c); fails++; } } while (0)
int main(int argc, char **argv) {
    (void)argc;
    ini_dir = argv[1];
    binds_load();
    /* (an old one-value line fills Input 1 only; a saved alternate fills Input 2) */
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_A, 0) == HOS_PAD_EAST);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_A, 1) == HOS_PAD_RPADDLE1);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 0) == HOS_PAD_SOUTH);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 1) == -1);   /* (an old one-value entry) */
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_X, 0) == HOS_PAD_NORTH);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_X, 1) == -1);   /* (an invalid second value) */
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_A, 2) == -1);   /* (no third slot) */
    CHECK(hos_binding(HOS_BIND_PAD, HOS_KEY_MOVE_UP, 0) == -1);

    const uint64_t face = 1ULL << HOS_PAD_EAST, paddle = 1ULL << HOS_PAD_RPADDLE1, a = 1ULL << HOS_SW_A;
    CHECK(bound_pad_buttons(face) == a && bound_pad_buttons(paddle) == a && bound_pad_buttons(face | paddle) == a);
    CHECK(bound_pad_buttons(1ULL << HOS_PAD_RTRIGGER) == 1ULL << HOS_SW_ZR);
    CHECK(bound_pad_buttons(1ULL << HOS_PAD_TOUCHPAD) == 0);

    hos_bind(HOS_BIND_PAD, HOS_SW_B, 1, HOS_PAD_RPADDLE1);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_A, 1) == -1);
    CHECK(bound_pad_buttons(paddle) == 1ULL << HOS_SW_B && bound_pad_buttons(face) == a);
    hos_bind(HOS_BIND_PAD, HOS_SW_B, 1, -1);
    CHECK(bound_pad_buttons(paddle) == 0 && bound_pad_buttons(face) == a);

    hos_bind(HOS_BIND_PAD, HOS_SW_A, 1, HOS_PAD_RPADDLE1);
    /* another button's only input into an empty Input 2: refused, nothing left unbound */
    const int y0 = hos_binding(HOS_BIND_PAD, HOS_SW_Y, 0);
    hos_bind(HOS_BIND_PAD, HOS_SW_B, 1, y0);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 1) == -1 && hos_binding(HOS_BIND_PAD, HOS_SW_Y, 0) == y0);
    /* from a button with an Input 2: taken, and that button's Input 2 moves up */
    hos_bind(HOS_BIND_PAD, HOS_SW_B, 1, HOS_PAD_EAST);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 1) == HOS_PAD_EAST);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_A, 0) == HOS_PAD_RPADDLE1 && hos_binding(HOS_BIND_PAD, HOS_SW_A, 1) == -1);

    /* a code past the positions, or a slot past the second: nothing changes */
    hos_bind(HOS_BIND_PAD, HOS_SW_B, 1, HOS_PAD_N);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 1) == HOS_PAD_EAST);
    hos_bind(HOS_BIND_PAD, HOS_SW_B, 2, HOS_PAD_NORTH);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 1) == HOS_PAD_EAST);

    /* Reset to defaults clears both inputs */
    hos_bind(HOS_BIND_PAD, -1, 0, 0);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_A, 0) == HOS_PAD_EAST && hos_binding(HOS_BIND_PAD, HOS_SW_A, 1) == -1);
    CHECK(hos_binding(HOS_BIND_PAD, HOS_SW_B, 0) == HOS_PAD_SOUTH && hos_binding(HOS_BIND_PAD, HOS_SW_B, 1) == -1);
    CHECK(bound_pad_buttons(face) == a && bound_pad_buttons(paddle) == 0);
    if (!fails) printf("PASS\n");
    return fails != 0;
}
"#;

#[test]
fn the_binding_table_loads_and_keeps_two_gamepad_inputs() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the gamepad binding fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let hid = source("host/nn_hid.c");
    let bindings = format!(
        "{}\n{}",
        between(
            &hid,
            "static const char *const bind_names[HOS_KEY_N]",
            "\nvoid hos_bind("
        ),
        block(&hid, "static uint64_t bound_pad_buttons(")
    );
    let fixture = HID_FIXTURE.replace("/* HOST_BINDINGS */", &bindings);
    let dir = scratch("gamepad-alternate");
    let ini = dir.join("hookshot_options.ini");
    fs::write(
        &ini,
        "key_move_up=90,0\npad_move_up=0\npad_a=1,17\npad_b=0\npad_x=3,99\n",
    )
    .expect("write the options file");
    let source_path = dir.join("t.c");
    fs::write(&source_path, fixture).expect("write the binding fixture");
    let exe = dir.join(if cfg!(windows) { "t.exe" } else { "t" });
    let build = Command::new(&cc)
        .args([
            "-std=gnu11",
            "-Wall",
            "-Wextra",
            "-Wno-unused-function",
            "-Werror",
        ])
        .arg(format!("-I{}", repo.join("gpu/vk").display()))
        .arg(&source_path)
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
        .arg(&dir)
        .output()
        .expect("run the binding fixture");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "PASS");
}

#[test]
fn the_controls_hit_test_finds_the_alternate_gamepad_cell() {
    let Some(cxx) = cxx() else {
        eprintln!("skipping the Controls hit-test: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let repo = root();
    let dir = scratch("gamepad-hit");
    let exe = dir.join(if cfg!(windows) { "hit.exe" } else { "hit" });
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(repo.join("tests/test_menu_hit.cpp"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the hit-test");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).starts_with("PASS:"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}
