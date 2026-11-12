// A GPU that stops finishing work ends the game with a report (gpu/vk/vk_replay.cpp gpu_hang_watch_start, upstream
// NotRexed/Hookshot ba6eef70). NVIDIA's Xid 109 (CTX SWITCH TIMEOUT) ended the game's GPU context and no Vulkan call
// came back VK_ERROR_DEVICE_LOST afterwards: device_lost() never ran and the game froze with no report. The watchdog
// reads the timeline once a second and goes to device_lost() when the device reads as lost, or when work sent to the
// queue makes no progress for RX_VK_GPU_HANG_SEC seconds. gpu_hang_watch_start() is cut into a fixture with the
// timeline, the sent value and device_lost() as stubs, and run four ways: a timeline stuck behind what was sent must
// end the process after the limit, a lost device at once, and a timeline that keeps moving and =0 must not. The real
// device_lost() is compiled too, to keep its VK_ERROR_DEVICE_LOST message and take a hang reason.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A C/C++ definition's braces, matched, from its first line.
fn block(text: &str, start: usize) -> String {
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

fn definition(text: &str, marker: &str) -> String {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    block(text, start)
}

fn compiler() -> Option<String> {
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

const FIXTURE: &str = r#"
#define _CRT_SECURE_NO_WARNINGS   // (the Windows CRT marks getenv and sscanf deprecated: an error under -Werror)
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <thread>
typedef int VkResult;
typedef void* VkDevice;
typedef void* VkSemaphore;
const VkResult VK_SUCCESS = 0, VK_ERROR_DEVICE_LOST = -4;
struct { VkDevice device; } dev;
VkSemaphore timeline = (VkSemaphore)1;
std::atomic<uint64_t> timeline_sent{10};
static const char* mode = "";
static std::atomic<uint64_t> now_done{5};
VkResult vkGetSemaphoreCounterValue(VkDevice, VkSemaphore, uint64_t* v) {
    if (!std::strcmp(mode, "lost")) return VK_ERROR_DEVICE_LOST;
    if (!std::strcmp(mode, "moving")) now_done++, timeline_sent = now_done + 5;
    *v = now_done;
    return VK_SUCCESS;
}
void logf(const char* fmt, ...) {
    va_list a;
    va_start(a, fmt);
    std::vprintf(fmt, a);
    va_end(a);
    std::printf("\n");
    std::fflush(stdout);
}
typedef int HANDLE;
static HANDLE GetCurrentThread() { return 0; }
static void SetThreadDescription(HANDLE, const wchar_t*) {}
[[noreturn]] void device_lost(const char* why = nullptr) {
    std::printf("device_lost: %s\n", why ? why : "(VK_ERROR_DEVICE_LOST)");
    std::fflush(stdout);
    std::_Exit(3);
}
/* WATCH */
int main(int argc, char** argv) {
    mode = argc > 1 ? argv[1] : "";
    gpu_hang_watch_start();
    gpu_hang_watch_start();   // (once only)
    std::this_thread::sleep_for(std::chrono::milliseconds(3500));
    std::printf("still running\n");
    return 0;
}
"#;

const REPORT_FIXTURE: &str = r#"
#include <cstdio>
#include <cstdlib>
#include <mutex>
static int reports = 0;
void checkpoints_report() { ++reports; }
[[noreturn]] void replay_fatal(const char* why) {
    std::printf("FATAL: %s (reports %d)\n", why, reports);
    std::fflush(stdout);
    std::_Exit(3);
}
/* DEVICE_LOST */
int main(int argc, char** argv) {
    device_lost(argc > 1 && argv[1][0] ? argv[1] : nullptr);
}
"#;

fn build(cxx: &str, dir: &Path, name: &str, source: &str) -> PathBuf {
    let src = dir.join(format!("{name}.cpp"));
    let exe = dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    });
    fs::write(&src, source).expect("write fixture");
    let build = Command::new(cxx)
        .args(["-std=c++17", "-Wall", "-Wextra", "-Werror"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", src.display());
    exe
}

fn run(exe: &Path, mode: &str, limit: u64) -> (Output, Duration) {
    let started = Instant::now();
    let output = Command::new(exe)
        .arg(mode)
        .env("RX_VK_GPU_HANG_SEC", limit.to_string())
        .output()
        .expect("run the fixture");
    (output, started.elapsed())
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_stuck_gpu_timeline_ends_the_game_with_the_report() {
    let repo = root();
    let replay =
        fs::read_to_string(repo.join("gpu/vk/vk_replay.cpp")).expect("gpu/vk/vk_replay.cpp");
    if !replay.contains("if (presents == 1) hang_watch_start(), gpu_hang_watch_start();") {
        panic!("the GPU hang watch is not started at the first present");
    }
    for site in [
        "note_timeline_sent(pend.signal)",
        "note_timeline_sent(signal.value)",
    ] {
        if !replay.contains(site) {
            panic!("a submission that signals the timeline is not counted as sent: {site}");
        }
    }

    let Some(cxx) = compiler() else {
        eprintln!(
            "skipping the GPU hang watch fixture: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let dir = scratch("gpu-hang-watch");
    let watch = FIXTURE.replace(
        "/* WATCH */",
        &definition(&replay, "void gpu_hang_watch_start() {"),
    );
    let exe = build(&cxx, &dir, "gpu_hang_watch", &watch);

    let (stuck, took) = run(&exe, "stuck", 2);
    let out = text(&stuck);
    assert_eq!(
        stuck.status.code(),
        Some(3),
        "a stuck timeline did not end the process:\n{out}"
    );
    assert!(
        out.contains("device_lost: the GPU hung"),
        "a stuck timeline did not report the hang:\n{out}"
    );
    assert!(
        out.contains("ends the game (RX_VK_GPU_HANG_SEC)"),
        "the hang watch does not log the limit it settled on:\n{out}"
    );
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(6),
        "a stuck timeline ended after {took:?}, not the 2 s limit:\n{out}"
    );

    let (lost, took) = run(&exe, "lost", 30);
    let out = text(&lost);
    assert_eq!(
        lost.status.code(),
        Some(3),
        "a lost device did not end the process:\n{out}"
    );
    assert!(
        out.contains("device_lost: (VK_ERROR_DEVICE_LOST)"),
        "a lost device did not keep the device-lost report:\n{out}"
    );
    assert!(
        took < Duration::from_secs(3),
        "a lost device ended after {took:?}:\n{out}"
    );

    let (moving, _) = run(&exe, "moving", 2);
    let out = text(&moving);
    assert_eq!(
        moving.status.code(),
        Some(0),
        "a moving timeline ended the process:\n{out}"
    );
    assert!(
        !out.contains("device_lost"),
        "a timeline that keeps moving was taken as a hang:\n{out}"
    );
    assert_eq!(
        out.matches("GPU hang watch:").count(),
        1,
        "the hang watch started more than once:\n{out}"
    );

    let (waited, _) = run(&exe, "stuck", 0);
    let out = text(&waited);
    assert_eq!(
        waited.status.code(),
        Some(0),
        "RX_VK_GPU_HANG_SEC=0 ended the process:\n{out}"
    );
    assert!(
        !out.contains("device_lost"),
        "RX_VK_GPU_HANG_SEC=0 ended the process:\n{out}"
    );
    assert!(
        out.contains("only logged"),
        "RX_VK_GPU_HANG_SEC=0 did not say it only logs:\n{out}"
    );

    let report = REPORT_FIXTURE.replace(
        "/* DEVICE_LOST */",
        &definition(&replay, "[[noreturn]] void device_lost(const char* why) {"),
    );
    let exe = build(&cxx, &dir, "device_lost", &report);
    let (default_report, _) = run(&exe, "", 30);
    let out = text(&default_report);
    assert_eq!(
        default_report.status.code(),
        Some(3),
        "device_lost did not end the process:\n{out}"
    );
    assert!(
        out.contains("FATAL: the GPU device was lost (VK_ERROR_DEVICE_LOST)"),
        "the default device-lost report changed:\n{out}"
    );
    assert!(
        out.contains("(reports 1)"),
        "the checkpoint report did not run once:\n{out}"
    );

    let why = "the GPU hung: no submission finished in 2 s";
    let (hang_report, _) = run(&exe, why, 30);
    let out = text(&hang_report);
    assert_eq!(
        hang_report.status.code(),
        Some(3),
        "device_lost did not end the process:\n{out}"
    );
    assert!(
        out.contains(&format!("FATAL: {why} (reports 1)")),
        "device_lost did not pass the hang reason through:\n{out}"
    );

    let _ = fs::remove_dir_all(dir);
    println!(
        "PASS: a stuck GPU timeline or a lost device ends the process with the report; a moving one and =0 do not"
    );
}
