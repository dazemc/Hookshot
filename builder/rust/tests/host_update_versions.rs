// The update tracks' version order and the guard that keeps a folder newer than the latest release from being offered
// the older one (host/hos_update.c; upstream NotRexed/Hookshot f64696d5). The functions are cut out of the source and
// compiled as they are, so the test reads the Windows and Linux copies of the guard too. No network, no Builder.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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
fn a_folder_newer_than_the_latest_release_is_not_offered_it() {
    let source = fs::read_to_string(root().join("host/hos_update.c")).expect("host/hos_update.c");
    // (Windows' and Linux's copies of hos_update_check carry the same guard)
    let guard = "if (!nightly_version(have) && newer_version(have, tag)) return;";
    assert_eq!(source.matches(guard).count(), 2, "{guard}");

    let Some(cc) = compiler() else {
        eprintln!("skipping the compiled version checks: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = scratch("update-versions");
    let fixture = dir.join("versions.c");
    let exe = dir.join(if cfg!(windows) {
        "versions.exe"
    } else {
        "versions"
    });
    let mut text = String::from(
        "#include <ctype.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\n",
    );
    for name in [
        "static int nightly_version(const char *s)",
        "static int parse_version(const char *s, long v[4])",
        "static int newer_version(const char *a, const char *b)",
    ] {
        text.push_str(&function(&source, name));
        text.push_str("\n\n");
    }
    text.push_str(
        r#"int main(void) {
    /* a release is newer than the release candidate before it, and older than the next one */
    if (!newer_version("v2.2.0", "v2.1.0") || newer_version("v2.1.0", "v2.2.0")) return 1;
    if (!newer_version("v2.1.0", "v2.1.0-rc.2") || !newer_version("v2.1.0-rc.2", "v2.1.0-rc.1")) return 2;
    if (newer_version("v2.1.0-rc.2", "v2.1.0") || newer_version("v2.1.0", "v2.1.0")) return 3;
    /* the guard: a test build of the next release (the latest is v2.1.0) is offered nothing, a nightly is not a release */
    if (!(newer_version("v2.2.0", "v2.1.0") && !nightly_version("v2.2.0"))) return 4;
    if (!(newer_version("v2.1.0-rc.1", "v2.1.0") == 0 && !nightly_version("v2.1.0-rc.1"))) return 5;
    if (nightly_version("v2.2.0") || nightly_version("v2.1.0-rc.1")) return 6;
    if (!nightly_version("v2.1.0-rc.1-3-g1a2b3c4") || !nightly_version("v2.1.0-3-g1a2b3c4-dirty")) return 7;
    puts("ok");
    return 0;
}
"#,
    );
    fs::write(&fixture, text).expect("write fixture");
    let build = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(&fixture)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C compiler");
    assert!(build.success(), "{cc} failed on {}", fixture.display());
    let run = Command::new(&exe)
        .output()
        .expect("run the version fixture");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
    let _ = fs::remove_dir_all(dir);
}
