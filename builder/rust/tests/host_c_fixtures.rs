// Game-free checks of the C host's pure parts, compiled with the host C compiler: the Steam pads' ignore-list filter
// (host/steam_pads.h, upstream NotRexed/Hookshot e4d45157), the gameplay field of view (host/oot_fov.c, upstream
// ba472ea3), the RomFS mods and their resource size table (host/mods.c, host/prompts.c, upstream bd9b07e6) and, later
// here, the other fixtures that need nothing but a compiler. A machine with no C compiler skips them, as CI's Rust job
// may have none on Windows.

use std::collections::BTreeMap;
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

/// Compile a C fixture with the host compiler, returning its folder and executable. `None` when there is no compiler,
/// so the caller can say it skipped. `strict` adds -Werror (some production sources carry warnings under it).
fn compile_fixture(
    name: &str,
    sources: &[PathBuf],
    includes: &[PathBuf],
    flags: &[&str],
    strict: bool,
) -> Option<(PathBuf, PathBuf)> {
    let cc = compiler()?;
    let dir = scratch(name);
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut build = Command::new(&cc);
    build.args(["-std=c11", "-Wall", "-Wextra"]);
    if strict {
        build.arg("-Werror");
    }
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
    Some((dir, exe))
}

/// Compile a C fixture with the host compiler, run it, and return its output. `None` when there is no compiler, so
/// the caller can say it skipped.
fn compile_and_run_with(
    name: &str,
    sources: &[PathBuf],
    includes: &[PathBuf],
    flags: &[&str],
    args: &[&str],
    envs: &[(&str, &str)],
    strict: bool,
) -> Option<Output> {
    let (dir, exe) = compile_fixture(name, sources, includes, flags, strict)?;
    let mut run = Command::new(&exe);
    run.args(args);
    for (name, value) in envs {
        run.env(name, value);
    }
    let run = run.output().expect("run the fixture");
    let _ = fs::remove_dir_all(dir);
    Some(run)
}

fn compile_and_run(
    name: &str,
    sources: &[PathBuf],
    includes: &[PathBuf],
    flags: &[&str],
) -> Option<Output> {
    compile_and_run_with(name, sources, includes, flags, &[], &[], true)
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
fn a_saves_link_follows_an_entry_that_moved_first() {
    if cfg!(windows) {
        // (the fixture is POSIX: mkdtemp, readlink and symlink)
        return;
    }
    let repo = root();
    let source = fs::read_to_string(repo.join("host/hos_main.c")).expect("host/hos_main.c");
    // (the move's link mapping and copy_link, from the source as it is: the
    // late link is the case a file system's directory order decides)
    let start = source
        .find("static const char *move_old, *move_from, *move_new;")
        .expect("the saves move has no move_from");
    let end = source
        .find("static int has_link(")
        .expect("the saves move has no copy_link");
    let functions = &source[start..end];
    let fixture = format!(
        r#"#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#ifndef MAX_PATH
#define MAX_PATH 4096
#endif
#define SEP '/'
#define SEP_S "/"
{functions}
static int dir(const char *p) {{ return mkdir(p, 0700) == 0 || errno == EEXIST; }}
static int text(const char *p, char *out, size_t n) {{
    const ssize_t r = readlink(p, out, n - 1);
    if (r < 0) return 0;
    out[r] = 0;
    return 1;
}}
static int is(const char *a, const char *b) {{ return strcmp(a, b) == 0; }}
int main(void) {{
    char base[] = "/tmp/hookshot-saves-link-XXXXXX", old[256], new[256];
    char save[512], id[1024], from[1024], to[1024], target[MAX_PATH], want[1024];
    if (!mkdtemp(base)) return 2;
    snprintf(old, sizeof old, "%s/old", base);
    snprintf(new, sizeof new, "%s/new", base);
    if (!dir(old) || !dir(new)) return 3;
    snprintf(save, sizeof save, "%s/save", old);
    snprintf(id, sizeof id, "%s/id", save);
    if (!dir(save) || !dir(id)) return 4;
    snprintf(want, sizeof want, "%s/save", new);
    if (!dir(want)) return 5;
    snprintf(want, sizeof want, "%s/save/id", new);
    if (!dir(want)) return 6;
    char real[MAX_PATH];
    if (!realpath(old, real)) return 7;
    move_old = real;
    move_from = old;
    move_new = new;

    /* The entry moved first: the link's text names nothing by now, and must
       still be pointed at the entry's new place. */
    snprintf(from, sizeof from, "%s/last", old);
    snprintf(to, sizeof to, "%s/last", new);
    if (symlink(id, from) || rmdir(id) || rmdir(save)) return 8;
    if (!copy_link(from, to)) return 9;
    if (!text(to, target, sizeof target)) return 10;
    if (!is(target, want)) {{
        fprintf(stderr, "the late link names %s, not %s\n", target, want);
        return 11;
    }}

    /* The same when realpath could not name the old folder: the text as
       given is checked too. */
    if (!dir(save) || !dir(id)) return 12;
    snprintf(from, sizeof from, "%s/last-2", old);
    snprintf(to, sizeof to, "%s/last-2", new);
    if (symlink(id, from) || rmdir(id) || rmdir(save)) return 13;
    move_old = NULL;
    if (!copy_link(from, to)) return 14;
    if (!text(to, target, sizeof target) || !is(target, want)) return 15;
    move_old = real;

    /* A link that names nothing, and one to something outside the move,
       keep their text. */
    snprintf(from, sizeof from, "%s/broken", old);
    snprintf(to, sizeof to, "%s/broken", new);
    if (symlink("../../nowhere", from) || !copy_link(from, to)) return 16;
    if (!text(to, target, sizeof target) || !is(target, "../../nowhere")) return 17;
    snprintf(from, sizeof from, "%s/outside", old);
    snprintf(to, sizeof to, "%s/outside", new);
    if (symlink("/etc/hostname", from) || !copy_link(from, to)) return 18;
    if (!text(to, target, sizeof target) || !is(target, "/etc/hostname")) return 19;

    char clean[MAX_PATH + 16];
    snprintf(clean, sizeof clean, "rm -rf '%s'", base);
    (void)system(clean);
    puts("PASS: a saves link follows an entry that moved first, and dangling and outside links keep their text");
    return 0;
}}
"#
    );
    let dir = scratch("saves-link");
    let path = dir.join("fixture.c");
    fs::write(&path, fixture).expect("write the saves link fixture");
    let Some(run) = compile_and_run("saves-link", &[path], &[], &[]) else {
        eprintln!("skipping the saves link fixture: no C compiler (CC, cc, gcc or clang)");
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

fn put(base: &Path, rel: &str, body: &str) {
    let path = base.join(rel);
    fs::create_dir_all(path.parent().expect("a file's folder")).expect("make the fixture folder");
    fs::write(path, body).expect("write the fixture file");
}

#[test]
fn mods_load_the_emulator_layout_and_pick_the_exact_size_table() {
    let repo = root();
    let dir = scratch("mods-layout");
    let mods = dir.join("mods");
    // Two mods replace the same file: 02-b sorts last and wins. Each also has a file the other lacks.
    put(&mods, "01-a/romfs/UI/Shared.zs", "a");
    put(&mods, "02-b/romfs/UI/Shared.zs", "b");
    put(&mods, "01-a/romfs/UI/OnlyA.zs", "a");
    put(&mods, "02-b/romfs/UI/ModOnly.zs", "b");
    fs::create_dir_all(mods.join("02-b/romfs/UI/Sub")).expect("make the mod folder");
    // A mod may carry several game versions' tables: the prompts ask for the game's exact one.
    put(
        &mods,
        "01-a/romfs/System/Resource/ResourceSizeTable.Product.110.Nin_Ounce_NVN2.rsizetable.zs",
        "110",
    );
    put(
        &mods,
        "02-b/romfs/System/Resource/ResourceSizeTable.Product.111.Nin_Ounce_NVN2.rsizetable.zs",
        "111",
    );
    // The emulator layout that sits deeper is a mod too.
    put(
        &mods,
        "contents/0100000000001000/deep/romfs/Deep/File.zs",
        "deep",
    );
    // Off folders, a folder that is not a mod, and an exefs-only mod.
    put(&mods, "_off/romfs/Off.zs", "off");
    put(&mods, "-off2/romfs/Off2.zs", "off");
    put(&mods, "notamod/NotAMod.zs", "not a mod");
    put(&mods, "exefs-only/exefs/main.pchtxt", "patch");
    // The game's own folder, for the listing: UI/Shared.zs is the game's, not a mod extra.
    put(&mods, "game/UI/Shared.zs", "game");

    let stub = dir.join("stub.h");
    fs::write(
        &stub,
        "#pragma once\n#include <stddef.h>\nvoid hos_log(const char *fmt, ...);\n",
    )
    .expect("write the hos.h stand-in");
    let stub_arg = stub.to_str().expect("the stub path");
    let mut flags = vec!["-DHOS_H", "-include", stub_arg];
    if !cfg!(windows) {
        flags.push("-D_GNU_SOURCE");
    }
    let mut sources = vec![repo.join("tests/mods_test.c"), repo.join("host/mods.c")];
    let mut includes = vec![repo.join("host")];
    if !cfg!(windows) {
        sources.push(repo.join("host/posix/win32_posix.c"));
        includes.push(repo.join("host/posix"));
    }
    let mods_arg = mods.to_str().expect("the mods path");
    let Some(run) = compile_and_run_with(
        "mods-layout",
        &sources,
        &includes,
        &flags,
        &[],
        &[("HOS_MODS", mods_arg)],
        false,
    ) else {
        eprintln!("skipping the mods fixture: no C compiler (CC, cc, gcc or clang)");
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
    assert!(
        String::from_utf8_lossy(&run.stdout).starts_with("ok:"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}

#[test]
fn the_prompts_pick_the_game_size_table_not_another_version() {
    let repo = root();
    let source = fs::read_to_string(repo.join("host/prompts.c")).expect("host/prompts.c");
    let table = function(&source, "static int game_table(");
    // (the prompts ask for the exact table the game opens, not the first matching one in a mod)
    assert!(source.contains("hos_mods_file(table"));
    assert!(!source.contains("hos_mods_find("));
    let fixture = format!(
        r#"#include <stdio.h>
#include <string.h>
#include <windows.h>
const char *hos_romfs_dir;
{table}
int main(int argc, char **argv) {{
    if (argc != 2) return 2;
    hos_romfs_dir = argv[1];
    char out[4096];
    if (!game_table(out, sizeof out)) {{ puts("none"); return 3; }}
    puts(out);
    return 0;
}}
"#
    );
    let dir = scratch("game-table");
    let c = dir.join("fixture.c");
    fs::write(&c, fixture).expect("write the game table fixture");
    let romfs = dir.join("romfs");
    let resource = romfs.join("System/Resource");
    fs::create_dir_all(&resource).expect("make the RomFS folder");
    // (the version that sorts first is made second, so directory order is not the answer)
    fs::write(
        resource.join("ResourceSizeTable.Product.111.Nin_Ounce_NVN2.rsizetable.zs"),
        b"111",
    )
    .expect("write a table");
    fs::write(
        resource.join("ResourceSizeTable.Product.110.Nin_Ounce_NVN2.rsizetable.zs"),
        b"110",
    )
    .expect("write the table");

    let mut sources = vec![c];
    let mut includes = Vec::new();
    let mut flags = Vec::new();
    if !cfg!(windows) {
        sources.push(repo.join("host/posix/win32_posix.c"));
        includes.push(repo.join("host/posix"));
        flags.push("-D_GNU_SOURCE");
    }
    let romfs_arg = romfs.to_str().expect("the RomFS path");
    let Some(run) = compile_and_run_with(
        "game-table",
        &sources,
        &includes,
        &flags,
        &[romfs_arg],
        &[],
        false,
    ) else {
        eprintln!("skipping the game table fixture: no C compiler (CC, cc, gcc or clang)");
        let _ = fs::remove_dir_all(dir);
        return;
    };
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "System/Resource/ResourceSizeTable.Product.110.Nin_Ounce_NVN2.rsizetable.zs"
    );

    let empty = dir.join("empty");
    fs::create_dir_all(&empty).expect("make the empty RomFS folder");
    let empty_arg = empty.to_str().expect("the empty RomFS path");
    let Some(run) = compile_and_run_with(
        "game-table-none",
        &sources,
        &includes,
        &flags,
        &[empty_arg],
        &[],
        false,
    ) else {
        let _ = fs::remove_dir_all(dir);
        return;
    };
    let _ = fs::remove_dir_all(dir);
    assert_eq!(run.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "none");
}

#[test]
fn a_mods_size_table_is_merged_with_the_prompts_entries() {
    let repo = root();
    let source = fs::read_to_string(repo.join("host/prompts.c")).expect("host/prompts.c");
    let functions = [
        function(&source, "static uint32_t rd32le("),
        function(&source, "static void wr32le("),
        function(&source, "static int restbl_head("),
        function(&source, "static int restbl_cmp("),
        function(&source, "static long restbl_part("),
        function(&source, "static uint8_t *merge_restbl("),
    ]
    .join("\n");
    let harness = format!(
        r#"#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
{functions}
static uint8_t *load(const char *path, size_t *n) {{
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    const long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *p = len > 0 ? (uint8_t *)malloc((size_t)len) : NULL;
    if (!p || fread(p, 1, (size_t)len, f) != (size_t)len) {{ fclose(f); free(p); return NULL; }}
    fclose(f);
    *n = (size_t)len;
    return p;
}}
int main(int argc, char **argv) {{
    if (argc != 4) return 2;
    size_t an = 0, bn = 0, n = 0;
    uint8_t *a = load(argv[1], &an), *b = load(argv[2], &bn);
    if (!a || !b) return 3;
    int raised = 0;
    uint8_t *t = merge_restbl(a, an, b, bn, &n, &raised);
    if (!t) {{ puts("refused"); free(a); free(b); return 0; }}
    FILE *f = fopen(argv[3], "wb");
    if (!f) return 4;
    const int ok = fwrite(t, 1, n, f) == n && fclose(f) == 0;
    free(t), free(a), free(b);
    if (!ok) return 5;
    printf("%d\n", raised);
    return 0;
}}
"#
    );
    let dir = scratch("merge-restbl");
    let c = dir.join("fixture.c");
    fs::write(&c, harness).expect("write the merge fixture");
    let Some((build_dir, exe)) = compile_fixture("merge-restbl", &[c], &[], &[], true) else {
        eprintln!("skipping the size table fixture: no C compiler (CC, cc, gcc or clang)");
        let _ = fs::remove_dir_all(dir);
        return;
    };

    fn restbl_unsorted(
        hashed: &[(u32, u32)],
        named: &[(&str, u32)],
        name_size: u32,
        magic: &[u8],
    ) -> Vec<u8> {
        assert_eq!(magic.len(), 6);
        let mut out = Vec::new();
        out.extend_from_slice(magic);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&name_size.to_le_bytes());
        out.extend_from_slice(&(hashed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(named.len() as u32).to_le_bytes());
        for (key, size) in hashed {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
        }
        for (name, size) in named {
            let mut field = name.as_bytes().to_vec();
            field.resize(name_size as usize, 0);
            out.extend_from_slice(&field);
            out.extend_from_slice(&size.to_le_bytes());
        }
        out
    }

    fn restbl(
        hashed: &[(u32, u32)],
        named: &[(&str, u32)],
        name_size: u32,
        magic: &[u8],
    ) -> Vec<u8> {
        let mut hashed = hashed.to_vec();
        hashed.sort_by_key(|e| e.0);
        let mut named = named.to_vec();
        named.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        restbl_unsorted(&hashed, &named, name_size, magic)
    }

    fn entries(t: &[u8]) -> (u32, BTreeMap<u32, u32>, BTreeMap<String, u32>) {
        let name_size = u32::from_le_bytes(t[10..14].try_into().unwrap());
        let hashed_n = u32::from_le_bytes(t[14..18].try_into().unwrap()) as usize;
        let named_n = u32::from_le_bytes(t[18..22].try_into().unwrap()) as usize;
        let mut hashed = BTreeMap::new();
        for i in 0..hashed_n {
            let at = 22 + 8 * i;
            hashed.insert(
                u32::from_le_bytes(t[at..at + 4].try_into().unwrap()),
                u32::from_le_bytes(t[at + 4..at + 8].try_into().unwrap()),
            );
        }
        let at = 22 + 8 * hashed_n;
        let mut named = BTreeMap::new();
        for i in 0..named_n {
            let e = at + (name_size as usize + 4) * i;
            let end = e + name_size as usize;
            let name =
                String::from_utf8(t[e..end].iter().take_while(|b| **b != 0).copied().collect())
                    .unwrap();
            named.insert(
                name,
                u32::from_le_bytes(t[end..end + 4].try_into().unwrap()),
            );
        }
        (name_size, hashed, named)
    }

    fn merged(a: &[u8], b: &[u8]) -> Vec<u8> {
        let (name_size, ah, an) = entries(a);
        let (_, bh, bn) = entries(b);
        let hashed: BTreeMap<u32, u32> = ah
            .keys()
            .chain(bh.keys())
            .map(|k| {
                (
                    *k,
                    ah.get(k)
                        .copied()
                        .unwrap_or(0)
                        .max(bh.get(k).copied().unwrap_or(0)),
                )
            })
            .collect();
        let named: BTreeMap<String, u32> = an
            .keys()
            .chain(bn.keys())
            .map(|k| {
                (
                    k.clone(),
                    an.get(k)
                        .copied()
                        .unwrap_or(0)
                        .max(bn.get(k).copied().unwrap_or(0)),
                )
            })
            .collect();
        let hashed: Vec<(u32, u32)> = hashed.into_iter().collect();
        let named: Vec<(&str, u32)> = named.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        restbl(&hashed, &named, name_size, b"RESTBL")
    }

    let run = |a: &[u8], b: &[u8]| -> Option<(i32, Vec<u8>)> {
        fs::write(dir.join("a"), a).expect("write a table");
        fs::write(dir.join("b"), b).expect("write a table");
        let out = Command::new(&exe)
            .arg(dir.join("a"))
            .arg(dir.join("b"))
            .arg(dir.join("out"))
            .output()
            .expect("run the merge fixture");
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text == "refused" {
            None
        } else {
            Some((
                text.parse().expect("the raised count"),
                fs::read(dir.join("out")).expect("read the merged table"),
            ))
        }
    };

    // (made-up tables: the prompts raise the font's entry; the mod raises a file's and adds another)
    let ours = restbl(
        &[(0x10, 100), (0x20, 200), (0x30, 900), (0x40, 400)],
        &[("Collide/A", 50), ("Collide/B", 60)],
        160,
        b"RESTBL",
    );
    let mod_b = restbl(
        &[
            (0x10, 100),
            (0x20, 250),
            (0x30, 300),
            (0x40, 400),
            (0x50, 500),
        ],
        &[("Collide/A", 50), ("Collide/B", 70)],
        160,
        b"RESTBL",
    );
    let (raised, got) = run(&mod_b, &ours).expect("the mod's table merged");
    assert_eq!(got, merged(&mod_b, &ours));
    assert_eq!(raised, 1);
    assert_eq!(
        u32::from_le_bytes(got[22 + 8 * 2..22 + 8 * 2 + 4].try_into().unwrap()),
        0x30
    );
    assert_eq!(
        u32::from_le_bytes(got[22 + 8 * 2 + 4..22 + 8 * 2 + 8].try_into().unwrap()),
        900
    );
    assert_eq!(
        u32::from_le_bytes(got[22 + 8 * 1 + 4..22 + 8 * 1 + 8].try_into().unwrap()),
        250
    );

    // (entries on one side only, both ways, and named ones: each the larger, sorted)
    let a = restbl(
        &[(5, 1), (0x20, 999), (0xFFFFFFF0, 7)],
        &[("B", 9), ("D", 1)],
        160,
        b"RESTBL",
    );
    let b = restbl(
        &[(1, 3), (0x20, 10), (0x30, 4)],
        &[("A", 2), ("B", 10), ("C", 3)],
        160,
        b"RESTBL",
    );
    let (raised, got) = run(&a, &b).expect("entries on one side only merged");
    assert_eq!(got, merged(&a, &b));
    assert_eq!(raised, 5);
    let (raised, got) = run(&restbl(&[], &[], 160, b"RESTBL"), &b).expect("an empty mod table");
    assert_eq!(got, b);
    assert_eq!(raised, 6);

    // (tables it cannot merge: the mod's is served as it is, and the game's icons stay)
    for (what, table, ours) in [
        (
            "not a RESTBL",
            restbl(&[(1, 1)], &[], 160, b"RESTBX"),
            ours.clone(),
        ),
        (
            "a wrong length",
            [mod_b.clone(), vec![0]].concat(),
            ours.clone(),
        ),
        (
            "a short table",
            b"RESTBL"
                .iter()
                .copied()
                .chain([0u8; 10])
                .collect::<Vec<u8>>(),
            ours.clone(),
        ),
        (
            "other name fields",
            restbl(&[(1, 1)], &[], 128, b"RESTBL"),
            ours.clone(),
        ),
        (
            "hashed entries out of order",
            restbl_unsorted(&[(2, 1), (1, 1)], &[], 160, b"RESTBL"),
            ours.clone(),
        ),
        (
            "a hashed entry twice",
            restbl_unsorted(&[(1, 1), (1, 2)], &[], 160, b"RESTBL"),
            ours.clone(),
        ),
        (
            "named entries out of order",
            mod_b.clone(),
            restbl_unsorted(&[], &[("B", 1), ("A", 1)], 160, b"RESTBL"),
        ),
    ] {
        assert!(run(&table, &ours).is_none(), "{what} was merged");
    }

    let _ = fs::remove_dir_all(dir);
    let _ = fs::remove_dir_all(build_dir);
}
