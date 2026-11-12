// Game-free checks of the host/runtime performance ports: the short-sleep poller cap and the spin before a wait sleeps
// (upstream NotRexed/Hookshot 376d0681), the native zstd stream decode (a2766f9c), the short command buffer's skipped
// dry pass (685133b3) and the HOS_CHAIN recorder (57ed3aa8). The C and
// C++ fixtures are cut out of the sources as they are and compiled with the host compiler; a machine with no compiler
// skips them, as CI's Rust job may have none on Windows.

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

/// A function's definition from its first line, braces matched.
fn definition(text: &str, marker: &str) -> String {
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

fn compiler(env: &str, candidates: &[&str]) -> Option<String> {
    let candidates = std::env::var(env)
        .ok()
        .into_iter()
        .chain(candidates.iter().map(|c| c.to_string()));
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

fn compile(name: &str, source_text: &str, ext: &str, flags: &[&str]) -> Option<(PathBuf, PathBuf)> {
    let (cc, std) = if ext == "c" {
        (compiler("CC", &["cc", "gcc", "clang"]), "-std=c11")
    } else {
        (compiler("CXX", &["c++", "g++", "clang++"]), "-std=c++20")
    };
    let cc = cc?;
    let dir = scratch(name);
    let file = dir.join(format!("fixture.{ext}"));
    fs::write(&file, source_text).expect("write the fixture");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut build = Command::new(&cc);
    build.args([std, "-Wall", "-Wextra", "-Werror"]);
    for flag in flags {
        build.arg(flag);
    }
    build.arg(&file).arg("-o").arg(&exe);
    let built = build.output().expect("run the compiler");
    assert!(
        built.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    Some((dir, exe))
}

fn run(exe: &Path, args: &[&str]) -> Output {
    Command::new(exe)
        .args(args)
        .output()
        .expect("run the fixture")
}

/// A C array initializer for a byte string.
fn c_bytes(bytes: &[u8]) -> String {
    let mut out = String::from("{");
    for (i, b) in bytes.iter().enumerate() {
        if i % 16 == 0 {
            out.push_str("\n    ");
        }
        out.push_str(&format!("0x{b:02x},"));
    }
    out.push_str("\n}");
    out
}

#[test]
fn short_sleep_pollers_follow_the_processor_count_and_the_switch() {
    let host = source("host/hos_thread.c");
    let threads = host
        .find("enum { SHORT_SLEEP_TIMER_THREADS = 6 };")
        .expect("the thread threshold is gone");
    let threads = &host[threads..threads + "enum { SHORT_SLEEP_TIMER_THREADS = 6 };".len()];
    let timer = definition(&host, "static int hos_short_sleep_timer(");
    let pollers = definition(&host, "static int hos_short_sleep_pollers(");
    let serial = definition(&host, "static int serial_name(");
    let fixture = format!(
        r#"#ifndef _WIN32
#define _GNU_SOURCE   /* (setenv and unsetenv) */
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "hookshot_env.h"
#define CHECK(x) do {{ if (!(x)) {{ fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); return 1; }} }} while (0)
{threads}
{timer}
{pollers}
{serial}
#if defined(_WIN32)
static void put(const char *name, const char *value) {{ _putenv_s(name, value ? value : ""); }}
#else
static void put(const char *name, const char *value) {{ if (value) setenv(name, value, 1); else unsetenv(name); }}
#endif
int main(void) {{
    put("HOS_SHORT_SLEEP_TIMER", NULL);
    put("HOS_SHORT_SLEEP_POLLERS", NULL);
    /* (the timer: <= 6 logical processors use it for the game's sleeps, more yield; set: 2 or 0) */
    CHECK(hos_short_sleep_timer(NULL, 4) == 1);
    CHECK(hos_short_sleep_timer(NULL, 6) == 1);
    CHECK(hos_short_sleep_timer(NULL, 7) == 0);
    CHECK(hos_short_sleep_timer(NULL, 16) == 0);
    CHECK(hos_short_sleep_timer(NULL, 0) == 0);
    CHECK(hos_short_sleep_timer("1", 4) == 2);
    CHECK(hos_short_sleep_timer("0", 16) == 0);
    CHECK(hos_short_sleep_timer("false", 16) == 0);
    /* (the poller cap: a quarter of the processors above six, none at six or fewer) */
    CHECK(hos_short_sleep_pollers(NULL, 6) == 0);
    CHECK(hos_short_sleep_pollers(NULL, 7) == 1);
    CHECK(hos_short_sleep_pollers(NULL, 8) == 2);
    CHECK(hos_short_sleep_pollers(NULL, 16) == 4);
    CHECK(hos_short_sleep_pollers(NULL, 0) == 0);
    put("HOS_SHORT_SLEEP_POLLERS", "3");
    CHECK(hos_short_sleep_pollers("3", 16) == 3);
    put("HOS_SHORT_SLEEP_POLLERS", "0");
    CHECK(hos_short_sleep_pollers("0", 16) == 0);
    put("HOS_SHORT_SLEEP_POLLERS", "9999");
    CHECK(hos_short_sleep_pollers("9999", 16) == 1024);
    put("HOS_SHORT_SLEEP_POLLERS", "-2");
    CHECK(hos_short_sleep_pollers("-2", 16) == 0);
    put("HOS_SHORT_SLEEP_POLLERS", "abc");
    CHECK(hos_short_sleep_pollers("abc", 16) == 4);
    CHECK(hos_short_sleep_pollers("", 16) == 4);
    /* (the frame's serial threads by the game's names) */
    CHECK(serial_name("sead::MainThread"));
    CHECK(serial_name("Presentation Thread"));
    CHECK(!serial_name("ModuleSystemWorker"));
    CHECK(!serial_name(""));
    puts("ok: short-sleep thresholds");
    return 0;
}}
"#
    );
    let Some((dir, exe)) = compile(
        "short-sleep-pollers",
        &fixture,
        "c",
        &["-I", &root().join("gpu/vk/compat").display().to_string()],
    ) else {
        eprintln!("skipping the short-sleep fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let out = run(&exe, &[]);
    let _ = fs::remove_dir_all(dir);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok: short-sleep thresholds"));

    // (both spins ask the role's length, and the switch names are still there)
    let svc = source("host/hos_svc.c");
    assert!(
        svc.contains("const int64_t limit = hos_spin_ns();"),
        "hos_svc.c no longer asks hos_spin_ns"
    );
    let sync = source("host/nn_os_sync.c");
    assert!(
        sync.contains("hos_spin_ns()") && !sync.contains("static int64_t spin_ns(void)"),
        "nn_os_sync.c still has its own spin length"
    );
    for switch in [
        "HOS_SHORT_SLEEP_POLLERS",
        "HOS_SPIN_OTHER_US",
        "HOS_SHORT_SLEEP_TIMER",
    ] {
        assert!(
            host.contains(switch),
            "hos_thread.c no longer reads {switch}"
        );
    }
}

#[test]
fn the_spin_before_a_wait_is_shorter_for_threads_off_the_frame_path() {
    let host = source("host/hos_thread.c");
    let threads = host
        .find("enum { SHORT_SLEEP_TIMER_THREADS = 6 };")
        .expect("the thread threshold is gone");
    let threads = &host[threads..threads + "enum { SHORT_SLEEP_TIMER_THREADS = 6 };".len()];
    let spin = definition(&host, "int64_t hos_spin_ns(void) {");
    let fixture = format!(
        r#"#ifndef _WIN32
#define _GNU_SOURCE   /* (setenv and unsetenv) */
#endif
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
{threads}
typedef struct HosThread HosThread;
static int fake_threads, fake_serial;
static int process_threads(void) {{ return fake_threads; }}
static HosThread *hos_self(void) {{ return (HosThread *)1; }}
static int hos_thread_serial(const HosThread *t) {{ (void)t; return fake_serial; }}
{spin}
#if defined(_WIN32)
static void put(const char *name, const char *value) {{ _putenv_s(name, value ? value : ""); }}
#else
static void put(const char *name, const char *value) {{ if (value) setenv(name, value, 1); else unsetenv(name); }}
#endif
int main(int argc, char **argv) {{
    if (argc != 4 && argc != 5) return 2;
    fake_threads = atoi(argv[1]);
    fake_serial = atoi(argv[2]);
    put("HOS_SPIN_US", NULL);
    put("HOS_SPIN_OTHER_US", NULL);
    if (strcmp(argv[3], "-")) put("HOS_SPIN_US", argv[3]);
    if (argc == 5 && strcmp(argv[4], "-")) put("HOS_SPIN_OTHER_US", argv[4]);
    printf("%lld\n", (long long)hos_spin_ns());
    return 0;
}}
"#
    );
    let Some((dir, exe)) = compile("spin-by-role", &fixture, "c", &[]) else {
        eprintln!("skipping the spin fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let case = |threads: &str, serial: &str, spin_us: &str| {
        let out = run(&exe, &[threads, serial, spin_us]);
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<i64>()
            .unwrap()
    };
    let case_other = |threads: &str, serial: &str, spin_us: &str, other_us: &str| {
        let out = run(&exe, &[threads, serial, spin_us, other_us]);
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<i64>()
            .unwrap()
    };
    // (16 processors: the serial threads keep 20 us, the others spin 5 us; 6 or fewer: one length for all)
    assert_eq!(case("16", "0", "-"), 5_000);
    assert_eq!(case("16", "1", "-"), 20_000);
    assert_eq!(case("6", "0", "-"), 20_000);
    assert_eq!(case("0", "0", "-"), 20_000);
    assert_eq!(case("16", "1", "7"), 7_000);
    assert_eq!(case("16", "1", "0"), 0);
    // (clamped, and never longer for the others than the serial threads')
    assert_eq!(case("16", "1", "2000000"), 1_000_000_000);
    assert_eq!(case("16", "0", "5"), 5_000);
    // (HOS_SPIN_OTHER_US asks for one length for the others, never above HOS_SPIN_US)
    assert_eq!(case_other("16", "0", "20", "1"), 1_000);
    assert_eq!(case_other("16", "0", "5", "20"), 5_000);
    assert_eq!(case_other("6", "0", "20", "1"), 1_000);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_native_zstd_stream_round_trips_and_stays_off_unless_asked() {
    let host = source("host/hos_zstd.c");
    assert!(
        host.contains("HOS_NATIVE_ZSTD") && host.contains("hos_vcore_away"),
        "host/hos_zstd.c no longer has the switch or the core give-up"
    );
    let plain: Vec<u8> = (0..96 * 1024)
        .map(|i| {
            let w = (i / 7) % 29;
            b"the quick brown fox jumps over the lazy dog. "[w % 45] ^ (i % 11) as u8
        })
        .collect();
    let packed = zstd::bulk::compress(&plain, 3).expect("make a zstd frame");
    assert!(
        packed.len() > 32 && packed.len() < plain.len(),
        "the fixture frame is not compressed"
    );
    let plain_c = c_bytes(&plain);
    let packed_c = c_bytes(&packed);
    let fixture = format!(
        r#"#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "hos.h"
#define SIZE 0x400000ull
uint8_t *zs_mem;
static int aways, backs, marks;
void zs_mark(const void *p, size_t n) {{
    const size_t a = (size_t)((const uint8_t *)p - zs_mem);
    if (n && a < SIZE) marks++;
}}
int hos_vcore_away(void) {{ aways++; return 1; }}
void hos_vcore_back(void) {{ backs++; }}
void hos_log(const char *fmt, ...) {{ (void)fmt; }}
#include "hos_zstd.c"
static const unsigned char plain[] = {plain_c};
static const unsigned char packed[] = {packed_c};
enum {{ PLAIN_LEN = sizeof plain, PACKED_LEN = sizeof packed }};
#define S0 (ZS_BASE + 0x100ull)
#define WORK (ZS_BASE + 0x2000ull)
#define STAGE (ZS_BASE + 0x4000ull)
#define STAGE_CAP 0x10000u
#define SRC (ZS_BASE + 0x90000ull)
#define DST (ZS_BASE + 0x200000ull)
#define DST_CAP 0x40000u
#if defined(_WIN32)
static void put(const char *name, const char *value) {{ _putenv_s(name, value ? value : ""); }}
#else
static void put(const char *name, const char *value) {{ if (value) setenv(name, value, 1); else unsetenv(name); }}
#endif
static void reset(uint64_t g, uint64_t dst, uint32_t room, uint64_t work) {{
    memset(GP(g), 0, 0x60);
    S_STATE(g) = 1;
    S_DST(g) = dst;
    S_ROOM(g) = room;
    S_LEFT(g) = 0;
    S_WORK(g) = work;
    S_STAGE(g) = STAGE;
    S_STAGE_CAP(g) = STAGE_CAP;
    S_STAGED(g) = 0;
    S_CTX(g) = 0;
}}
static int32_t call(uint64_t g, const unsigned char *p, uint32_t n) {{
    memcpy(GP(SRC), p, n);
    Ctx c;
    memset(&c, 0, sizeof c);
    c.x[0] = g, c.x[1] = SRC, c.x[2] = n, c.x[3] = 0;
    if (!hos_zstd_stream(&c)) return -1000;
    return (int32_t)(uint32_t)c.x[0];
}}
static int round_trip(int split) {{
    reset(S0, DST, DST_CAP, WORK);
    uint32_t at = 0, first = split ? 18u : (uint32_t)PACKED_LEN;
    if (first > PACKED_LEN) first = PACKED_LEN;
    int32_t r = call(S0, packed, first);
    at += first;
    if (r < 0) {{ fprintf(stderr, "first call: %d\n", r); return 10; }}
    while (at < PACKED_LEN) {{
        uint32_t n = split ? 7u : (uint32_t)PACKED_LEN - at;
        if (n > PACKED_LEN - at) n = (uint32_t)PACKED_LEN - at;
        r = call(S0, packed + at, n);
        at += n;
        const int32_t left = (int32_t)((uint64_t)PLAIN_LEN - (S_DST(S0) - DST));
        if (r != left) {{ fprintf(stderr, "call at %u: %d, left %d\n", at, r, left); return 11; }}
    }}
    if (S_STATE(S0) != 4 || S_LEFT(S0) != 0 || S_STAGED(S0) != 0 || S_CTX(S0) != WORK) {{
        fprintf(stderr, "end: state %u left %u staged %u ctx %llu\n", S_STATE(S0), S_LEFT(S0), S_STAGED(S0),
                (unsigned long long)S_CTX(S0));
        return 12;
    }}
    if (S_DST(S0) != DST + PLAIN_LEN || S_ROOM(S0) != DST_CAP - PLAIN_LEN) {{
        fprintf(stderr, "output: dst %llu room %u\n", (unsigned long long)S_DST(S0), S_ROOM(S0));
        return 13;
    }}
    if (memcmp(GP(DST), plain, PLAIN_LEN)) {{ fprintf(stderr, "the output differs\n"); return 14; }}
    return 0;
}}
int main(int argc, char **argv) {{
    if (argc != 2) return 2;
    zs_mem = calloc(1, SIZE);
    if (!zs_mem) return 3;
    const int mode = atoi(argv[1]);
    if (mode == 0) {{
        put("HOS_NATIVE_ZSTD", "0");
        reset(S0, DST, DST_CAP, WORK);
        if (call(S0, packed, PACKED_LEN) != -1000) return 20;
        if (S_STATE(S0) != 1 || S_DST(S0) != DST || aways || backs || marks) return 21;
        puts("ok: off");
        return 0;
    }}
    if (mode == 3) {{   /* (the switch off: an unaligned workspace falls back to the lifted code) */
        put("HOS_NATIVE_ZSTD", "1");
        reset(S0, DST, DST_CAP, WORK + 1);
        if (call(S0, packed, PACKED_LEN) != -1000) return 30;
        if (S_STATE(S0) != 1 || S_DST(S0) != DST) return 31;
        puts("ok: unaligned");
        return 0;
    }}
    if (mode == 4) {{   /* (a bad magic: the game's wrapper fails the stream) */
        put("HOS_NATIVE_ZSTD", "2");
        unsigned char bad[64];
        memcpy(bad, packed, sizeof bad);
        bad[0] ^= 0xff;
        reset(S0, DST, DST_CAP, WORK);
        const int32_t r = call(S0, bad, sizeof bad);
        if (r != -1 || S_STATE(S0) != 3) {{ fprintf(stderr, "bad: %d state %u\n", r, S_STATE(S0)); return 40; }}
        puts("ok: bad frame");
        return 0;
    }}
    put("HOS_NATIVE_ZSTD", mode == 1 ? "1" : "2");
    if (round_trip(1)) return 50;
    if (mode == 1 && (aways == 0 || aways != backs)) return 51;
    if (mode == 2 && (aways || backs)) return 52;
    if (!marks) return 53;
    /* (a second frame on the same stream object, whole) */
    reset(S0, DST, DST_CAP, WORK);
    const int32_t r = call(S0, packed, PACKED_LEN);
    if (r != 0 || S_STATE(S0) != 4 || memcmp(GP(DST), plain, PLAIN_LEN)) return 54;
    printf("ok: mode %d, %d calls gave the core up\n", mode, aways);
    return 0;
}}
"#
    );
    let dir = scratch("native-zstd");
    fs::write(
        dir.join("hos.h"),
        r#"#pragma once
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdlib.h>
typedef struct { uint64_t x[32]; } Ctx;
#define ZS_BASE 0x10000000ull
extern uint8_t *zs_mem;
#define GP(a) ((void *)(zs_mem + ((uint64_t)(a) - ZS_BASE)))
#define G64(a) (*(uint64_t *)GP(a))
#define G32(a) (*(uint32_t *)GP(a))
void zs_mark(const void *p, size_t n);
#define HOS_WRITTEN(p, n) zs_mark((p), (n))
int hos_vcore_away(void);
void hos_vcore_back(void);
void hos_log(const char *fmt, ...);
"#,
    )
    .expect("write the zstd stand-in hos.h");
    fs::write(dir.join("fixture.c"), fixture).expect("write the zstd fixture");
    fs::copy(root().join("host/hos_zstd.c"), dir.join("hos_zstd.c")).expect("copy hos_zstd.c");
    let Some(cc) = compiler("CC", &["cc", "gcc", "clang"]) else {
        eprintln!("skipping the native zstd fixture: no C compiler (CC, cc, gcc or clang)");
        let _ = fs::remove_dir_all(dir);
        return;
    };
    let zstd = root().join("third_party/zstd/lib");
    let mut sources: Vec<PathBuf> = [
        "common/debug.c",
        "common/entropy_common.c",
        "common/error_private.c",
        "common/fse_decompress.c",
        "common/xxhash.c",
        "common/zstd_common.c",
        "decompress/huf_decompress.c",
        "decompress/zstd_ddict.c",
        "decompress/zstd_decompress.c",
        "decompress/zstd_decompress_block.c",
    ]
    .iter()
    .map(|f| zstd.join(f))
    .collect();
    sources.push(dir.join("fixture.c"));
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut build = Command::new(&cc);
    build
        .args([
            "-std=c11",
            "-O1",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-DZSTD_DISABLE_ASM",
            "-D_GNU_SOURCE",
        ])
        .arg(format!("-I{}", dir.display()))
        .arg(format!("-I{}", zstd.display()))
        .arg(format!(
            "-include{}",
            root().join("gpu/vk/compat/hookshot_env.h").display()
        ))
        .args(&sources)
        .arg("-o")
        .arg(&exe);
    let built = build.output().expect("run the C compiler");
    assert!(
        built.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    for (mode, want) in [
        (0, "ok: off"),
        (1, "ok: mode 1"),
        (2, "ok: mode 2"),
        (3, "ok: unaligned"),
        (4, "ok: bad frame"),
    ] {
        let out = run(&exe, &[&mode.to_string()]);
        assert!(
            out.status.success(),
            "mode {mode}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(want),
            "mode {mode}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_short_command_buffer_skips_the_dry_pass() {
    let replay = source("gpu/vk/vk_replay.cpp");
    let header = source("gpu/render/rx_nvn.h");
    let ops_at = header
        .find("typedef struct { uint32_t op, size; } nvn_op_hdr;")
        .expect("nvn_op_hdr is gone");
    let ops_end = header[ops_at..]
        .find("typedef struct { int32_t n;")
        .expect("the command op enum's end is gone")
        + ops_at;
    let ops = &header[ops_at..ops_end];
    let work = definition(&replay, "bool op_is_work(uint32_t op) {");
    let count = definition(
        &replay,
        "int64_t stream_work(const uint8_t* p, int64_t most) {",
    );
    // (the dry pass counts with the same op test, and the short decision comes before it)
    let handle_at = replay
        .find("const uint8_t* replay_handle(")
        .expect("replay_handle is gone");
    let handle = &replay[handle_at..];
    let short_at = handle
        .find("stream_work(h, 2 * per) < 2 * per")
        .expect("a short command buffer is not counted before the dry pass");
    let dry_at = handle
        .find("dry_marks_begin();")
        .expect("the dry pass is gone");
    assert!(
        short_at < dry_at,
        "the dry pass still runs before the short command buffer is decided"
    );
    let short = &handle[short_at..dry_at];
    for part in [
        "replay(cmd, state, h);",
        "split_off = true;",
        "split_off = was_split;",
    ] {
        assert!(
            short.contains(part),
            "the short command buffer's recording misses {part}"
        );
    }
    assert!(
        replay.contains("env_on(\"RX_VK_NO_DRY_SHORT\")"),
        "RX_VK_NO_DRY_SHORT is gone"
    );
    let split = definition(&replay, "void maybe_split(VkCommandBuffer& cmd) {");
    assert!(
        split.contains("if (split_off ||"),
        "maybe_split no longer leaves a short command buffer whole"
    );
    let dry = &replay[replay
        .find("if (op_is_work(h.op)) {")
        .expect("the dry pass's op test is gone")..];
    assert!(
        dry[..dry
            .find("p += h.size;")
            .expect("the dry pass's end is gone")]
            .contains("++dry_work"),
        "the dry pass no longer counts its work with op_is_work"
    );

    let fixture = format!(
        r#"#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
{ops}
#define CHECK(x) do {{ if (!(x)) {{ std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); }} }} while (0)
{work}
{count}
static uint8_t s[4096];
static size_t at;
static void op(uint32_t o, uint32_t payload = 0) {{
    const nvn_op_hdr h{{o, 8 + payload}};
    std::memcpy(s + at, &h, sizeof h);
    at += sizeof h + payload;
}}
int main() {{
    op(NOP_DRAW), op(NOP_POLY_OFFSET, 24), op(NOP_DRAW_IDX), op(NOP_DRAW_INDIRECT), op(NOP_DRAW_MESH, 16);
    op(NOP_STENCIL, 8), op(NOP_DISPATCH), op(NOP_DISPATCH_INDIRECT), op(NOP_BARRIER);
    op(NOP_END), op(NOP_DRAW), op(NOP_DRAW);
    CHECK(stream_work(s, 1000) == 6);   // (ops past the end are not counted)
    CHECK(stream_work(s, 4) == 4);      // (enough: stops there)
    CHECK(stream_work(s, 6) == 6);
    CHECK(stream_work(s, 7) == 6);
    at = 0;
    op(NOP_POLY_OFFSET, 4), op(NOP_END);
    CHECK(stream_work(s, 2) == 0);      // (state alone: no segments)
    std::puts("PASS: a short command buffer is counted without the dry pass");
    return 0;
}}
"#
    );
    let Some((dir, exe)) = compile("parallel-small-cb", &fixture, "cpp", &[]) else {
        eprintln!(
            "skipping the short command buffer fixture: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let out = run(&exe, &[]);
    let _ = fs::remove_dir_all(dir);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("PASS: a short command buffer"));
}
#[test]
fn the_chain_recorder_stays_off_until_asked_and_writes_its_records() {
    if cfg!(windows) {
        // (the fixture starts the recorder's thread with pthreads; the Windows recorder is plain Win32 and builds there)
        return;
    }
    let fixture = r#"#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <windows.h>
#include "hos.h"
static Ctx ctx;
static int64_t now;
static int logs;
HosThread *hos_self(void) { return (HosThread *)1; }
Ctx *hos_ctx(void) { return &ctx; }
const char *hos_thread_name(HosThread *t) { (void)t; return "MainThread"; }
int64_t hos_now_ns(void) { return now += 1000; }
void hos_log(const char *fmt, ...) { (void)fmt; logs++; }
typedef struct { LPTHREAD_START_ROUTINE fn; void *arg; } Start;
static void *start_run(void *p) {
    Start *s = (Start *)p;
    s->fn(s->arg);
    free(s);
    return NULL;
}
HANDLE CreateThread(void *attr, SIZE_T stack, LPTHREAD_START_ROUTINE fn, void *arg, DWORD flags, DWORD *id) {
    (void)attr; (void)stack; (void)flags; (void)id;
    Start *s = (Start *)malloc(sizeof *s);
    s->fn = fn, s->arg = arg;
    pthread_t t;
    if (pthread_create(&t, NULL, start_run, s)) return NULL;
    pthread_detach(t);
    return (HANDLE)1;
}
void Sleep(DWORD ms) { usleep((useconds_t)ms * 1000); }
#include "hos_chain.c"
typedef struct { int64_t t; uint64_t key, lr; uint16_t thread; uint8_t kind; uint8_t pad[5]; } Rec;
static int wait_for(int (*done)(const char *), const char *path) {
    for (int i = 0; i < 200; i++) {
        if (done(path)) return 1;
        Sleep(10);
    }
    return 0;
}
static int on(const char *path) { (void)path; return hos_chain_on; }
static int exists(const char *path) { return access(path, F_OK) == 0; }
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char go[1024], bin[1024];
    snprintf(go, sizeof go, "%s.go", argv[1]);
    snprintf(bin, sizeof bin, "%s", argv[1]);
    remove(go), remove(bin);
    /* (off: a note does nothing) */
    hos_chain_on = 0;
    hos_chain_note(HOS_CHAIN_WAIT, 0xabc, 0);
    if (nrecs != 0) return 10;
    /* (the switch starts the recorder only while <file>.go exists) */
    setenv("HOS_CHAIN", bin, 1);
    FILE *g = fopen(go, "wb");
    if (!g) return 11;
    fclose(g);
    hos_chain_start();
    if (!wait_for(on, bin)) return 12;
    ctx.x[30] = 0x1234;
    hos_chain_note(HOS_CHAIN_WAIT, 0xabc, 0);
    hos_chain_note(HOS_CHAIN_PRESENT, 7, 0x55);
    if (nrecs != 2) return 13;
    remove(go);
    if (!wait_for(exists, bin)) return 14;
    /* (the file's layout: header, threads, count, then 32-byte records) */
    FILE *f = fopen(bin, "rb");
    if (!f) return 15;
    char line[256];
    if (!fgets(line, sizeof line, f) || strcmp(line, "HOSCHAIN1\n")) return 16;
    if (!fgets(line, sizeof line, f) || !strstr(line, "thread 0 MainThread")) return 17;
    if (!fgets(line, sizeof line, f) || !strstr(line, "records 2")) return 18;
    Rec r[2];
    if (fread(r, sizeof r[0], 2, f) != 2) return 19;
    fclose(f);
    if (r[0].kind != HOS_CHAIN_WAIT || r[0].key != 0xabc || r[0].lr != 0x1234 || r[0].thread != 0) return 20;
    if (r[1].kind != HOS_CHAIN_PRESENT || r[1].key != 7 || r[1].lr != 0x55) return 21;
    if (r[1].t <= r[0].t) return 22;
    /* (off again: no more records) */
    hos_chain_on = 0;
    hos_chain_note(HOS_CHAIN_SIGNAL, 9, 0);
    if (nrecs != 2) return 23;
    if (!logs) return 24;
    puts("PASS: HOS_CHAIN records only while its .go file exists");
    return 0;
}
"#;
    let dir = scratch("chain");
    fs::write(
        dir.join("hos.h"),
        r#"#pragma once
#include <stdint.h>
typedef struct HosThread HosThread;
typedef struct { uint64_t x[32]; } Ctx;
enum { HOS_CHAIN_WAIT = 1, HOS_CHAIN_WOKE, HOS_CHAIN_SIGNAL, HOS_CHAIN_SLEEP, HOS_CHAIN_AWAKE, HOS_CHAIN_PRESENT };
extern volatile int hos_chain_on;
void hos_chain_note(int kind, uint64_t key, uint64_t lr);
HosThread *hos_self(void);
Ctx *hos_ctx(void);
const char *hos_thread_name(HosThread *t);
int64_t hos_now_ns(void);
void hos_log(const char *fmt, ...);
"#,
    )
    .expect("write the chain stand-in hos.h");
    fs::write(dir.join("fixture.c"), fixture).expect("write the chain fixture");
    fs::copy(root().join("host/hos_chain.c"), dir.join("hos_chain.c")).expect("copy hos_chain.c");
    let Some(cc) = compiler("CC", &["cc", "gcc", "clang"]) else {
        eprintln!("skipping the HOS_CHAIN fixture: no C compiler (CC, cc, gcc or clang)");
        let _ = fs::remove_dir_all(dir);
        return;
    };
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let build = Command::new(&cc)
        .args([
            "-std=c11",
            "-O1",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-D_GNU_SOURCE",
            "-pthread",
        ])
        .arg(format!("-I{}", dir.display()))
        .arg(format!("-I{}", root().join("host/posix").display()))
        .arg(dir.join("fixture.c"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let path = dir.join("chain.bin");
    let out = run(&exe, &[path.to_str().unwrap()]);
    let _ = fs::remove_dir_all(dir);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("PASS: HOS_CHAIN"));

    // (the recorder's notes are wired into the waits, signals, sleeps and presents, and the switch is in the allowed list)
    let hos = source("host/hos.h");
    assert!(
        hos.contains("HOS_CHAIN_WAIT = 1, HOS_CHAIN_WOKE, HOS_CHAIN_SIGNAL, HOS_CHAIN_SLEEP, HOS_CHAIN_AWAKE, HOS_CHAIN_PRESENT"),
        "the HOS_CHAIN kinds changed"
    );
    for (file, needles) in [
        ("host/hos_main.c", vec!["HOS_CHAIN", "hos_chain_start"]),
        (
            "host/nn_os_sync.c",
            vec!["HOS_CHAIN(HOS_CHAIN_WAIT", "HOS_CHAIN(HOS_CHAIN_SIGNAL"],
        ),
        (
            "host/hos_thread.c",
            vec![
                "wake_chain_key",
                "HOS_CHAIN(HOS_CHAIN_WAIT",
                "HOS_CHAIN(HOS_CHAIN_SLEEP",
            ],
        ),
        ("host/nvn_native.c", vec!["HOS_CHAIN(HOS_CHAIN_PRESENT"]),
    ] {
        let text = source(file);
        for needle in needles {
            assert!(text.contains(needle), "{file} misses {needle}");
        }
    }
    assert!(
        root().join("scripts/chain.py").is_file(),
        "scripts/chain.py is missing"
    );
}

#[test]
fn the_presenter_queue_is_a_second_graphics_or_a_free_compute_queue() {
    let Some(cc) = compiler("CXX", &["c++", "g++", "clang++"]) else {
        eprintln!(
            "skipping the presenter queue fixture: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let dir = scratch("presenter-queue");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let build = Command::new(&cc)
        .args(["-std=c++17", "-Wall", "-Wextra", "-Werror"])
        .arg(root().join("tests/presenter_queue_test.cpp"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let out = run(&exe, &[]);
    let _ = fs::remove_dir_all(dir);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("PASS: the presenter's queue"));

    // (the thread is opt-in, and its queue is only made when the switch asks for it)
    let main = source("gpu/vk/vk_main.cpp");
    assert!(
        main.contains("const bool presenter_wanted = pt_env && atoi(pt_env) != 0;"),
        "RX_VK_PRESENT_THREAD is no longer off by default in create_device"
    );
    assert!(
        main.contains("presenter_queue_pick(")
            && main.contains("if (dev.presenter_supported) share(dev.presenter_family);"),
        "the presenter's queue or the swapchain's sharing is gone"
    );
    let replay = source("gpu/vk/vk_replay.cpp");
    let on = definition(&replay, "bool present_thread_on() {");
    assert!(
        on.contains("return e && atoi(e) != 0;"),
        "RX_VK_PRESENT_THREAD is no longer opt-in in present_thread_on"
    );
    let loop_body = definition(&replay, "void presenter_loop() {");
    assert!(
        loop_body.contains("vkQueuePresentKHR(dev.presenter_queue, &pi)")
            && loop_body.contains("presenter_wait_copy(done)")
            && loop_body.contains("wait_for_present(dev.device, sc, id, 20000000ull)"),
        "the presenter no longer waits for the copy and presents on its own queue with a limit"
    );
    let hand = definition(
        &replay,
        "void present_hand_off(uint32_t index, VkFence done) {",
    );
    assert!(
        hand.find("presenter_wait_idle(lk);") < hand.find("presenter.busy = true"),
        "a present is handed over before the one before is made"
    );
    let drain = definition(&replay, "void present_thread_drain() {");
    assert!(
        drain.contains("presenter_wait_idle(lk, 5000)"),
        "present_thread_drain no longer waits with a limit"
    );
}
