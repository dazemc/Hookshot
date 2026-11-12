// Game-free checks of the C host's pure parts, compiled with the host C compiler: the Steam pads' ignore-list filter
// (host/steam_pads.h, upstream NotRexed/Hookshot e4d45157), the gameplay field of view (host/oot_fov.c, upstream
// ba472ea3) and, later here, the other fixtures that need nothing but a compiler. A machine with no C compiler skips
// them, as CI's Rust job may have none on Windows.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// Compile a C fixture with the host compiler, returning its output. `None` when there is no compiler, so the caller
/// can say it skipped.
fn compile_and_run(
    name: &str,
    sources: &[PathBuf],
    includes: &[PathBuf],
    flags: &[&str],
) -> Option<Output> {
    let cc = compiler()?;
    let dir = scratch(name);
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut build = Command::new(&cc);
    build.args(["-std=c11", "-Wall", "-Wextra", "-Werror"]);
    for include in includes {
        build.arg(format!("-I{}", include.display()));
    }
    for source in sources {
        build.arg(source);
    }
    build.args(flags).arg("-o").arg(&exe);
    let built = build.output().expect("run the C compiler");
    assert!(
        built.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&exe).output().expect("run the fixture");
    let _ = fs::remove_dir_all(dir);
    Some(run)
}

#[test]
fn steam_pads_keep_their_motion_sensors_under_steam_input() {
    let repo = root();
    let Some(run) = compile_and_run(
        "steam-pads",
        &[repo.join("tests/steam_pads_test.c")],
        &[repo.join("host")],
        &[],
    ) else {
        eprintln!("skipping the Steam pads fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).starts_with("ok:"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}

#[test]
fn the_window_title_takes_its_optional_override() {
    let repo = root();
    let source = fs::read_to_string(repo.join("host/posix/display_sdl.c")).expect("display_sdl.c");
    let title = function(
        &source,
        "static const char *window_title(const char *fallback)",
    );
    let dir = scratch("window-title");
    let fixture = dir.join("window_title.c");
    fs::write(
        &fixture,
        format!(
            r#"#ifndef _WIN32
#define _GNU_SOURCE   /* (setenv and unsetenv) */
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
{title}
#if defined(_WIN32)
static void put(const char *name, const char *value) {{ _putenv_s(name, value ? value : ""); }}
#else
static void put(const char *name, const char *value) {{ if (value) setenv(name, value, 1); else unsetenv(name); }}
#endif
int main(void) {{
    put("HOS_WINDOW_TITLE", NULL);
    if (strcmp(window_title("Hookshot"), "Hookshot")) return 1;   /* (unset: the fallback) */
    put("HOS_WINDOW_TITLE", "");
    if (strcmp(window_title("Hookshot"), "Hookshot")) return 2;   /* (empty: the fallback too) */
    put("HOS_WINDOW_TITLE", "The Legend of Zelda: Ocarina of Time");
    if (strcmp(window_title("Hookshot"), "The Legend of Zelda: Ocarina of Time")) return 3;
    if (strcmp(window_title("Other"), "The Legend of Zelda: Ocarina of Time")) return 4;
    put("HOS_WINDOW_TITLE", NULL);
    puts("ok");
    return 0;
}}
"#
        ),
    )
    .expect("write the window title fixture");
    let Some(run) = compile_and_run("window-title", &[fixture], &[], &[]) else {
        eprintln!("skipping the window title fixture: no C compiler (CC, cc, gcc or clang)");
        let _ = fs::remove_dir_all(dir);
        return;
    };
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}

#[test]
fn native_mouse_look_holds_the_camera_angles() {
    if cfg!(windows) {
        // (host/posix's windows.h stands in for Windows here; the fixture is built on the POSIX host)
        return;
    }
    let Some(cc) = compiler() else {
        eprintln!("skipping the native mouse fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let dir = scratch("mouse-native");
    // (beside the stand-in hos.h, so mouse_native.c's #include "hos.h" finds that one)
    fs::write(
        dir.join("hos.h"),
        r#"#pragma once
#include <stdint.h>
#include <stddef.h>
extern uint8_t *fake_heap;
#define HOS_HEAP_BASE 0x1000000000ULL
#define GP(a) ((void *)(fake_heap + ((uint64_t)(a) - HOS_HEAP_BASE)))
extern uint64_t hos_heap_size;
void hos_log(const char *fmt, ...);
int64_t hos_now_ns(void);
"#,
    )
    .expect("write the stand-in hos.h");
    fs::copy(repo.join("host/mouse_native.c"), dir.join("mouse_native.c"))
        .expect("copy mouse_native.c");
    fs::copy(repo.join("tests/mouse_native_test.c"), dir.join("t.c")).expect("copy the fixture");
    let exe = dir.join("t");
    let build = Command::new(&cc)
        .args(["-std=gnu11", "-include"])
        .arg(repo.join("gpu/vk/compat/hookshot_env.h"))
        .args(["-Wall", "-O1", "-g"])
        .arg(format!("-I{}", dir.display()))
        .arg(format!("-I{}", repo.join("host/posix").display()))
        .arg(dir.join("t.c"))
        .arg(dir.join("mouse_native.c"))
        .arg("-o")
        .arg(&exe)
        .arg("-lm")
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the mouse fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("PASS"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}

#[test]
fn field_of_view_scales_only_the_gameplay_camera() {
    let repo = root();
    let source = fs::read_to_string(repo.join("host/oot_fov.c")).expect("host/oot_fov.c");
    // (the host's includes need the whole runtime; the body alone takes these stubs, as upstream's test did)
    let body: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("#include"))
        .map(|line| format!("{line}\n"))
        .collect();
    const HEAD: &str = r##"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)
typedef struct { uint64_t x[32]; uint64_t vreg[32][2]; } Ctx;
enum { HOS_OPT_FOV = 51 };
static unsigned char objects[0x10000], globals[0x20000];
static int menu = 100, reads;
static void *ptr(uint64_t a, unsigned n) {
    if (a >= 0x10000000ULL && a + n <= 0x10010000ULL) return objects + (a - 0x10000000ULL);
    CHECK(a >= 0x84c90000ULL && a + n <= 0x84cb0000ULL);
    return globals + (a - 0x84c90000ULL);
}
static uint64_t recomp_load64(Ctx *c, uint64_t a) {
    uint64_t v; (void)c; reads++; memcpy(&v, ptr(a, 8), 8); return v;
}
static uint32_t recomp_load32(Ctx *c, uint64_t a) {
    uint32_t v; (void)c; reads++; memcpy(&v, ptr(a, 4), 4); return v;
}
static void a64_vset64(uint64_t *d, uint64_t v) { d[0] = v; d[1] = 0; }
int rx_option(int id, int otherwise) { CHECK(id == HOS_OPT_FOV && otherwise == 100); return menu; }
"##;
    const MAIN: &str = r##"
static void site(Ctx *c) {
    hos_oot_fov(c, c->x[19]);
}
static void put64(uint64_t a, uint64_t v) { memcpy(ptr(a, 8), &v, 8); }
static void put32(uint64_t a, uint32_t v) { memcpy(ptr(a, 4), &v, 4); }
static const uint64_t camera = 0x10001000, views = 0x10002000, view = 0x10003000;
static const uint64_t singleton = 0x10004000, manager = 0x10005000, cameras = 0x10006000, main_camera = 0x10007000;
static const uint64_t events_singleton = 0x10008000, events = 0x10009000;
static Ctx input(float angle) {
    Ctx c; memset(&c, 0xab, sizeof c);
    c.x[19] = camera;
    uint32_t bits; memcpy(&bits, &angle, 4); a64_vset64(c.vreg[0], bits);
    return c;
}
static float output(Ctx *c) { float f; memcpy(&f, c->vreg[0], 4); return f; }
static int near(float a, float b) { return fabsf(a - b) < 1e-5f; }
static void unchanged(float angle) {
    Ctx c = input(angle), before = c; site(&c); CHECK(!memcmp(&c, &before, sizeof c));
}
static void setup(void) {
    put64(camera + 0x18, views); put64(views + 8, view); put64(view, 0x84c18a88ULL);
    put64(0x84ca6bc0ULL, singleton); put64(singleton, manager);
    put32(manager + 32, 1); put64(manager + 40, cameras); put64(cameras, main_camera);
    put64(main_camera + 120, view);
    put64(0x84c9e090ULL, events_singleton); put64(events_singleton, events);
    put64(events + 104, 0);
}
int main(void) {
    /* Original and unsupported choices touch no camera memory, preserve all bits. */
    unchanged(1.0f); unchanged(NAN); CHECK(reads == 0);
    for (int i = 0; i < 3; i++) { menu = (int[]){0, 49, 151}[i]; unchanged(1.0f); }
    CHECK(reads == 0);
    setup(); menu = 150;
    unsigned char before_objects[sizeof objects], before_globals[sizeof globals];
    memcpy(before_objects, objects, sizeof objects); memcpy(before_globals, globals, sizeof globals);
    /* One authored angle each update: no compounded scale. */
    for (int i = 0; i < 10; i++) {
        Ctx c = input(1.0f), expected = c;
        uint32_t bits; float wider = 1.5f; memcpy(&bits, &wider, 4); a64_vset64(expected.vreg[0], bits);
        site(&c); CHECK(!memcmp(&c, &expected, sizeof c));
    }
    menu = 50; Ctx c = input(1.0f); site(&c); CHECK(near(output(&c), 0.5f));
    menu = 125; c = input(0.4f); site(&c); CHECK(near(output(&c), 0.5f));
    menu = 100; unchanged(0.4f);   /* a live reset, with no cached scaled value */
    menu = 150;
    CHECK(!memcmp(objects, before_objects, sizeof objects) && !memcmp(globals, before_globals, sizeof globals));
    c = input(3.0f); site(&c); CHECK(near(output(&c), 2.443460953f));
    menu = 50; c = input(0.01f); site(&c); CHECK(near(output(&c), 0.087266463f));
    menu = 150; unchanged(NAN); unchanged(INFINITY); unchanged(0.0f); unchanged(-1.0f); unchanged(3.2f);

    /* Other views and scripted events retain their angles. Null links stop safely. */
    put64(main_camera + 120, view + 0x100); unchanged(1.0f); setup();
    put64(events + 104, 1); unchanged(1.0f); setup();
    put64(view, 0); unchanged(1.0f); setup();
    put64(camera + 0x18, 0); unchanged(1.0f); setup();
    put64(views + 8, 0); unchanged(1.0f); setup();
    put64(0x84ca6bc0ULL, 0); unchanged(1.0f); setup();
    put64(singleton, 0); unchanged(1.0f); setup();
    put32(manager + 32, 0); unchanged(1.0f); setup();
    put64(manager + 40, 0); unchanged(1.0f); setup();
    put64(cameras, 0); unchanged(1.0f); setup();
    c = input(1.0f); c.x[19] = 0; Ctx before = c; site(&c); CHECK(!memcmp(&c, &before, sizeof c));
    puts("PASS: live FOV, original reset, zoom, view ownership, events, bounds and register preservation");
    return 0;
}
"##;
    let dir = scratch("oot-fov");
    let fixture = dir.join("fixture.c");
    fs::write(&fixture, format!("{HEAD}{body}{MAIN}")).expect("write the FOV fixture");
    let Some(run) = compile_and_run("oot-fov", &[fixture], &[], &["-lm"]) else {
        eprintln!("skipping the field of view fixture: no C compiler (CC, cc, gcc or clang)");
        let _ = fs::remove_dir_all(dir);
        return;
    };
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).starts_with("PASS"));
}
