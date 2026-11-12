// Game-free source guards for the Windows-only parts of the updater that a
// Linux test cannot run: builder/setup/setup.c checks a part's length and
// SHA-256, scans the zip before tar.exe, verifies the requested release, and
// reads its installed marker whole; the Flutter updater keeps API asset links
// on the API host and checks a requested release's tag; the Python setup scans
// its archive and writes its markers atomically. No game or network.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_windows_setup_checks_the_download_before_it_unpacks() {
    let setup = source("builder/setup/setup.c");
    let length = setup
        .find("if (expect && got != expect) ok = 0;")
        .expect("setup.c no longer checks a download's length");
    let sha = setup
        .find("if (strcmp(have, p->sha)) DeleteFileW(part)")
        .expect("setup.c no longer checks a download's SHA-256");
    let scan = setup
        .find("const int safe = zf && zip_members_safe(zf);")
        .expect("setup.c no longer scans a part's zip");
    let tar = setup
        .find("RunResult tar = run_wait(tarcmd, dest);")
        .expect("setup.c no longer runs tar.exe");
    assert!(
        length < sha && sha < scan && scan < tar,
        "setup.c must check length, SHA-256 and the zip before tar.exe"
    );
    assert!(
        setup.contains("if (!parts_has(buf, p->sha)) return 0;"),
        "setup.c reuses a part from a substring of parts.txt"
    );
    assert!(
        setup.contains("read_small(marker, installed, sizeof installed);")
            && setup.contains("strcmp(installed, tag)"),
        "setup.c trusts an installed marker without reading it whole"
    );
    assert!(
        setup.contains("strcmp(want, tag)"),
        "setup.c no longer refuses a setup list for another release"
    );
}

#[test]
fn the_flutter_updater_keeps_asset_links_on_the_api_host() {
    let installer = source("builder/flutter/lib/updater/installer.dart");
    assert!(
        installer.contains("assetUrl.startsWith('https://api.github.com/')"),
        "the Flutter updater follows an API asset link outside api.github.com"
    );
    assert!(
        installer.contains("manifest.tag != tag"),
        "the Flutter updater installs a list for another release"
    );
    assert!(
        installer.contains("createSync(exclusive: true)"),
        "the Flutter updater writes a part-supplied leaf without an exclusive create"
    );
}

#[test]
fn the_python_setup_checks_its_archive_and_markers() {
    let setup = source("builder/py/hookshot_builder/setup_parts.py");
    let check = setup
        .find("check_zip_members(z)")
        .expect("the Python setup extracts a zip without checking its members");
    let extract = setup
        .find("z.extractall(dest)")
        .expect("the Python setup no longer extracts the part");
    assert!(
        check < extract,
        "the Python setup extracts before checking the zip"
    );
    assert!(
        setup.contains("def check_zip_members(zipped):") && setup.contains("stat.S_ISLNK"),
        "the Python setup does not refuse a zip link"
    );
    assert!(
        setup.contains("write_small_atomic(os.path.join(final, \"installed.txt\"), tag)")
            && setup.contains("paths.read_small(os.path.join(d, \"installed.txt\")) == tag"),
        "the Python setup trusts a partial or stale installed marker"
    );
}
