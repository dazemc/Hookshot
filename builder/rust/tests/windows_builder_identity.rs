// The Windows Builder's identity and the game's DPI/manifest contract, as source checks: the runner's title and
// AppUserModelID, its version resource, the three manifests' DPI settings, and the game window's title/class.
// Upstream's WPF builder uses the same product name and app id (builder/py/hookshot_builder_start.py); the game's
// UTF-8 manifest is upstream nx2/win_manifest.py's. No executable, game folder or game data is involved.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn the_flutter_runner_names_the_builder_and_groups_it() {
    let main = source("builder/flutter/windows/runner/main.cpp");
    assert!(main.contains("window.Create(L\"Hookshot Builder\""));
    assert!(
        main.contains("SetCurrentProcessExplicitAppUserModelID(L\"NotRexed.Hookshot.Builder\")")
    );
    assert!(main.contains("#include <shobjidl.h>"));
    let cmake = source("builder/flutter/windows/runner/CMakeLists.txt");
    assert!(cmake.contains("shell32.lib"));
    let rc = source("builder/flutter/windows/runner/Runner.rc");
    assert!(rc.contains("VALUE \"ProductName\", \"Hookshot Builder\""));
    assert!(rc.contains("VALUE \"FileDescription\", \"Hookshot Builder\""));
    assert!(rc.contains("VALUE \"InternalName\", \"HookshotBuilder\""));
    assert!(rc.contains("VALUE \"OriginalFilename\", \"HookshotBuilder.exe\""));
    assert!(rc.contains("BLOCK \"040904b0\""));
    assert!(rc.contains("VALUE \"Translation\", 0x409, 1200"));
}

#[test]
fn every_windows_manifest_keeps_its_dpi_and_utf8_contract() {
    let runner = source("builder/flutter/windows/runner/runner.exe.manifest");
    assert!(runner.contains("PerMonitorV2"));
    let wpf = source("builder/HookshotBuilder/app.manifest");
    assert!(wpf.contains("PerMonitorV2"));
    let setup = source("builder/setup/setup.manifest");
    assert!(setup.contains("dpiAware"));
    let game = source("builder/rust/src/win_manifest.rs");
    assert!(game.contains("activeCodePage"));
    assert!(game.contains("UTF-8"));
    assert!(game.contains("const RT_MANIFEST: u16 = 24;"));
    assert!(game.contains("const CREATEPROCESS_MANIFEST_RESOURCE_ID: u16 = 1;"));
    let python = source("nx2/win_manifest.py");
    assert!(python.contains("activeCodePage"));
    assert!(python.contains("RT_MANIFEST = 24"));
}

#[test]
fn the_game_window_keeps_its_title_class_and_dpi_handling() {
    let display = source("host/display.c");
    assert!(display.contains("wc.lpszClassName = \"Hookshot\";"));
    assert!(display.contains("CreateWindowA(\"Hookshot\", HOS_GAME_TITLE"));
    assert!(display.contains("WM_DPICHANGED"));
    assert!(display.contains("dpi_aware_thread();"));
    assert!(display.contains("SetThreadDpiAwarenessContext"));
    let hos = source("host/hos.h");
    assert!(hos.contains("#define HOS_GAME_TITLE \"Hookshot\""));
    let main = source("host/hos_main.c");
    assert!(main.contains("SetProcessDpiAwarenessContext"));
    assert!(main.contains("HOS_DPI_UNAWARE"));
}
