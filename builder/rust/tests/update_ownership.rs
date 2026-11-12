// Update ownership after upstream NotRexed/Hookshot 6b02e1fe (#2926): the folder's launcher asks GitHub and
// offers Update, and the game no longer makes a network call of its own. A folder started without its launcher
// still starts: host/hos_main.c says where updates come from, without waiting on the network. Game-free: the
// sources and the build scripts are read, nothing is launched.

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
fn the_launcher_owns_updates_and_the_game_has_a_safe_fallback() {
    let main = source("host/hos_main.c");
    assert!(
        !main.contains("hos_update_check"),
        "the game still calls its update check"
    );
    assert!(
        main.contains("updates: this game was started without its folder's launcher")
            && main.contains("Updates are never ")
            && main.contains("checked from inside the game."),
        "a folder started without its launcher must say where updates come from"
    );
    for forbidden in [
        "WinHttp",
        "WinHttpOpen",
        "curl ",
        "api.github.com",
        "HOOKSHOT_UPDATE",
    ] {
        assert!(
            !main.contains(forbidden),
            "the fallback must not reach the network ({forbidden})"
        );
    }

    // The launchers mark the game as theirs; the marker is what silences the notice, and older folders keep it too.
    for path in [
        "builder/linux/hookshot-launch",
        "builder/linux/hookshot-folder",
        "builder/flutter/lib/blocs/builder_bloc.dart",
        "builder/HookshotBuilder/MainWindow.xaml.cs",
        "builder/setup/setup.c",
    ] {
        let text = source(path);
        assert!(
            text.contains("HOOKSHOT_NO_UPDATE_CHECK"),
            "{path} does not tell the game its launcher owns updates"
        );
    }

    // No WinHTTP link, and no builder arguments only the old game-side offer read.
    let build = source("nx2/build.py");
    assert!(!build.contains("-lwinhttp"), "the host still links WinHTTP");
    let rust_host = source("builder/rust/src/host_build.rs");
    assert!(
        !rust_host.contains("-lwinhttp"),
        "the Rust host build still links WinHTTP"
    );
    let make_game = source("nx2/make_game.py");
    assert!(
        !make_game.contains("builder_args"),
        "hookshot_version.txt still writes builder_args"
    );
    let core = source("builder/py/hookshot_builder/core.py");
    assert!(
        !core.contains("HOOKSHOT_BUILDER_ARGS"),
        "the Python builder still passes arguments only the game-side offer read"
    );

    // The fallback is documented, so a player who starts bin/hookshot by hand knows what to do.
    let development = source("docs/development.md");
    assert!(
        development.contains("launcher owns updates")
            && development.contains("HOOKSHOT_NO_UPDATE_CHECK=1"),
        "docs/development.md does not explain update ownership"
    );
}
