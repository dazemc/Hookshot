// The game folder's Linux launcher picks the builder that made the game
// before an installed one (builder/linux/hookshot-folder, upstream
// NotRexed/Hookshot bd699925). A folder built by a newer release must not be
// opened by an older installed builder that would offer to rebuild it
// backwards. The test runs the real shell script against authored stand-ins;
// no game, builder or network is involved.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hookshot-launcher-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_exec(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A game folder with the real launcher, plus an executable game that says
/// it ran.
fn game(name: &str) -> (PathBuf, PathBuf) {
    let folder = scratch(name);
    let script = folder.join("hookshot");
    fs::copy(root().join("builder/linux/hookshot-folder"), &script).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let marker = folder.join("ran.txt");
    write_exec(
        &folder.join("bin/hookshot"),
        "#!/bin/sh\nprintf 'game %s\\n' \"$*\" > \"$MARKER\"\n",
    );
    (folder, marker)
}

fn builder(path: &Path, name: &str) {
    write_exec(
        path,
        &format!("#!/bin/sh\nprintf '{name} %s\\n' \"$*\" > \"$MARKER\"\n"),
    );
}

fn run(folder: &Path, home: &Path, marker: &Path) -> String {
    let output = Command::new("/bin/sh")
        .arg(folder.join("hookshot"))
        .current_dir(folder)
        .env("HOOKSHOT_BUILDER_HOME", home)
        .env("MARKER", marker)
        .output()
        .expect("run the launcher");
    assert!(
        output.status.success(),
        "launcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::read_to_string(marker).unwrap_or_default()
}

#[test]
fn the_builder_that_made_the_game_wins_over_an_installed_one() {
    let (folder, marker) = game("recorded");
    let home = scratch("recorded-home");
    let recorded = folder.join("recorded/AppRun");
    let installed = home.join("hookshot/builder/versions/v9/AppRun");
    builder(&recorded, "recorded");
    builder(&installed, "installed");
    fs::write(home.join("hookshot/builder/current"), "v9\n").unwrap();
    fs::write(
        folder.join("hookshot_version.txt"),
        format!(
            "profiles=1\nversion=v2.2.0\nbuilder={}\n",
            recorded.display()
        ),
    )
    .unwrap();

    let ran = run(&folder, &home, &marker);
    assert!(ran.starts_with("recorded --launch "), "{ran}");
    assert!(!ran.contains("installed"), "{ran}");
}

#[test]
fn a_missing_recorded_builder_falls_back_to_the_installed_one() {
    let (folder, marker) = game("missing");
    let home = scratch("missing-home");
    let installed = home.join("hookshot/builder/versions/v9/AppRun");
    builder(&installed, "installed");
    fs::write(home.join("hookshot/builder/current"), "v9\n").unwrap();
    fs::write(
        folder.join("hookshot_version.txt"),
        format!(
            "profiles=1\nversion=v2.2.0\nbuilder={}\n",
            folder.join("gone/AppRun").display()
        ),
    )
    .unwrap();

    let ran = run(&folder, &home, &marker);
    assert!(ran.starts_with("installed --launch "), "{ran}");
}

#[test]
fn neither_builder_starts_the_game() {
    let (folder, marker) = game("game");
    let home = scratch("game-home");

    let ran = run(&folder, &home, &marker);
    assert!(ran.starts_with("game "), "{ran}");
}
