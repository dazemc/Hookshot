// Game-free checks of the Windows setup's Builder readiness contract (builder/setup/setup.c, upstream
// NotRexed/Hookshot 2dfe9011 adapted to this fork's Flutter package). The C function is cut out of the production
// source and compiled against authored Win32 file-attribute doubles: no installer, no Windows executable, no game data.

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

#[test]
fn an_incomplete_builder_is_repaired_instead_of_reused_or_launched() {
    let repo = root();
    let setup = fs::read_to_string(repo.join("builder/setup/setup.c")).expect("setup.c");
    let ready = function(&setup, "static int builder_ready(const wchar_t *exe)");
    let work = function(&setup, "static DWORD WINAPI work(LPVOID unused)");
    let launch = function(&setup, "static int launch_installed(const wchar_t *args)");
    assert!(
        work.contains("!builder_ready(exe)"),
        "an installed version with a missing startup file must be unpacked again"
    );
    assert!(
        work.contains("!builder_ready(staged_exe)"),
        "a staged version must be checked before it is published"
    );
    assert!(
        work.contains("quarantined") && work.contains("No complete installation was published"),
        "the staged failure must name the quarantine and say nothing was published"
    );
    assert!(
        launch.contains("!builder_ready(exe)"),
        "the launcher must fall back to the setup instead of starting an incomplete Builder"
    );
    assert!(
        launch.contains("!tag[0] || tag[0] == '.' || !list_name_ok(tag, 0)"),
        "a current.txt tag must be a folder name before it becomes a path"
    );
    assert!(
        ready.contains("flutter_windows.dll")
            && ready.contains("data\\\\icudtl.dat")
            && ready.contains("data\\\\app.so")
            && ready.contains("hookshot_engine.exe")
            && ready.contains("hookshot_engine.dll"),
        "the readiness contract must cover the Flutter runtime, its data folder and the Rust engine"
    );
    let package = fs::read_to_string(repo.join("scripts/windows/release_parts.sh"))
        .expect("scripts/windows/release_parts.sh");
    assert!(
        package.contains(
            "for name in flutter_windows.dll data/icudtl.dat data/app.so hookshot_engine.exe hookshot_engine.dll"
        ),
        "packaging must refuse a bundle the setup's builder_ready would reject"
    );

    let Some(cc) = compiler() else {
        eprintln!("skipping the setup readiness fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = scratch("setup-ready");
    let fixture = dir.join("ready.c");
    fs::write(
        &fixture,
        format!(
            r#"#define _CRT_SECURE_NO_WARNINGS   /* (Windows: wcscpy is not deprecated here) */
#include <stddef.h>
#include <stdio.h>
#include <wchar.h>

#define MAX_PATH 260
#define FILE_ATTRIBUTE_DIRECTORY 0x10
#define GetFileExInfoStandard 0
typedef unsigned long DWORD;
typedef struct {{
    DWORD dwFileAttributes;
    DWORD nFileSizeHigh;
    DWORD nFileSizeLow;
}} WIN32_FILE_ATTRIBUTE_DATA;

typedef struct {{ const wchar_t *path; int dir; unsigned size; }} Fake;
static Fake files[32];
static int nfiles;
static int fake_get(const wchar_t *path, int level, WIN32_FILE_ATTRIBUTE_DATA *info) {{
    (void)level;
    for (int i = 0; i < nfiles; i++) {{
        if (!wcscmp(path, files[i].path)) {{
            info->dwFileAttributes = files[i].dir ? FILE_ATTRIBUTE_DIRECTORY : 0;
            info->nFileSizeHigh = 0;
            info->nFileSizeLow = files[i].size;
            return 1;
        }}
    }}
    return 0;
}}
#define GetFileAttributesExW fake_get

{ready}

static const wchar_t *const exe = L"C:\\Games\\HookshotBuilder\\HookshotBuilder.exe";
static const wchar_t *const flutter = L"C:\\Games\\HookshotBuilder\\flutter_windows.dll";
static const wchar_t *const icu = L"C:\\Games\\HookshotBuilder\\data\\icudtl.dat";
static const wchar_t *const app = L"C:\\Games\\HookshotBuilder\\data\\app.so";
static const wchar_t *const engine = L"C:\\Games\\HookshotBuilder\\hookshot_engine.exe";
static const wchar_t *const engine_dll = L"C:\\Games\\HookshotBuilder\\hookshot_engine.dll";

static void add(const wchar_t *path, int is_dir, unsigned size) {{
    files[nfiles].path = path;
    files[nfiles].dir = is_dir;
    files[nfiles].size = size;
    nfiles++;
}}
static void complete(void) {{
    nfiles = 0;
    add(exe, 0, 100);
    add(flutter, 0, 100);
    add(icu, 0, 100);
    add(app, 0, 100);
    add(engine, 0, 100);
    add(engine_dll, 0, 100);
}}
static int without(const wchar_t *path) {{
    complete();
    for (int i = 0; i < nfiles; i++) if (!wcscmp(files[i].path, path)) {{
        files[i] = files[--nfiles];
        return 1;
    }}
    return 0;
}}
static void alter(const wchar_t *path, int dir, unsigned size) {{
    complete();
    for (int i = 0; i < nfiles; i++) if (!wcscmp(files[i].path, path)) {{
        files[i].dir = dir;
        files[i].size = size;
    }}
}}

int main(void) {{
    complete();
    if (!builder_ready(exe)) return 1;                       /* (a complete Flutter package) */
    if (!without(flutter) || builder_ready(exe)) return 2;   /* (the Flutter runtime) */
    if (!without(icu) || builder_ready(exe)) return 3;       /* (the data folder) */
    if (!without(app) || builder_ready(exe)) return 4;       /* (the AOT snapshot) */
    if (!without(engine) || builder_ready(exe)) return 5;    /* (the Rust engine's CLI) */
    if (!without(engine_dll) || builder_ready(exe)) return 6;/* (the Rust engine's library) */

    alter(engine_dll, 0, 0);
    if (builder_ready(exe)) return 7;                        /* (an empty file is not a component) */

    alter(app, 1, 100);
    if (builder_ready(exe)) return 8;                        /* (a folder in place of a file) */

    alter(exe, 1, 100);
    if (builder_ready(exe)) return 9;                        /* (the executable itself) */

    alter(exe, 0, 0);
    if (builder_ready(exe)) return 10;                       /* (an empty executable) */

    complete();
    if (builder_ready(L"HookshotBuilder.exe")) return 11;    /* (no folder to anchor the names) */

    complete();
    if (!without(exe) || builder_ready(exe)) return 12;      /* (nothing to start) */

    puts("ok");
    return 0;
}}
"#
        ),
    )
    .expect("write the setup readiness fixture");
    let binary = dir.join(if cfg!(windows) { "ready.exe" } else { "ready" });
    let built = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(&fixture)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("run the C compiler");
    assert!(
        built.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&binary).output().expect("run the fixture");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        run.status.success(),
        "fixture exit {:?}: {}{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}
