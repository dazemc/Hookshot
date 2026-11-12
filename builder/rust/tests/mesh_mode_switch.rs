// Mesh shaders is a launch switch, not a menu row (gpu/vk/mesh_mode.h, vk_main.cpp, vk_menu.cpp). An existing
// hookshot_options.ini's mesh_shaders= line still chooses as the old row did, so a saved path is not silently changed;
// RX_VK_MESH_SHADERS over it. The rule is compiled and run from tests/test_mesh_mode_switch.cpp. No game, dump or GPU.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A C/C++ definition's braces, matched, from its first line.
fn block(text: &str, marker: &str) -> String {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + text[start..].find('{').expect("opening brace") + 1;
    let bytes = text.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    text[start..end].to_string()
}

fn compiler() -> Option<String> {
    let candidates =
        std::env::var("CXX")
            .ok()
            .into_iter()
            .chain(["c++".into(), "g++".into(), "clang++".into()]);
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

#[test]
fn the_old_row_is_gone_and_the_switch_chooses() {
    let main = source("gpu/vk/vk_main.cpp");
    let device = block(&main, "bool create_device() {");
    assert!(
        device.contains(
            "mesh_rule::choose(getenv(\"RX_VK_MESH_SHADERS\"), getenv(\"RX_VK_FORCE_MESH_FALLBACK\"), old)"
        ),
        "create_device does not take the mode from RX_VK_MESH_SHADERS through mesh_mode.h's rule"
    );
    assert!(
        device.contains("mesh_rule::old_ini_choice(line, &old)"),
        "create_device no longer reads the old mesh_shaders= line that existing files keep"
    );
    assert!(
        device.contains("logf(\"vk: mesh shaders: %s, from %s: %s\"")
            && device.contains("mesh_choice.why"),
        "the log line does not say where the mesh mode came from"
    );
    assert!(
        device.contains(
            "if (mesh_mode_used == 0 && dev.props.vendorID == 0x1002) mesh_mode_used = 2;"
        ),
        "Automatic no longer picks Compute on Linux AMD"
    );

    let menu = source("gpu/vk/vk_menu.cpp");
    let table = menu[menu.find("Row rows[] = {").expect("the row table")..].to_string();
    let table = &table[..table.find("\n};").expect("the row table's end")];
    assert!(
        !table.contains("\"Mesh shaders\"") && !menu.contains("opts.mesh_mode"),
        "the menu still has a Mesh shaders row"
    );
    assert!(
        !block(&menu, "const char* ini_key(").contains("\"mesh_shaders\""),
        "ini_key still names mesh_shaders: the game would write it"
    );
    let save = block(&menu, "void save() {");
    assert!(
        !save.contains("mine |= key == \"mesh_shaders\"")
            && !save.contains("fprintf(f, \"mesh_shaders"),
        "save() treats mesh_shaders as its own: an existing line would be dropped or rewritten"
    );
    assert!(
        save.contains("an old file's line still chooses"),
        "save() does not say why it keeps an old mesh_shaders= line"
    );
    let options = source("gpu/vk/vk_menu.h");
    assert!(
        !options.contains("mesh_mode"),
        "the options still have a mesh_mode field"
    );

    let dev = source("docs/development.md");
    assert!(
        dev.contains("RX_VK_MESH_SHADERS=native") && dev.contains("still chooses as its row did"),
        "docs/development.md does not document the switch and the old line"
    );
    assert!(
        source("readme.md").contains("RX_VK_MESH_SHADERS=native"),
        "readme.md does not name the launch switch"
    );
}

#[test]
fn the_rule_compiles_and_runs_game_free() {
    let header = source("gpu/vk/mesh_mode.h");
    let fixture = source("tests/test_mesh_mode_switch.cpp");
    for forbidden in [
        "fopen",
        "getenv",
        "romfs",
        "exefs",
        ".nup",
        "work/dump",
        "HOS_DUMP",
    ] {
        assert!(
            !header.contains(forbidden) && !fixture.contains(forbidden),
            "the mesh rule or its fixture reads {forbidden}: it must stay pure and game-free"
        );
    }
    assert_eq!(
        fixture.matches("#include").count(),
        4,
        "the fixture includes only the rule and the C++ library"
    );

    let Some(cxx) = compiler() else {
        eprintln!("skipping the compiled mesh rule: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-mesh-mode-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) { "mesh.exe" } else { "mesh" });
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", root().join("gpu/vk").display()))
        .arg(root().join("tests/test_mesh_mode_switch.cpp"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the mesh rule");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).starts_with("mesh mode:"));
}
