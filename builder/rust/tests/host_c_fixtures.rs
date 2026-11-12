// Game-free checks of the C host's pure parts, compiled with the host C compiler: the Steam pads' ignore-list filter
// (host/steam_pads.h, upstream NotRexed/Hookshot e4d45157) and, later here, the other fixtures that need nothing but a
// compiler. A machine with no C compiler skips them, as CI's Rust job may have none on Windows.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// Compile a C fixture with the host compiler, returning its output. `None` when there is no compiler, so the caller
/// can say it skipped.
fn compile_and_run(
    name: &str,
    sources: &[PathBuf],
    includes: &[PathBuf],
    flags: &[&str],
) -> Option<Output> {
    let cc = compiler()?;
    let dir = scratch(name);
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
    for source in sources {
        build.arg(source);
    }
    build.args(flags).arg("-o").arg(&exe);
    let built = build.output().expect("run the C compiler");
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
fn steam_pads_keep_their_motion_sensors_under_steam_input() {
    let repo = root();
    let Some(run) = compile_and_run(
        "steam-pads",
        &[repo.join("tests/steam_pads_test.c")],
        &[repo.join("host")],
        &[],
    ) else {
        eprintln!("skipping the Steam pads fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).starts_with("ok:"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}
