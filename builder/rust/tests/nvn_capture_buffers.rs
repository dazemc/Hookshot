// Game-free check of the capture-buffer size classes and prewarm from upstream NotRexed/Hookshot 7de45133 (#2437), which
// this fork keeps on top of current main: sizes above 16 MB round up to 32/64/128/192/256 MB classes, spares are kept up to
// 256 MB (768 MB in all), and 32/64/128/192 MB spares are made ahead on a worker. A source check plus a compiled run of
// cap_buf_size; no game, dump or GPU.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|_| panic!("missing {path}"))
}

/// A function's definition from a source, braces matched.
fn block(source: &str, marker: &str) -> String {
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

fn compiler() -> Option<String> {
    for candidate in
        std::env::var("CXX")
            .ok()
            .into_iter()
            .chain(["c++".into(), "g++".into(), "clang++".into()])
    {
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

fn compile_and_run_cxx(name: &str, source: &str) -> Option<std::process::Output> {
    let cc = compiler()?;
    let dir = scratch(name);
    let file = dir.join("fixture.cpp");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    fs::write(&file, source).expect("fixture source");
    let built = Command::new(&cc)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(&file)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
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
fn capture_buffers_use_size_classes_and_are_made_ahead() {
    let memory = read("gpu/vk/vk_memory.cpp");
    for part in [
        "RX_VK_CAPTURE_KEEP_OLD",
        "768ull << 20",
        "RX_VK_CAP_PREWARM",
        "cap_prewarm_class(c)",
        "std::thread(cap_spare_make, bytes, index).detach()",
        "std::atexit(cap_exit_note)",
        "cap_shutting_down",
    ] {
        assert!(memory.contains(part), "the capture buffer fix lacks {part}");
    }
    let size = block(&memory, "uint64_t cap_buf_size(uint64_t need) {")
        .replace("static const uint64_t min", "const uint64_t min")
        .replace("static const bool keep_old", "const bool keep_old");
    let Some(run) = compile_and_run_cxx(
        "nvn-capture-size",
        &format!(
            r#"#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
static bool keep_old_env = false;
static bool env_on(const char*) {{ return keep_old_env; }}
{size}
int main() {{
    constexpr uint64_t MB = 1ull << 20;
    if (cap_buf_size(1 * MB) != 2 * MB) return 1;     // (the smallest class, as before)
    if (cap_buf_size(20 * MB) != 32 * MB) return 2;   // (16 MB and up: the 32 MB class)
    if (cap_buf_size(40 * MB) != 64 * MB) return 3;
    if (cap_buf_size(100 * MB) != 128 * MB) return 4;
    if (cap_buf_size(130 * MB) != 192 * MB) return 5;
    if (cap_buf_size(200 * MB) != 256 * MB) return 6;
    if (cap_buf_size(300 * MB) != 300 * MB) return 7; // (past the classes: the MB)
    keep_old_env = true;
    if (cap_buf_size(100 * MB) != 100 * MB) return 8; // (RX_VK_CAPTURE_KEEP_OLD: rounded to the MB)
    std::puts("ok");
    return 0;
}}
"#,
            size = size,
        ),
    ) else {
        eprintln!("skipping the capture size fixture: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).starts_with("ok"));
}
