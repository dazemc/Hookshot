// The portable layout is shared by four places that cannot call each other in a
// game-free test: the native setup, the game's update offer, the Rust packager
// and the Flutter app. This check reads their production sources and requires
// the same marker, relative association and home wiring in each. No game data,
// no Windows executable and no Builder launch.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
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

#[test]
fn the_native_setup_creates_and_reuses_one_marked_home() {
    let header = source("builder/setup/portable_home.h");
    assert!(header.contains("#define HB_HOME_MARKER L\".hookshot-portable\""));
    assert!(header.contains("#define HB_HOME_MAGIC \"Hookshot Builder portable home v1\""));
    assert!(header.contains("#define HB_GAME_HOME L\".hookshot_builder_home\""));
    assert!(header.contains("hb_home_discover("));
    assert!(header.contains("hb_home_game("));

    let setup = source("builder/setup/setup.c");
    assert!(setup.contains("#include \"portable_home.h\""));
    assert!(setup.contains("static HbHome g_home;"));
    let prefix = function(&setup, "static int portable_prefix(");
    assert!(prefix.contains("L\"--portable\""));
    let home = function(&setup, "static int init_home(");
    assert!(home.contains("SetEnvironmentVariableW(L\"HOOKSHOT_SETUP_BASE\""));
    assert!(home.contains("SetEnvironmentVariableW(L\"HOOKSHOT_BUILDER_PORTABLE\""));
    assert!(home.contains("hb_home_mark(g_home.root)"));
    assert!(home.contains("hb_home_discover(&g_home"));
    let work = function(&setup, "static DWORD WINAPI work(");
    assert!(work.contains("wcscpy(base, g_home.root)"));
    assert!(work.contains("hb_home_marked(g_home.root)"));
    assert!(work.contains("publish_install(install, final_dir, g_home.portable)"));
    assert!(work.contains("if (!g_home.portable) rmtree(final_dir);"));
    assert!(work.contains("builder_ready(staged_exe)"));
    let reuse = function(&setup, "static int reuse_part(");
    assert!(reuse.contains("if (!g_home.portable) return MoveFileW(from, to) != 0;"));
    assert!(reuse.contains("copy_part_tree(from, to)"));
    let launch = function(&setup, "static int launch_installed(");
    assert!(launch.contains("wcscpy(base, g_home.root)"));
    assert!(!launch.contains("CSIDL_LOCAL_APPDATA"));
    assert!(launch.contains("builder_ready(exe)"));
}

#[test]
fn the_update_offer_uses_the_game_relative_launcher_and_ignores_absolute_arguments() {
    let update = source("host/hos_update.c");
    let local = function(&update, "static int local_builder(");
    assert!(local.contains("\"builder_local\""));
    assert!(local.contains("L\"%ls\\\\hookshot.exe\""));
    assert!(local.contains("GetFileAttributesW(out)"));
    let check = function(&update, "void hos_update_check(");
    assert!(check.contains("const int portable = read_value(file, \"builder_local\""));
    assert!(check.contains("portable ? local_builder(file, dir, wbuilder, MAX_PATH)"));
    assert!(check.contains("!portable && read_value(file, \"builder_args\""));
}

#[test]
fn the_rust_packager_publishes_a_relative_association_and_no_absolute_builder() {
    let package = source("builder/rust/src/package.rs");
    assert!(package.contains("crate::portable::association_for(out)"));
    assert!(package.contains("crate::portable::write_game_home(out, association.as_deref())"));
    assert!(package.contains("crate::portable::version_text(&release, association.as_deref())"));
    assert!(package.contains("stage_game_launcher(platform, &root, out)"));
    assert!(package.contains("HookshotBuilder-Setup.exe"));
    let portable = source("builder/rust/src/portable.rs");
    assert!(portable.contains("pub const HOME_MARKER: &str = \".hookshot-portable\";"));
    assert!(portable.contains("pub const GAME_HOME: &str = \".hookshot_builder_home\";"));
    assert!(portable.contains("\"builder_local=1\\n\""));
    assert!(!portable.contains("format!(\"builder="));
    let orchestrate = source("builder/rust/src/orchestrate.rs");
    assert!(orchestrate.contains("crate::portable::check_env()?"));
}

#[test]
fn the_flutter_app_keeps_its_home_and_settings_relative() {
    let config = source("builder/flutter/lib/config.dart");
    assert!(config.contains("const portableMarker = '.hookshot-portable';"));
    assert!(config.contains("const portableMarkerText = 'Hookshot Builder portable home v1';"));
    assert!(config.contains("const gameHomeMarker = '.hookshot_builder_home';"));
    assert!(config.contains("BuilderHome? _discoverFromExecutable("));
    assert!(config.contains("for (var depth = 0; depth <= 3; depth++)"));
    assert!(config.contains("portableSavePath("));
    assert!(config.contains("portableLoadPath("));
    assert!(config.contains("const configPathKeys = {'out', 'nup', 'romfs', 'exefs'};"));
    assert!(config.contains("if (portableHome && configPathKeys.contains(entry.key)"));
    let build = source("builder/flutter/lib/backend/rust_build.dart");
    assert!(build.contains("if (cfg.isPortable()) return cfg.builderHome();"));
    assert!(build.contains("if (cfg.isPortable()) return p.join(cfg.builderHome(), 'ngx-sdk');"));
    let bloc = source("builder/flutter/lib/blocs/builder_bloc.dart");
    assert!(bloc.contains("env['HOOKSHOT_SETUP_BASE'] = cfg.builderHome();"));
    assert!(bloc.contains("env['HOOKSHOT_BUILDER_PORTABLE'] = '1';"));
}

#[test]
fn the_python_fallback_build_writes_the_same_association() {
    let make_game = source("nx2/make_game.py");
    assert!(make_game.contains("def portable_game_home(out):"));
    assert!(make_game.contains("def write_game_home(out, relative):"));
    assert!(make_game.contains(".hookshot-portable"));
    assert!(make_game.contains("builder_local=1"));
    assert!(make_game.contains("os.environ.get(\"HOOKSHOT_SETUP_BASE\")"));
    assert!(make_game.contains("os.replace(partial, destination)"));
}

#[test]
fn the_windows_package_keeps_the_builder_relative_to_the_install() {
    let release = source("scripts/windows/release_parts.sh");
    assert!(release.contains("lines+=(\"run HookshotBuilder/HookshotBuilder.exe\")"));
    assert!(release.contains("cp -f \"$setup\" \"$app/HookshotBuilder-Setup.exe\""));
    assert!(!release.contains("run C:\\"));
    assert!(!release.contains("run /"));
}
