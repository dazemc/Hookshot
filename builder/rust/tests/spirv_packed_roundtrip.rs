// Game-free round-trip check for the packed SPIR-V representation: stages pack into one buffer and unpack byte-for-byte.
// No game, dump or GPU is involved.
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiler() -> Option<String> {
    for c in ["g++", "c++", "clang++"] {
        if Command::new(c).arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
            return Some(c.to_string());
        }
    }
    None
}

#[test]
fn spirv_packed_round_trips() {
    let Some(cxx) = compiler() else {
        eprintln!("skipping: no C++ compiler");
        return;
    };
    let repo = root();
    let dir = std::env::temp_dir().join("hookshot-spirv-packed");
    let _ = std::fs::create_dir_all(&dir);
    let src = repo.join("tests/spirv_packed_roundtrip_test.cpp");
    let exe = dir.join("spirv_packed");
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run compiler");
    assert!(build.success(), "spirv packed fixture did not compile");
    let run = Command::new(&exe).output().expect("run");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(String::from_utf8_lossy(&run.stdout).contains("spirv packed round-trip: ok"));
    let _ = std::fs::remove_dir_all(&dir);
}
