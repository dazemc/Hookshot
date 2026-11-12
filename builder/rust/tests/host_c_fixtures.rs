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
fn the_linux_saves_move_follows_links_in_any_directory_order() {
    if !cfg!(target_os = "linux") {
        // (the fixture builds the host's Linux win32 layer: symlink, readlink and win32_posix.c)
        return;
    }
    let Some(cc) = compiler() else {
        eprintln!("skipping the Linux saves move fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/hos_main.c")).expect("host/hos_main.c");
    let start = source
        .find("static int is_dir(const char *p) {")
        .expect("the saves move has no is_dir");
    let saves_dir = source
        .find("static const char *saves_dir(const char *beside) {")
        .expect("the saves move has no saves_dir");
    let end = source[saves_dir..]
        .find("#endif")
        .map(|at| saves_dir + at)
        .expect("the saves move has no end");
    let functions = &source[start..end];
    let fixture = format!(
        r#"#define _GNU_SOURCE
#include <windows.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define SEP '/'
#define SEP_S "/"
static void hos_log(const char *fmt, ...) {{
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
}}
{functions}
int main(int argc, char **argv) {{
    if (argc != 2) return 2;
    printf("%s\n", saves_dir(argv[1]));
    return 0;
}}
"#
    );
    let dir = scratch("saves-move-c");
    let path = dir.join("fixture.c");
    fs::write(&path, fixture).expect("write the Linux saves move fixture");
    let exe = dir.join("fixture");
    let build = Command::new(&cc)
        .args(["-std=gnu11", "-D_GNU_SOURCE", "-w"])
        .arg("-I")
        .arg(repo.join("host/posix"))
        .arg(&path)
        .arg(repo.join("host/posix/win32_posix.c"))
        .arg("-o")
        .arg(&exe)
        .args(["-lpthread", "-ldl"])
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let root = scratch("saves-move-tree");
    let game = root.join("game");
    let user = game.join("user");
    let data = root.join("data");
    let sync = root.join("sync");
    let uid = "5a3c9e1f0b7d24680000000000000001";
    fs::create_dir_all(&user).unwrap();
    fs::create_dir_all(sync.join(uid)).unwrap();
    fs::write(sync.join(uid).join("account.sav"), b"my save").unwrap();
    let save = user.join("save");
    let id = save.join(uid);
    // (links made before and after save/: on a file system that lists in
    // creation order some are seen after save/ has moved, as upstream's
    // newest-first listing does; both must follow it)
    let names: Vec<String> = "abcdefghijklmnopqrstuvwxyz0123456789"
        .chars()
        .map(|ch| format!("last-{ch}"))
        .collect();
    for name in &names[..18] {
        std::os::unix::fs::symlink(&id, user.join(name)).unwrap();
    }
    fs::create_dir_all(&id).unwrap();
    fs::write(id.join("account.sav"), b"my save").unwrap();
    for name in &names[18..] {
        std::os::unix::fs::symlink(&id, user.join(name)).unwrap();
    }
    std::os::unix::fs::symlink(&sync, user.join("sync")).unwrap();
    // (written from where it stands: the move names the folder in full)
    std::os::unix::fs::symlink("../../sync", user.join("relative")).unwrap();
    std::os::unix::fs::symlink("../../nowhere", user.join("dangling")).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", user.join("outside")).unwrap();

    let new = data.join("hookshot");
    for start in 1..=2 {
        let run = Command::new(&exe)
            .arg(&user)
            .env("XDG_DATA_HOME", &data)
            .output()
            .expect("run the saves move");
        assert!(
            run.status.success(),
            "start {start}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).trim(),
            new.to_string_lossy(),
            "start {start} did not use the new place"
        );
    }
    assert!(!user.exists(), "user/ is still there");
    assert!(new.join("save").join(uid).join("account.sav").is_file());
    assert_eq!(
        fs::read(&sync.join(uid).join("account.sav")).unwrap(),
        b"my save"
    );
    assert_eq!(fs::read_link(new.join("sync")).unwrap(), sync);
    assert_eq!(fs::read_link(new.join("relative")).unwrap(), sync);
    assert_eq!(
        fs::read_link(new.join("dangling")).unwrap(),
        Path::new("../../nowhere")
    );
    assert_eq!(
        fs::read_link(new.join("outside")).unwrap(),
        Path::new("/etc/hostname")
    );
    for name in &names {
        assert_eq!(
            fs::read_link(new.join(name)).unwrap(),
            new.join("save").join(uid),
            "{name} does not follow save/ to the new place"
        );
    }
    assert!(
        fs::read_to_string(game.join("saves-moved.txt"))
            .unwrap()
            .contains(new.to_string_lossy().as_ref())
    );

    // (a user/ that is a link itself is the player's choice of place, used as it is)
    let linked_game = root.join("linked-game");
    let real_user = root.join("real-user");
    fs::create_dir_all(real_user.join("save").join(uid)).unwrap();
    fs::write(
        real_user.join("save").join(uid).join("account.sav"),
        b"linked",
    )
    .unwrap();
    fs::create_dir_all(&linked_game).unwrap();
    std::os::unix::fs::symlink(&real_user, linked_game.join("user")).unwrap();
    let linked_data = root.join("linked-data");
    let run = Command::new(&exe)
        .arg(linked_game.join("user"))
        .env("XDG_DATA_HOME", &linked_data)
        .output()
        .expect("run the saves move on a linked user folder");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        linked_game.join("user").to_string_lossy()
    );
    assert!(
        real_user
            .join("save")
            .join(uid)
            .join("account.sav")
            .is_file()
    );
    assert!(!linked_data.join("hookshot").exists());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&dir);
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
