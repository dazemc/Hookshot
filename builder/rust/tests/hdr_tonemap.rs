// The Native HDR tone mapper (upstream ddb156df, PsychoV35 from RenoDX): the menu row and its saved option, the host's
// scene-buffer switch, the shader pass's wiring, and that the checks stay game-free. The real sources are read; nothing is
// compiled or run here.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// An enum's value, from `enum { NAME = value` in vk_api.h.
fn option(api: &str, name: &str) -> i32 {
    let at = api
        .find(name)
        .unwrap_or_else(|| panic!("{name} is not declared"));
    let (_, after) = api[at..]
        .split_once('=')
        .unwrap_or_else(|| panic!("{name} has no value"));
    after
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or_default()
        .parse()
        .unwrap_or_else(|_| panic!("{name} has no number"))
}

#[test]
fn native_hdr_tone_mapper_is_a_saved_menu_row() {
    let api = source("gpu/vk/vk_api.h");
    let options = source("gpu/vk/vk_menu.h");
    let menu = source("gpu/vk/vk_menu.cpp");
    let replay = source("gpu/vk/vk_replay.cpp");
    let host = source("host/nn_vi.c");

    assert_eq!(option(&api, "HOS_OPT_HDR_TONEMAP"), 47);
    assert!(api.contains("the scene buffer RGBA16F only then"));
    let ids = [
        option(&api, "HOS_OPT_HDR_NATIVE"),
        option(&api, "HOS_OPT_HDR_TONEMAP"),
        option(&api, "HOS_OPT_PROMPTS"),
        option(&api, "HOS_OPT_CAMERA_SPEED"),
    ];
    assert_eq!(
        ids.iter().collect::<BTreeSet<_>>().len(),
        ids.len(),
        "the HDR tone mapper must not alias another option"
    );

    assert!(options.contains("int hdr_tonemap = 0;"));
    assert!(options.contains("bool scene_tonemap_on();"));
    assert!(menu.contains(
        "const char* tx_tonemap(int v) { return v == 1 ? \"PsychoV35\" : \"Vanilla\"; }"
    ));
    assert!(menu.contains("\"HDR tone mapper\", &opts.hdr_tonemap, 0, 1, 1, tx_tonemap"));
    assert!(menu.contains("(v != &opts.hdr_tonemap || opts.hdr == 2)"));
    assert!(menu.contains(
        "bool scene_tonemap_on() { return dev.hdr && opts.hdr == 2 && opts.hdr_tonemap == 1; }"
    ));
    assert!(replay.contains("if (id == HOS_OPT_HDR_TONEMAP) return scene_tonemap_on();"));

    assert!(host.contains("volatile int hos_scene_rgba16f = 0;"));
    assert!(host.contains("hos_scene_rgba16f = rx_option(HOS_OPT_HDR_TONEMAP, 0) != 0;"));

    // (the option is an enum value: no MkwVkHost or MkwVkRenderer field changed, so the ABI version stays)
    assert!(api.contains("#define MKWVK_API_VERSION 19"));
    assert!(
        !api.contains("hdr_tonemap"),
        "an option value must not become an ABI field"
    );
}

#[test]
fn the_shader_pass_is_wired_to_translation_and_the_program_key() {
    let shader_h = source("gpu/vk/vk_shader.h");
    let shader = source("gpu/vk/vk_shader.cpp");
    let patches = source("builder/rust/src/oot_patches.rs");
    let python_patches = source("nx2/oot_patches.py");

    assert!(shader_h.contains("bool scene_tonemap = false;"));
    assert!(shader.contains("#include \"shader_recompiler/ir_opt/scene_tonemap_pass.h\""));
    assert!(shader.contains("#include \"vk_menu.h\""));
    assert!(shader.contains("bool scene_fit = false;"));
    assert!(
        shader.contains("static constexpr float kFit[] = {2.51f, 0.03f, 2.43f, 0.59f, 0.14f};")
    );
    assert!(shader.contains("bool scene_tonemap_for(const Code* fs)"));
    assert!(shader.contains("if (tonemap) key = hash_bytes(\"psychov35\", 9, key);"));
    assert!(shader.contains("Shader::Optimization::SceneToneMapPass(fs->program)"));
    assert!(shader.contains("p->scene_tonemap = tonemap;"));
    assert!(shader.contains("p->scene_tonemap = !mesh && scene_tonemap_for(fs);"));
    assert!(shader.contains("tonemap_seen"));
    assert!(shader.contains("picks.clear();"));
    assert!(
        shader
            .contains("graphics_key(*stage_code[0], stage_code[1], s.vattr, color_types, tonemap)")
    );

    assert!(patches.contains("SCENE_FORMAT_HEAD"));
    assert!(patches.contains("hos_scene_rgba16f ? 0x2cULL : 0x1aULL"));
    assert!(patches.contains("patch_scene_format(&out)"));
    assert!(patches.contains("\"scene format\""));
    // (the legacy Python builder keeps the same switch: the lift is game-derived either way)
    assert!(python_patches.contains("def patch_scene_format(s):"));
    assert!(python_patches.contains("hos_scene_rgba16f ? 0x2cULL : 0x1aULL"));

    let license = source("gpu/vk/eden/shader_recompiler/ir_opt/LICENSE-renodx.md");
    assert!(license.contains("MIT License"));
    assert!(license.contains("Carlos Lopez Jr."));
    let provenance = source("gpu/PROVENANCE.md");
    assert!(provenance.contains("RenoDX"));
    assert!(provenance.contains("psychov35.h"));
    assert!(provenance.contains("main+0x65234C"));
}

#[test]
fn the_tone_mapper_checks_need_no_game_data() {
    // (every file this feature adds or changes is text; none is a dump, a key, or made from one)
    for path in [
        "gpu/PROVENANCE.md",
        "gpu/vk/eden/shader_recompiler/ir_opt/LICENSE-renodx.md",
        "gpu/vk/eden/shader_recompiler/ir_opt/psychov35.h",
        "gpu/vk/eden/shader_recompiler/ir_opt/scene_tonemap_pass.h",
        "builder/rust/src/oot_patches.rs",
        "tests/scene_tonemap_ir.cpp",
        "tests/scene_tonemap_ir/CMakeLists.txt",
    ] {
        let text = source(path);
        assert!(!text.contains('\0'), "{path} is not text");
        for needle in [".nso", ".nsp", ".xci", ".z64", ".kip", "work/dump", "romfs"] {
            assert!(
                !text.contains(needle),
                "{path} names game data ({needle}): the checks must run without a dump"
            );
        }
    }

    // (the IR fixture builds its programs in memory: no file, no environment, no game header)
    let fixture = source("tests/scene_tonemap_ir.cpp");
    for call in [
        "fopen(",
        "std::ifstream",
        "std::ofstream",
        "read_to_string",
        "getenv(",
    ] {
        assert!(
            !fixture.contains(call),
            "the IR fixture must not read game data or the environment ({call})"
        );
    }
    assert!(!fixture.contains("work/"));
    assert!(!fixture.contains("hookshot_shaders.bin"));

    // (the fixture's own build directory holds only its CMake project)
    let dir = root().join("tests/scene_tonemap_ir");
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["CMakeLists.txt".to_string()]);
}
