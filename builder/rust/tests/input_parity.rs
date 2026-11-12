// The newest upstream input fixes, game-free: a pad's gyro stops with its buttons behind another window (#2542),
// the stick dead zone keeps a diagonal's direction (#1600), and a connection logs the SDL controller's GUID and
// active mapping (#2678). The host C functions are cut out of their sources and compiled with stubs, as the other
// host fixtures here are; a machine with no C compiler skips the compiled parts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A C function's definition from a source, braces matched.
fn function(text: &str, marker: &str) -> String {
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

/// Compile one C fixture and run it. `None` when there is no compiler, so the caller can say it skipped.
fn run_fixture(name: &str, code: &str, includes: &[PathBuf], flags: &[&str]) -> Option<Output> {
    let cc = compiler()?;
    let dir = scratch(name);
    let fixture = dir.join("fixture.c");
    fs::write(&fixture, code).expect("write the C fixture");
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
    let built = build
        .arg(&fixture)
        .args(flags)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C compiler");
    assert!(
        built.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&exe).output().expect("run the fixture");
    let _ = fs::remove_dir_all(dir);
    Some(run)
}

fn assert_passed(run: Output, what: &str) {
    assert!(
        run.status.success(),
        "{what}: {}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("PASS"),
        "{what}: {}",
        String::from_utf8_lossy(&run.stdout)
    );
}

const MOTION_PRELUDE: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "hos_motion.h"
typedef struct { uint64_t x[32]; } Ctx;
#define HLE(sym) void imp_##sym(Ctx *c)
#define ARG(n) (c->x[n])
#define RET(v) (c->x[0] = (uint64_t)(v))
#define GP(a) ((void *)&memory[(a)])
#define G64(a) (*(uint64_t *)GP(a))
#define G32(a) (*(uint32_t *)GP(a))
static unsigned char memory[4096];
static int front, menu, history_calls;
static int rx_option(int id, int otherwise) { return id == 4 ? menu : otherwise; }
static int game_in_front(void) { return front; }
static int hos_sdl_motion_history(HosMotionSample *out, int max) {
    history_calls++;
    const int n = max < 3 ? max : 3;
    for (int i = 0; i < n; i++) {
        hos_motion_rest(&out[i].st);
        out[i].st.gyro[0] = 0.5f + (float)i;
        out[i].n = 100 - i;
        out[i].dt_ns = 5000000;
    }
    return n;
}
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)
"#;

const MOTION_MAIN: &str = r#"
static float f32(uint64_t a) { float v; memcpy(&v, GP(a), sizeof v); return v; }
static uint64_t u64at(uint64_t a) { uint64_t v; memcpy(&v, GP(a), sizeof v); return v; }
static int six(int count) {
    Ctx c = {{0}};
    c.x[0] = 64;
    c.x[1] = (uint64_t)count;
    imp__ZN2nn3hid22GetSixAxisSensorStatesEPNS0_18SixAxisSensorStateEiRKNS0_19SixAxisSensorHandleE(&c);
    return (int)c.x[0];
}
int main(void) {
    /* The buttons' rule, as the pads' own: in front, the row, or HOS_INPUT_BACKGROUND over both. */
    CHECK(!pads_in_background(NULL, 0));
    CHECK(pads_in_background(NULL, 1));
    CHECK(pads_in_background("1", 0));
    CHECK(!pads_in_background("0", 1));
    CHECK(motion_counts(1, NULL, 0));
    CHECK(!motion_counts(0, NULL, 0));
    CHECK(motion_counts(0, NULL, 1));
    CHECK(motion_counts(0, "1", 0));
    CHECK(!motion_counts(0, "0", 1));

    /* In front: the live readings reach the game, gyro and all. */
    front = 1, menu = 0, history_calls = 0;
    CHECK(six(3) == 3);
    CHECK(history_calls == 1);
    CHECK(f32(64 + 0x1c) == 0.5f);          /* (the first reading's gyro x) */
    CHECK(f32(64 + 0x18) == -1.0f);         /* (a real pad's accelerometer, not the rest pose) */
    const uint64_t live_before = u64at(64 + 8);

    /* Behind another window with the row off: no motion is read, and the game gets the pad at rest. */
    front = 0, menu = 0, history_calls = 0;
    CHECK(six(3) == 3);
    CHECK(history_calls == 0);
    for (int i = 0; i < 3; i++) {
        const uint64_t e = 64 + (uint64_t)i * 0x60;
        CHECK(f32(e + 0x1c) == 0.0f && f32(e + 0x20) == 0.0f && f32(e + 0x24) == 0.0f);
        CHECK(f32(e + 0x18) == -1.0f);
    }
    const uint64_t rest = u64at(64 + 8);

    /* The row on, behind another window: live again. */
    menu = 1, history_calls = 0;
    CHECK(six(3) == 3);
    CHECK(history_calls == 1);
    CHECK(f32(64 + 0x1c) == 0.5f);
    const uint64_t live_after = u64at(64 + 8);

    /* Each switch moves the sample numbers a whole history past the last ones given out. */
    CHECK(rest > live_before && live_after > rest);

    puts("PASS: gyro live in front and at rest behind another window, with the menu's row and the environment");
    return 0;
}
"#;

#[test]
fn a_pads_gyro_stops_with_its_buttons_behind_another_window() {
    let hid = source("host/nn_hid.c");
    assert!(
        function(
            &hid,
            "HLE(_ZN2nn3hid22GetSixAxisSensorStatesEPNS0_18SixAxisSensorStateEiRKNS0_19SixAxisSensorHandleE)",
        )
        .contains("motion_counts(game_in_front(), background_env, rx_option(4, 0))"),
        "GetSixAxisSensorStates does not apply the buttons' background rule to motion"
    );
    let code = [
        MOTION_PRELUDE,
        &function(&hid, "static int pads_in_background("),
        &function(&hid, "static int motion_counts("),
        &function(
            &hid,
            "HLE(_ZN2nn3hid22GetSixAxisSensorStatesEPNS0_18SixAxisSensorStateEiRKNS0_19SixAxisSensorHandleE)",
        ),
        MOTION_MAIN,
    ]
    .join("\n");
    let Some(run) = run_fixture("motion-background", &code, &[root().join("host")], &[]) else {
        eprintln!("skipping the motion background fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    assert_passed(run, "the motion background fixture");
}

const STICK_PRELUDE: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
enum { AX_LX, AX_LY, AX_RX, AX_RY };
static struct { unsigned rested; signed char held[4]; } pads[2];
static int options[20];
static int rx_option(int id, int fallback) { (void)fallback; return options[id]; }
static int16_t hos_sdl_pad_axis(void *gc, int axis) { return ((int16_t *)gc)[axis]; }
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)
"#;

const STICK_POLL_OPEN: &str = r#"
static void poll(int i, int16_t raw[4], int32_t output[4]) {
    void *gc = raw;
"#;

const STICK_MAIN: &str = r#"
    memcpy(output, v, sizeof v);
}
static int expected(int raw, int axis) {
    int value = raw < -32767 ? -32767 : raw;
    if (axis == AX_LY || axis == AX_RY) value = -value;
    if ((axis == AX_RX && options[16]) || (axis == AX_RY && options[17])) value = -value;
    return value;
}
static void expect(int16_t x, int16_t y, int zone, int32_t want_x, int32_t want_y) {
    int32_t stick[2];
    hos_filter_stick(x, y, zone, stick);
    if (stick[0] != want_x || stick[1] != want_y) {
        fprintf(stderr, "zone %d: (%d, %d) became (%ld, %ld), wanted (%ld, %ld)\n", zone, x, y,
                (long)stick[0], (long)stick[1], (long)want_x, (long)want_y);
        exit(1);
    }
}
int main(void) {
    const int32_t t[6] = {7849, 1638, 3277, 4915, 6553, 9830};
    unsigned scenarios = 0;

    /* The filter alone: every axis value at every threshold keeps the old cutoff on an axis. */
    for (int z = 0; z < 6; z++) {
        for (int32_t i = -32768; i <= 32767; i++) {
            const int32_t a = i < -32767 ? -32767 : i;
            const int32_t want = (a > t[z] || a < -t[z]) ? a : 0;
            expect((int16_t)i, 0, z, want, 0);
            expect(0, (int16_t)i, z, 0, want);
        }
        const int16_t inside = t[z] / 2, diagonal = 3 * t[z] / 4;
        expect(inside, -inside, z, 0, 0);
        expect(diagonal, diagonal, z, diagonal, diagonal);
        expect(-diagonal, diagonal, z, -diagonal, diagonal);
        expect(diagonal, -diagonal, z, diagonal, -diagonal);
        expect(-diagonal, -diagonal, z, -diagonal, -diagonal);
        expect(-32768, -32768, z, -32767, -32767);
        expect(23170, -23170, z, 23170, -23170);
        expect(-32767, 32767, z, -32767, 32767);
    }
    expect(12000, 4000, 0, 12000, 4000);   /* (the smaller component used to be zeroed) */
    expect(2949, 3932, 3, 0, 0);           /* (a 3-4-5 triangle exactly on the radius 4915) */
    expect(2949, 3933, 3, 2949, 3933);
    const int bad[] = {-1, 6, 99, -2147483647 - 1};
    for (int k = 0; k < 4; k++) {
        expect(7849, 0, bad[k], 0, 0);
        expect(7849, 1, bad[k], 7849, 1);
    }

    /* The real SDL conversion and rest/output path, as pads_poll runs it: diagonals, all thresholds, inversion. */
    for (int zone = 0; zone < 6; zone++) {
        for (int axis = AX_RX; axis <= AX_RY; axis++) {
            const int levels[4] = {32767, -32768, t[zone] + 1, -t[zone] - 1};
            for (unsigned level = 0; level < 4; level++) {
                int16_t raw[4] = {0}, neutral[4] = {0};
                int32_t output[4];
                memset(pads, 0, sizeof pads);
                memset(options, 0, sizeof options);
                options[11] = zone;
                raw[axis] = (int16_t)levels[level];
                poll(1, neutral, output);
                for (int n = 0; n < 12; n++) {
                    options[16] = n & 1;
                    options[17] = (n >> 1) & 1;
                    poll(0, raw, output);
                    CHECK(output[axis] == 0);
                    poll(1, raw, output);
                    CHECK(output[axis] == expected(raw[axis], axis));
                }
                raw[axis] = raw[axis] < 0 ? 25000 : -25000;
                poll(0, raw, output);
                CHECK(output[axis] == expected(raw[axis], axis));
                raw[axis] = (int16_t)levels[level];
                poll(0, raw, output);
                CHECK(output[axis] == expected(raw[axis], axis));
                scenarios++;

                memset(pads, 0, sizeof pads);
                poll(0, raw, output);
                CHECK(output[axis] == 0);
                options[axis == AX_RX ? 16 : 17] ^= 1;
                poll(0, raw, output);
                CHECK(output[axis] == 0);
                raw[axis] = (int16_t)t[zone];
                poll(0, raw, output);
                CHECK(output[axis] == 0);
                raw[axis] = (int16_t)levels[level];
                poll(0, raw, output);
                CHECK(output[axis] == expected(raw[axis], axis));
                scenarios++;
            }
        }

        /* A diagonal just outside the circle keeps both components, though each is inside its own threshold. */
        {
            const int16_t diagonal = (int16_t)(3 * t[zone] / 4);
            for (int quadrant = 0; quadrant < 4; quadrant++) {
                int16_t raw[4] = {0};
                int32_t output[4];
                memset(pads, 0, sizeof pads);
                memset(options, 0, sizeof options);
                options[11] = zone;
                poll(0, raw, output);
                raw[0] = (quadrant & 1) ? -diagonal : diagonal;
                raw[1] = (quadrant & 2) ? -diagonal : diagonal;
                poll(0, raw, output);
                CHECK(output[0] == raw[0] && output[1] == -raw[1]);
                scenarios++;
            }
        }

        /* A phantom axis held from connection does not carry its partner's drift past the circle. */
        for (int phantom = 0; phantom < 4; phantom++) {
            const int partner = phantom ^ 1;
            int16_t raw[4] = {0};
            int32_t output[4];
            memset(pads, 0, sizeof pads);
            memset(options, 0, sizeof options);
            options[11] = zone;
            raw[phantom] = 32767;
            raw[partner] = (int16_t)(t[zone] / 2);
            poll(0, raw, output);
            CHECK(output[phantom] == 0 && output[partner] == 0);
            raw[partner] = (int16_t)(t[zone] + 1);
            poll(0, raw, output);
            CHECK(output[phantom] == 0 && output[partner] == expected(raw[partner], partner));
            raw[partner] = (int16_t)(-t[zone] / 2);
            poll(0, raw, output);
            CHECK(output[partner] == 0);
            scenarios++;
        }
    }

    /* The snap #1480 reported: the smaller component is kept, and the Y direction is still SDL's. */
    {
        int16_t raw[4] = {0};
        int32_t output[4];
        memset(pads, 0, sizeof pads);
        memset(options, 0, sizeof options);
        poll(0, raw, output);
        raw[2] = 12000, raw[3] = -4000;
        poll(0, raw, output);
        CHECK(output[2] == 12000 && output[3] == 4000);
        options[16] = 1;
        poll(0, raw, output);
        CHECK(output[2] == -12000 && output[3] == 4000);
        options[17] = 1;
        poll(0, raw, output);
        CHECK(output[2] == -12000 && output[3] == -4000);
    }

    /* Both right axes toggle independently; the left stick keeps its mapping. */
    {
        int16_t raw[4] = {0};
        int32_t output[4];
        memset(pads, 0, sizeof pads);
        memset(options, 0, sizeof options);
        poll(0, raw, output);
        raw[0] = 22000, raw[1] = -24000, raw[2] = -28000, raw[3] = 30000;
        for (int setting = 0; setting < 4; setting++) {
            options[16] = setting & 1;
            options[17] = (setting >> 1) & 1;
            poll(0, raw, output);
            for (int k = 0; k < 4; k++) CHECK(output[k] == expected(raw[k], k));
        }
        scenarios++;
    }

    printf("PASS: %u stick/rest scenarios; circular dead zone, every threshold and inversion\n", scenarios);
    return 0;
}
"#;

#[test]
fn the_stick_dead_zone_keeps_the_direction_and_inversion() {
    let hid = source("host/nn_hid.c");
    let pad = source("host/pad_sdl.c");
    let start = pad
        .find("            const int inv_x = rx_option(16, 0);")
        .expect("pad_sdl.c has no inversion/rest block");
    let end = pad
        .find("            for (int k = 0; k < 4; k++)\n                if ((v[k]")
        .expect("pad_sdl.c's inversion/rest block has no end");
    let pipeline = &pad[start..end];
    assert!(
        pipeline.contains("hos_filter_stick((int16_t)v[0], (int16_t)v[1], z, v)")
            && pipeline.contains("hos_filter_stick((int16_t)v[2], (int16_t)v[3], z, v + 2)"),
        "pad_sdl.c does not re-filter both sticks after rest detection"
    );
    assert!(
        function(&hid, "static void pads_read(")
            .contains("hos_filter_stick(thumb[0], thumb[1], zone, v)")
            && function(&hid, "static void pads_read(")
                .contains("hos_filter_stick(thumb[2], thumb[3], zone, v + 2)"),
        "XInput's pads do not use the circular stick filter"
    );
    assert!(
        function(&pad, "static void motion_only_used(")
            .contains("hos_filter_stick(hos_sdl_pad_axis(pads[i].gc, AX_LX)"),
        "a motion-only pad's stick signature is not filtered as a pair"
    );
    let code = [
        STICK_PRELUDE,
        &function(&hid, "void hos_filter_stick("),
        STICK_POLL_OPEN,
        &pad[start..end],
        STICK_MAIN,
    ]
    .join("\n");
    let Some(run) = run_fixture("stick-dead-zone", &code, &[], &[]) else {
        eprintln!("skipping the stick dead zone fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    assert_passed(run, "the stick dead zone fixture");
}

const SDL_PRELUDE: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <string.h>
typedef void *HMODULE;
typedef void (*FARPROC)(void);
static FARPROC GetProcAddress(HMODULE module, const char *name);
static char logged[4096];
static void hos_log(const char *format, ...) {
    char message[1024];   /* (host/hos_thread.c's real per-message limit) */
    va_list ap;
    va_start(ap, format);
    vsnprintf(message, sizeof message, format, ap);
    va_end(ap);
    strncat(logged, message, sizeof logged - strlen(logged) - 1);
    strncat(logged, "\n", sizeof logged - strlen(logged) - 1);
}
static int version;
#define CALL(type, fn) ((type)f.fn)
"#;

const SDL_FIXTURE: &str = r#"
enum { NO_GUID = 1, NO_MAPPING = 2, NO_FREE = 4, NO_JOYSTICK = 8 };
/* (takes, not "gets": MinGW's stdio.h still declares gets(), and a variable of that name does not compile there) */
static int missing, unknown, takes, frees, wrong_symbols, unexpected_calls, checks, failures;
static char *allocated;
static const char *mapping_text;
static void dummy(void) { unexpected_calls++; }
static const char *pad_name(void *pad) { (void)pad; return "Test controller"; }
static void *joystick(void *pad) { return pad; }
static HosSdlGuid guid_of(void *pad) {
    (void)pad;
    HosSdlGuid guid = {{0}};
    if (!unknown) {
        const unsigned char bytes[16] = {3,0,0,0,0x34,0x12,0,0,0x78,0x56,0,0,1,0,0,0};
        memcpy(guid.data, bytes, sizeof bytes);
    }
    return guid;
}
static char *mapping_of(void *pad) {
    (void)pad;
    takes++;
    if (!mapping_text) return NULL;
    allocated = malloc(strlen(mapping_text) + 1);
    if (allocated) strcpy(allocated, mapping_text);
    return allocated;
}
static void free_mapping(void *ptr) {
    if (ptr != allocated) unexpected_calls++;
    frees++;
    free(ptr);
    allocated = NULL;
}
static FARPROC GetProcAddress(HMODULE module, const char *name) {
    (void)module;
    if (strstr(name, "Serial") || strstr(name, "Path")) unexpected_calls++;
    if (!strcmp(name, "SDL_GetGamepadName") || !strcmp(name, "SDL_GameControllerName")) return (FARPROC)pad_name;
    if (!strcmp(name, "SDL_GetGamepadJoystick") || !strcmp(name, "SDL_GameControllerGetJoystick")) {
        if ((version == 3) != !strcmp(name, "SDL_GetGamepadJoystick")) wrong_symbols++;
        return missing & NO_JOYSTICK ? NULL : (FARPROC)joystick;
    }
    if (!strcmp(name, "SDL_GetJoystickGUID") || !strcmp(name, "SDL_JoystickGetGUID")) {
        if ((version == 3) != !strcmp(name, "SDL_GetJoystickGUID")) wrong_symbols++;
        return missing & NO_GUID ? NULL : (FARPROC)guid_of;
    }
    if (!strcmp(name, "SDL_GetGamepadMapping") || !strcmp(name, "SDL_GameControllerMapping")) {
        if ((version == 3) != !strcmp(name, "SDL_GetGamepadMapping")) wrong_symbols++;
        return missing & NO_MAPPING ? NULL : (FARPROC)mapping_of;
    }
    if (!strcmp(name, "SDL_free")) return missing & NO_FREE ? NULL : (FARPROC)free_mapping;
    return dummy;   /* required unrelated entry points are resolved, never called */
}
#define CHECK(c) do { checks++; if (!(c)) { fprintf(stderr, "FAIL SDL%d line %d: %s\n  %s\n", version, __LINE__, #c, logged); failures++; } } while (0)
static void connect_pad(int motion, int steam) {
    struct { int motion, steam; } pads[1] = {{motion, steam}};
    const int slot = 0;
    void *gc = (void *)(uintptr_t)1;
    ACTUAL_CONNECTION
}
static void load(int absent) {
    memset(&f, 0, sizeof f);
    missing = absent;
    takes = frees = 0;
    logged[0] = 0;
    CHECK(get_all((HMODULE)(uintptr_t)1, version == 3));
}
int main(void) {
    for (version = 2; version <= 3; version++) {
        unknown = 0;
        mapping_text = "03000000341200007856000001000000,Test mapping,a:b1,b:b2,x:b0,y:b3,platform:Windows,";
        load(0);
        connect_pad(1, 1);
        CHECK(strstr(logged, "connected (SDL), with a gyroscope, Steam Input's") != NULL);
        CHECK(strstr(logged, "guid=03000000341200007856000001000000") != NULL);
        CHECK(strstr(logged, "mapping=available; mapping-name=\"Test mapping\"") != NULL);
        CHECK(strstr(logged, mapping_text) != NULL);
        CHECK(takes == 1 && frees == 1 && !allocated);
        printf("SDL%d known: %s\n", version, logged);

        unknown = 1;
        mapping_text = NULL;
        load(0);
        connect_pad(0, 0);
        CHECK(strstr(logged, "guid=unknown; mapping=none; mapping-name=\"?\"") != NULL);
        CHECK(!strstr(logged, "with a gyroscope") && !strstr(logged, "Steam Input's"));
        CHECK(takes == 1 && frees == 0);
        printf("SDL%d unknown: %s\n", version, logged);

        load(NO_GUID | NO_MAPPING);
        connect_pad(0, 0);
        CHECK(strstr(logged, "guid=unavailable; mapping=unavailable") != NULL);
        CHECK(takes == 0 && frees == 0);
        printf("SDL%d missing functions: %s\n", version, logged);

        unknown = 0;
        mapping_text = "03000000341200007856000001000000,Other name,a:b0,";
        load(NO_MAPPING);
        connect_pad(0, 0);
        CHECK(strstr(logged, "guid=03000000341200007856000001000000; mapping=unavailable") != NULL);
        CHECK(!takes && !frees);
        load(NO_GUID);
        connect_pad(0, 0);
        CHECK(strstr(logged, "guid=unavailable; mapping=available") != NULL);
        CHECK(takes == 1 && frees == 1);
        if (version == 2) {   /* SDL3 already requires SDL_free to enumerate controllers. */
            load(NO_FREE);
            connect_pad(0, 0);
            CHECK(strstr(logged, "mapping=unavailable") != NULL);
            CHECK(!takes && !frees);   /* never allocate a string that cannot be freed */
        } else {
            load(NO_JOYSTICK);
            connect_pad(0, 0);
            CHECK(strstr(logged, "guid=unavailable; mapping=available") != NULL);
            CHECK(takes == 1 && frees == 1);
        }
        char long_mapping[1400];
        memset(long_mapping, 'z', sizeof long_mapping);
        memcpy(long_mapping, "03000000341200007856000001000000,Long mapping,", 46);
        strcpy(long_mapping + 1300, "END_MUST_NOT_BE_LOGGED");
        mapping_text = long_mapping;
        load(0);
        connect_pad(0, 0);
        CHECK(strstr(logged, "mapping=available") != NULL);
        CHECK(!strstr(logged, "END_MUST_NOT_BE_LOGGED"));
        CHECK(strstr(logged, " (truncated)") != NULL);
        const char *first = strstr(logged, "mapping-string[1/2]=\""), *second = strstr(logged, "mapping-string[2/2]=\"");
        CHECK(first && second);
        if (first && second) {
            first += strlen("mapping-string[1/2]=\"");
            second += strlen("mapping-string[2/2]=\"");
            CHECK(!strncmp(first, long_mapping, 512) && first[512] == '"');
            CHECK(!strncmp(second, long_mapping + 512, 512) && second[512] == '"');
        }
        CHECK(strlen(logged) < 1400);
        CHECK(takes == 1 && frees == 1 && !allocated);
        CHECK(!wrong_symbols && !unexpected_calls);
    }
    printf("%s: %d checks, %d failures; SDL2/SDL3 connection diagnostics\n", failures ? "FAIL" : "PASS", checks, failures);
    return failures != 0;
}
"#;

#[test]
fn the_connection_log_records_the_guid_and_active_mapping() {
    let sdl = source("host/hos_sdl.c");
    let pad = source("host/pad_sdl.c");
    let state = &sdl[sdl.find("static struct {").expect("hos_sdl.c's f table")
        ..sdl.find("} f;").expect("the f table's end") + "} f;".len()];
    let guid = regex::Regex::new(r"typedef struct \{[^}]*\} HosSdlGuid;")
        .expect("guid regex")
        .find(&sdl)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "typedef struct { uint8_t data[16]; } HosSdlGuid;".into());
    let at = pad
        .find("pads[slot].motion_only = 0;")
        .expect("pad_sdl.c's connection site");
    let at = pad[at..].find('\n').expect("the connection line") + at + 1;
    let end = pad[at..].find(';').expect("the connection line's end") + at + 1;
    let connection = pad[at..end].trim();
    assert!(
        connection.starts_with("hos_sdl_pad_log_connection("),
        "the connection site does not call the diagnostic helper: {connection}"
    );
    let code = [
        SDL_PRELUDE.to_string(),
        state.to_string(),
        guid,
        function(&sdl, "static int get_all("),
        function(&sdl, "const char *hos_sdl_pad_name("),
        function(&sdl, "void hos_sdl_pad_log_connection("),
        SDL_FIXTURE.replace("ACTUAL_CONNECTION", connection),
    ]
    .join("\n");
    let Some(run) = run_fixture("sdl-diagnostics", &code, &[], &[]) else {
        eprintln!("skipping the SDL diagnostics fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    assert_passed(run, "the SDL diagnostics fixture");
}
