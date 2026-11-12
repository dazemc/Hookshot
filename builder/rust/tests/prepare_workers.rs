// The shader preparation's worker pool with the segment heap (gpu/vk/prepare_workers.h, vk_draw.cpp prepare_run,
// vk_progcache.cpp, nx2/win_manifest.py; upstream NotRexed/Hookshot abacc8ea, #2895). Game-free: the real header is
// compiled and its counting checked; the manifest and prepare_run are read. No game, GPU or Windows is needed.

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

const FIXTURE: &str = r#"
#include "prepare_workers.h"
#include <cassert>
#include <cstring>
using prepare_workers::count;
using prepare_workers::kMaxWorkers;
using prepare_workers::kPerWorkerMB;
using prepare_workers::kReserveMB;
int main() {
    // (the NT heap: half the CPUs, whatever the memory)
    assert(count(6, 12, 64 * 1024, false) == 6);
    assert(count(6, 12, 0, false) == 6);
    assert(count(0, 1, 64 * 1024, false) == 1);
    assert(count(40, 80, 1 << 20, false) == kMaxWorkers);
    // (the segment heap: every usable CPU when the memory allows it)
    assert(count(6, 12, 10 * 1024, true) == 12);
    assert(count(6, 12, kReserveMB, true) == 6);
    assert(count(6, 12, kReserveMB - 1, true) == 6);
    assert(count(6, 12, kReserveMB + kPerWorkerMB - 1, true) == 6);
    assert(count(6, 12, kReserveMB + kPerWorkerMB, true) == 7);
    assert(count(6, 12, kReserveMB + 3 * kPerWorkerMB, true) == 9);
    assert(count(6, 12, 1 << 30, true) == 12);
    assert(count(6, 6, 1 << 30, true) == 6);
    assert(count(16, 64, 1 << 30, true) == kMaxWorkers);
    unsigned char heap[32];
    std::memset(heap, 0, sizeof heap);
    const unsigned char segment[4] = {0xEE, 0xDD, 0xEE, 0xDD}, nt[4] = {0xEE, 0xFF, 0xEE, 0xFF};
    std::memcpy(heap + 16, segment, 4);
    assert(prepare_workers::segment_heap_signature(heap));
    std::memcpy(heap + 16, nt, 4);
    assert(!prepare_workers::segment_heap_signature(heap));
    assert(!prepare_workers::segment_heap_signature(nullptr));
    return 0;
}
"#;

#[test]
fn the_segment_heap_gets_more_prepare_workers() {
    let repo = root();
    let draw = source("gpu/vk/vk_draw.cpp");
    let run = draw
        .find("bool prepare_run(void (*progress)(size_t done, size_t total), bool background, bool refresh_states) {")
        .expect("prepare_run");
    let body = &draw[run..];
    let pool = &body[body.find("const unsigned quarter").expect("quarter")
        ..body.find("threads.emplace_back(worker)").expect("workers")];
    assert!(
        pool.contains("background ? std::min(quarter, cpus) : prepare_cpus()"),
        "prepare_run sizes its pool before the game with prepare_cpus, in play with its quarter"
    );
    assert!(
        body.contains("segment_heap() ? \"the segment heap"),
        "the log does not say which heap sized the pool"
    );

    let cache = source("gpu/vk/vk_progcache.cpp");
    let start = cache
        .find("unsigned prepare_cpus() {")
        .expect("prepare_cpus");
    let end = cache[start..].find("\n}\n").expect("prepare_cpus end") + start;
    let prepare = &cache[start..end];
    assert!(prepare.contains("prepare_workers::count(half_cpus(), usable_cpus()"));
    assert!(prepare.contains("segment_heap()"));
    let manifest = source("nx2/win_manifest.py");
    assert!(
        manifest.contains(">SegmentHeap</heapType>"),
        "the game's manifest does not ask for the segment heap"
    );
    let rust_manifest = source("builder/rust/src/win_manifest.rs");
    assert!(
        rust_manifest.contains(">SegmentHeap</heapType>"),
        "the Rust builder's manifest does not ask for the segment heap"
    );

    let Some(cxx) = cxx() else {
        eprintln!(
            "skipping the compiled prepare workers: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-prepare-workers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let file = dir.join("workers.cpp");
    let exe = dir.join(if cfg!(windows) {
        "workers.exe"
    } else {
        "workers"
    });
    fs::write(&file, FIXTURE).expect("write fixture");
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", repo.join("gpu/vk").display()))
        .arg(&file)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", file.display());
    let run = Command::new(&exe).output().expect("run the fixture");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}
