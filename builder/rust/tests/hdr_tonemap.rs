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

    // (the option is an enum value: no MkwVkHost or MkwVkRenderer field changed; 20 is the display modes' screen_size)
    assert!(api.contains("#define MKWVK_API_VERSION 20"));
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

type M3 = [[f64; 3]; 3];

fn mul(m: M3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn matmul(a: M3, b: M3) -> M3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}

/// The floats of a `inline constexpr float name[..] = { ... };` in psychov35.h.
fn header_floats(header: &str, name: &str) -> Vec<f64> {
    let start = header
        .find(name)
        .unwrap_or_else(|| panic!("missing {name}"));
    let value = start + header[start..].find('=').expect("value") + 1;
    let end = value + header[value..].find(';').expect("array end");
    let body = &header[value..end];
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = body[at..].find(|c: char| c.is_ascii_digit() || c == '-') {
        let from = at + open;
        let token: String = body[from..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
            .collect();
        if let Ok(value) = token.trim_end_matches('f').parse::<f64>() {
            out.push(value);
        }
        at = from + token.len().max(1);
    }
    out
}

#[test]
fn the_tone_mappers_matrices_are_bt709s_own() {
    // (upstream's test_psychov35.py header checks: the reduced form's matrices are the BT.709, BT.2020 and cone
    // definitions, not tuned constants; a wrong matrix would still pass the pass-versus-header fixture)
    let header = source("gpu/vk/eden/shader_recompiler/ir_opt/psychov35.h");
    let bt709_to_xyz: M3 = [
        [0.4123907993, 0.3575843394, 0.1804807884],
        [0.2126390059, 0.7151686788, 0.0721923154],
        [0.0193308187, 0.1191947798, 0.9505321522],
    ];
    let xyz_to_bt709: M3 = [
        [3.2409699419, -1.5373831776, -0.4986107603],
        [-0.9692436363, 1.8759675015, 0.0415550574],
        [0.0556300797, -0.2039769589, 1.0569715142],
    ];
    let xyz_to_bt2020: M3 = [
        [1.7166511880, -0.3556707838, -0.2533662814],
        [-0.6666843518, 1.6164812366, 0.0157685458],
        [0.0176398574, -0.0427706133, 0.9421031212],
    ];
    let xyz_to_lms: M3 = [
        [0.2670502842655792, 0.8471990148492798, -0.03470416612462053],
        [-0.38706882411220156, 1.165429935890458, 0.10302286696614202],
        [0.026727793989083093, -0.02729131667566509, 0.5333267257603284],
    ];
    let det = dot(xyz_to_lms[0], cross(xyz_to_lms[1], xyz_to_lms[2]));
    let cols = [
        cross(xyz_to_lms[1], xyz_to_lms[2]),
        cross(xyz_to_lms[2], xyz_to_lms[0]),
        cross(xyz_to_lms[0], xyz_to_lms[1]),
    ];
    let mut lms_to_xyz = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            lms_to_xyz[i][j] = cols[j][i] / det;
        }
    }
    let bt709_to_lms = matmul(xyz_to_lms, bt709_to_xyz);
    let lms_to_bt709 = matmul(xyz_to_bt709, lms_to_xyz);
    let lms_to_bt2020 = matmul(xyz_to_bt2020, lms_to_xyz);
    let white = mul(bt709_to_lms, [1.0, 1.0, 1.0]);
    let mut ratio = [[0.0; 3]; 3];
    let mut to_2020 = [[0.0; 3]; 3];
    let mut to_709 = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            ratio[i][j] = bt709_to_lms[i][j] / (0.18 * white[i]);
            to_2020[i][j] = lms_to_bt2020[i][j] * white[j];
            to_709[i][j] = lms_to_bt709[i][j] * white[j];
        }
    }
    for (name, wanted) in [
        ("kToRatio[3][3]", ratio),
        ("kToBt2020[3][3]", to_2020),
        ("kToBt709[3][3]", to_709),
    ] {
        let got = header_floats(&header, name);
        let flat: Vec<f64> = wanted.iter().flatten().copied().collect();
        assert_eq!(got.len(), 9, "{name} is not a 3x3 matrix");
        for (got, want) in got.iter().zip(&flat) {
            assert!(
                (got - want).abs() <= 1e-7 * want.abs().max(1.0),
                "{name}: {got}, not {want}"
            );
        }
    }
    let luminance = header_floats(&header, "kLuminance[3]");
    assert_eq!(luminance, bt709_to_xyz[1], "kLuminance is not BT.709's Y row");

    // (the midgray's exposure is ACES(0.18) / 0.18, the game's own fit)
    let aces = |m: f64| (m * (2.51 * m + 0.03) / (m * (2.43 * m + 0.59) + 0.14)).clamp(0.0, 1.0);
    let exposure = header_floats(&header, "kReferenceExposure");
    assert!(
        (exposure[0] - aces(0.18) / 0.18).abs() < 1e-6,
        "kReferenceExposure is not ACES(0.18) / 0.18"
    );
    let hue = header_floats(&header, "kHueClip");
    assert_eq!(hue, [0.5]);
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
