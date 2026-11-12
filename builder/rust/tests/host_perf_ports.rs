// Game-free checks of the host/runtime performance ports: the short-sleep poller cap and the spin before a wait sleeps
// (upstream NotRexed/Hookshot 376d0681), the native zstd stream decode (a2766f9c), the short command buffer's skipped
// dry pass (685133b3), the segment-heap preparation (upstream PR 2895) and the HOS_CHAIN recorder (57ed3aa8). The C and
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
        (
            compiler("CC", &["cc", "gcc", "clang"]),
            "-std=c11",
        )
    } else {
        (
            compiler("CXX", &["c++", "g++", "clang++"]),
            "-std=c++20",
        )
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
    for switch in ["HOS_SHORT_SLEEP_POLLERS", "HOS_SPIN_OTHER_US", "HOS_SHORT_SLEEP_TIMER"] {
        assert!(host.contains(switch), "hos_thread.c no longer reads {switch}");
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
        String::from_utf8_lossy(&out.stdout).trim().parse::<i64>().unwrap()
    };
    let case_other = |threads: &str, serial: &str, spin_us: &str, other_us: &str| {
        let out = run(&exe, &[threads, serial, spin_us, other_us]);
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().parse::<i64>().unwrap()
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
