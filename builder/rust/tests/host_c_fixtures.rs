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

/// A function's definition from a C source, braces matched.
fn function(source: &str, marker: &str) -> String {
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

#[test]
fn the_window_title_takes_its_optional_override() {
    let repo = root();
    let source = fs::read_to_string(repo.join("host/posix/display_sdl.c")).expect("display_sdl.c");
    let title = function(
        &source,
        "static const char *window_title(const char *fallback)",
    );
    let dir = scratch("window-title");
    let fixture = dir.join("window_title.c");
    fs::write(
        &fixture,
        format!(
            r#"#ifndef _WIN32
#define _GNU_SOURCE   /* (setenv and unsetenv) */
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
{title}
#if defined(_WIN32)
static void put(const char *name, const char *value) {{ _putenv_s(name, value ? value : ""); }}
#else
static void put(const char *name, const char *value) {{ if (value) setenv(name, value, 1); else unsetenv(name); }}
#endif
int main(void) {{
    put("HOS_WINDOW_TITLE", NULL);
    if (strcmp(window_title("Hookshot"), "Hookshot")) return 1;   /* (unset: the fallback) */
    put("HOS_WINDOW_TITLE", "");
    if (strcmp(window_title("Hookshot"), "Hookshot")) return 2;   /* (empty: the fallback too) */
    put("HOS_WINDOW_TITLE", "The Legend of Zelda: Ocarina of Time");
    if (strcmp(window_title("Hookshot"), "The Legend of Zelda: Ocarina of Time")) return 3;
    if (strcmp(window_title("Other"), "The Legend of Zelda: Ocarina of Time")) return 4;
    put("HOS_WINDOW_TITLE", NULL);
    puts("ok");
    return 0;
}}
"#
        ),
    )
    .expect("write the window title fixture");
    let Some(run) = compile_and_run("window-title", &[fixture], &[], &[]) else {
        eprintln!("skipping the window title fixture: no C compiler (CC, cc, gcc or clang)");
        let _ = fs::remove_dir_all(dir);
        return;
    };
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}
