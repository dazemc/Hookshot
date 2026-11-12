// Game-free checks of the Vulkan memory, write-back and readback rules that live in plain headers: page ranges and
// part matching (gpu/vk/wb_pages.h), fence/timeline ordering (gpu/vk/rec_value.h), write-back shadow lifetime
// (gpu/vk/wb_shadow_policy.h), retired import give-back (gpu/vk/retired_give_back.h) and completed write-back
// observations (gpu/vk/wb_observe.h). They need a C++ compiler and nothing else: no game, GPU or Vulkan headers.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// Compile one fixture with the C++ compiler and run it. None when there is no compiler, so the caller can say it
/// skipped (CI's Rust job may have none on Windows).
fn run_fixture(name: &str, source: &str) -> Option<String> {
    let cxx = compiler()?;
    let repo = root();
    let dir = std::env::temp_dir().join(format!(
        "hookshot-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let built = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(repo.join(source))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        built.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&exe).output().expect("run the fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{source}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(String::from_utf8_lossy(&run.stdout).into_owned())
}

fn check(name: &str, source: &str, expected: &str) {
    let Some(stdout) = run_fixture(name, source) else {
        eprintln!("skipping {source}: no C++ compiler (CXX, c++, g++, clang++)");
        return;
    };
    assert!(
        stdout.contains(expected),
        "{source}: expected {expected:?}, got {stdout:?}"
    );
}

#[test]
fn writeback_page_ranges_and_parts() {
    check(
        "wb-pages",
        "tests/wb_pages_test.cpp",
        "PASS: write-back page ranges and part matching",
    );
}

#[test]
fn fence_and_timeline_ordering() {
    check(
        "rec-value",
        "tests/test_rec_value.cpp",
        "PASS: a value names the recording that finished it",
    );
}

#[test]
fn writeback_shadow_lifetime() {
    check(
        "wb-shadow",
        "tests/wb_shadow_policy_test.cpp",
        "PASS: write-back shadow lifetime",
    );
}

#[test]
fn retired_imports_with_refused_mirrors_go_back() {
    check(
        "retired-give-back",
        "tests/retired_give_back_test.cpp",
        "retired_give_back: ok",
    );
}

#[test]
fn completed_writeback_observations() {
    check(
        "wb-observe",
        "tests/wb_observation_lifetime_test.cpp",
        "PASS: completed write-back observations",
    );
}
