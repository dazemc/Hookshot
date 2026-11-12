// The menu's upscaler rows and DLSS scene-size checks (gpu/vk/upscaler_rows.h, gpu/vk/dlss_checks.h): the saved shares,
// the engines' steps and the rounding allowance that lets a rounded render size up to two input pixels under a third count
// as Ultra Performance. Game-free: two header-only fixtures are compiled and run; no game, dump or GPU is needed.

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

fn run_fixture(cxx: &str, source: &str) {
    let repo = root();
    let dir = std::env::temp_dir().join(format!(
        "hookshot-menu-upscaler-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) { "fixture.exe" } else { "fixture" });
    let build = Command::new(cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", repo.display()))
        .arg(repo.join(source))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cxx} {source}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{source}: {}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn the_upscaler_rows_and_dlss_sizes_keep_their_rounding_rules() {
    let Some(cxx) = compiler() else {
        eprintln!("skipping the upscaler fixtures: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    run_fixture(&cxx, "tests/test_upscaler_rows.cpp");
    run_fixture(&cxx, "tests/test_dlss_checks.cpp");
}
