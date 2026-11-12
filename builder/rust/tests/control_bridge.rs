// The control bridge's Debug-tab commands (host/control.c, host/control.h), game-free: tests/control_test.c builds the
// real bridge with HOS_CONTROL_NO_GAME and a fake game, then drives it over a real socket. The cases include the
// teleport and the age switch (upstream NotRexed/Hookshot b157c3e8): a bad argument, the age in status, already that
// age, a stale sample of it, a timeout outside play, replaced by a later teleport, and without waiting. No game or GPU
// is needed; a machine with no C compiler skips it, as CI's Rust job may have none on Windows.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

#[test]
fn the_bridge_carries_the_debug_tabs_teleport_and_age_switch() {
    if cfg!(windows) {
        // (the fixture's POSIX half: a Unix socket and fork; the Windows half is built by the Windows CI's own run)
        return;
    }
    let Some(cc) = compiler() else {
        eprintln!("skipping the control bridge fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let dir = scratch("control-bridge");
    let exe = dir.join("control_test");
    // (control.c's POSIX transport uses accept4 and struct ucred, so _GNU_SOURCE is the build's own)
    let build = Command::new(&cc)
        // (not -Werror: the fixture's fixed-size socket paths trip GCC's format-truncation warning on a long scratch path)
        .args([
            "-std=gnu11",
            "-D_GNU_SOURCE",
            "-DHOS_CONTROL_NO_GAME",
            "-Wall",
            "-Wextra",
        ])
        .arg(format!("-I{}", repo.join("host").display()))
        .arg(format!("-I{}", repo.join("gpu/vk/compat").display()))
        .arg(repo.join("tests/control_test.c"))
        .arg(repo.join("host/control.c"))
        .arg("-o")
        .arg(&exe)
        .arg("-lpthread")
        .arg("-lm")
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe)
        .arg(&dir)
        .output()
        .expect("run the control bridge fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8_lossy(&run.stdout);
    assert!(
        text.contains("the age switch"),
        "the age cases did not run: {text}"
    );
    assert!(text.contains("PASS: status"), "{text}");
}

#[test]
fn the_control_cli_offers_the_age_command() {
    let repo = root();
    let candidates = std::env::var("PYTHON")
        .ok()
        .into_iter()
        .chain(["python3".into(), "python".into()]);
    let Some(python) = candidates.into_iter().find(|python| {
        Command::new(python)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }) else {
        eprintln!("skipping the control CLI check: no Python");
        return;
    };
    let help = Command::new(&python)
        .arg(repo.join("scripts/hookshot_ctl.py"))
        .arg("--help")
        .output()
        .expect("run the control CLI");
    assert!(
        help.status.success(),
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("Switch age"), "{text}");
    assert!(text.contains("child") && text.contains("adult"), "{text}");
}
