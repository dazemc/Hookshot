// FSR 3.1 on the temporal chain takes the game's native HDR colour, and the motion/depth inputs it shares with DLSS and
// FSR 4. These are game-free checks: the source wiring, the actual `chain_hdr` switch, the actual `bones_used` reader and
// the actual `temporal_mode` fallback, compiled and run from the renderer's own text. No game, dump or GPU is needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// A C/C++ definition's braces, matched, from its first line.
fn function(text: &str, marker: &str) -> String {
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

fn index(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"))
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

/// Fill a fixture's markers from the renderer's source, compile it and run it. The returned output is checked by the
/// caller; `None` means no C++ compiler is here (the test says it skipped).
fn fixture(
    name: &str,
    source: &str,
    replacements: &[(&str, String)],
    args: &[&str],
) -> Option<std::process::Output> {
    let cxx = compiler()?;
    let mut text = read(source);
    for (marker, value) in replacements {
        assert!(text.contains(marker), "{source}: fixture marker {marker}");
        text = text.replace(marker, value);
    }
    let dir = scratch(name);
    let file = dir.join("fixture.cpp");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    fs::write(&file, &text).expect("write fixture");
    let repo = root();
    let build = Command::new(&cxx)
        .args([
            "-std=c++20",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Wno-missing-field-initializers",
            "-pthread",
        ])
        .arg(format!("-I{}", repo.display()))
        .arg(&file)
        .arg("-o")
        .arg(&exe)
        .current_dir(&dir)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", file.display());
    let run = Command::new(&exe)
        .args(args)
        .output()
        .expect("run the fixture");
    let _ = fs::remove_dir_all(dir);
    Some(run)
}

#[test]
fn fsr3_chain_takes_the_games_native_hdr_colour() {
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let temporal_h = read("gpu/vk/vk_temporal.h");
    let fsr3 = read("gpu/vk/vk_fsr3.cpp");
    let fsr3_h = read("gpu/vk/vk_fsr3.h");
    let shaders = read("scripts/fsr3_shaders.py");
    let development = read("docs/development.md");

    // The engine says it takes HDR colour only while the game presents its own PQ picture, and the chain then chooses its
    // HDR prep/out pair for every engine that says so, FSR 3.1 included.
    let hdr = function(&fsr3, "bool chain_hdr()");
    assert!(hdr.contains("env_switch(\"RX_VK_FSR3_HDR\") != 0"));
    assert!(hdr.contains("present_native_pq.load(std::memory_order_relaxed)"));
    let engine = fsr3
        .split_once("const TemporalEngine kChainEngine")
        .expect("kChainEngine")
        .1;
    assert!(
        engine.contains(".hdr = chain_hdr"),
        "FSR 3.1's engine does not take HDR colour"
    );
    let upscale = function(&temporal, "bool temporal_upscale(");
    assert!(upscale.contains("engine.e->hdr && engine.e->hdr()"));
    let select = index(
        &upscale,
        "const VkPipeline prep_pipe = hdr ? s.prep_hdr : s.prep",
    );
    let out = index(&upscale, "out_pipe = hdr ? s.out_hdr : s.out");
    assert!(
        select < out,
        "the HDR pair must be selected before the passes are recorded"
    );
    assert!(upscale.contains("const bool hdr = engine.e->hdr && engine.e->hdr();"));

    // The chain's HDR prep squares the square-root encoded scene into linear light and does not clamp at 1; its out pass
    // writes the engine's linear picture as it is. The vendored FSR 3.1 passes are compiled for linear HDR colour.
    let pipelines = function(&temporal, "bool make_pipelines()");
    assert!(pipelines.contains("static const PrepSpec prep_hdr_data{16384.0f, VK_TRUE};"));
    assert!(pipelines.contains("static const VkBool32 out_hdr_square = VK_FALSE;"));
    assert!(shaders.contains("-DFFX_FSR3UPSCALER_OPTION_HDR_COLOR_INPUT=1"));
    assert!(fsr3_h.contains("linear light when the game presents its"));
    assert!(temporal_h.contains("bool (*hdr)() = nullptr;"));

    // A colour-space change starts a new history: the old one was accumulated in the other space.
    let evaluate = function(&fsr3, "bool chain_evaluate(");
    let hdr_now = index(&evaluate, "const bool hdr = chain_hdr();");
    let changed = index(
        &evaluate,
        "const bool hdr_changed = c.told_hdr && c.hdr != hdr;",
    );
    let reset = index(&evaluate, "ai.reset = f.reset || hdr_changed;");
    assert!(hdr_now < changed && changed < reset);
    assert!(evaluate.contains("the game's native HDR turned on or off: a new history"));

    // The switch is documented with the other engines' HDR comparisons.
    assert!(development.contains("RX_VK_FSR3_HDR=0"));
}

#[test]
fn fsr3_hdr_switch_fixture() {
    let fsr3 = read("gpu/vk/vk_fsr3.cpp");
    let replacements = [
        (
            "/* ENV_SWITCH */",
            function(&fsr3, "int env_switch(const char* name)"),
        ),
        ("/* CHAIN_HDR */", function(&fsr3, "bool chain_hdr()")),
    ];
    let Some(run) = fixture("fsr3-hdr", "tests/fsr3_hdr_test.cpp", &replacements, &[]) else {
        eprintln!("skipping the FSR 3.1 HDR fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "the HDR fixture failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    // The switch is read once, at the first ask: a second process with RX_VK_FSR3_HDR=0 must keep the LDR pair even with
    // Native HDR. (The fixture caches it, as the renderer does, so one process cannot test both.)
    let cxx = compiler().expect("the fixture ran, so a compiler is here");
    let dir = scratch("fsr3-hdr-env");
    let file = dir.join("fixture.cpp");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut text = read("tests/fsr3_hdr_test.cpp");
    for (marker, value) in replacements {
        text = text.replace(marker, &value);
    }
    fs::write(&file, text).expect("write fixture");
    let built = Command::new(&cxx)
        .args([
            "-std=c++20",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Wno-missing-field-initializers",
        ])
        .arg(format!("-I{}", root().display()))
        .arg(&file)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C++ compiler");
    assert!(built.success());
    let run = Command::new(&exe)
        .env("RX_VK_FSR3_HDR", "0")
        .arg("native-on")
        .output()
        .expect("run the fixture with RX_VK_FSR3_HDR=0");
    assert!(
        run.status.success(),
        "RX_VK_FSR3_HDR=0 did not keep the LDR pair: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn fsr3_motion_inputs_fixture() {
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let draw = read("gpu/vk/vk_draw.cpp");
    let actor = read("gpu/vk/dlss_actor.h");
    let shader = read("gpu/vk/shaders/dlss_actor.vert");
    let replacements = [(
        "/* BONES_USED */",
        function(&temporal, "uint32_t bones_used("),
    )];
    let Some(run) = fixture(
        "fsr3-motion",
        "tests/fsr3_motion_test.cpp",
        &replacements,
        &[],
    ) else {
        eprintln!("skipping the FSR 3.1 motion fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "the motion fixture failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    // The skin check maps the other layouts the chain's actor motion reads: one influence in a second set, two, and a
    // rigid model on one bone. dlss_actor.h classifies them and counts the bones; vk_temporal delegates to it; the shader
    // reads a set's influences at any byte offset.
    assert!(draw.contains("dlss_actor::kR8"));
    assert!(draw.contains("dlss_actor::kRG8UI"));
    assert!(draw.contains("dlss_actor::classify(sa)"));
    assert!(temporal.contains("dlss_actor::bones_used(v.data()"));
    assert!(actor.contains("constexpr uint32_t kSingle = 8;"));
    assert!(actor.contains("constexpr uint32_t kPair = 32;"));
    assert!(actor.contains("constexpr uint32_t kNoWeight = 128;"));
    assert!(actor.contains("inline SkinLayout classify("));
    assert!(actor.contains("inline int influences("));
    assert!(actor.contains("inline uint32_t bones_used("));
    assert!(shader.contains("uint byte_at(Words vb, uint o)"));
    assert!(shader.contains("const bool byte_joints = (pc.formats & (2u << s)) != 0u"));
}

#[test]
fn fsr3_fallback_fixture() {
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let replacements = [(
        "/* TEMPORAL_MODE */",
        function(&temporal, "int temporal_mode()"),
    )];
    let Some(run) = fixture(
        "fsr3-fallback",
        "tests/fsr3_fallback_test.cpp",
        &replacements,
        &[],
    ) else {
        eprintln!("skipping the FSR 3.1 fallback fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "the fallback fixture failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn dlss_actor_shader_matches_its_source() {
    let header = read("gpu/vk/shaders/dlss_actor_vert_spv.h");
    assert!(header.contains("DLSS_ACTOR_VERT_SPV"));
    // (the header's first line names the glslang that made it: 16.6.0, as the other checked-in shaders)
    assert!(
        header.contains("1116.6.0"),
        "dlss_actor.vert's header was not regenerated with glslang 16.6.0"
    );
    let glslang = std::env::var("GLSLANG")
        .ok()
        .into_iter()
        .chain(["glslangValidator".into(), "glslang".into()])
        .find(|candidate| {
            Command::new(candidate)
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        });
    let Some(glslang) = glslang else {
        eprintln!("skipping the dlss_actor.vert SPIR-V check: glslangValidator not found");
        return;
    };
    let dir = scratch("dlss-actor-spv");
    let made = dir.join("dlss_actor_vert_spv.h");
    let built = Command::new(&glslang)
        .args([
            "-V",
            "--target-env",
            "vulkan1.3",
            "--vn",
            "DLSS_ACTOR_VERT_SPV",
            "-o",
        ])
        .arg(&made)
        .arg(root().join("gpu/vk/shaders/dlss_actor.vert"))
        .status()
        .expect("run glslangValidator");
    assert!(built.success());
    let word = regex::Regex::new(r"0x[0-9a-fA-F]{8}").unwrap();
    let words = |text: &str| -> Vec<String> {
        word.find_iter(text)
            .map(|m| m.as_str().to_ascii_lowercase())
            .collect()
    };
    let made_text = fs::read_to_string(&made).expect("read generated header");
    assert_eq!(
        words(&made_text),
        words(&header),
        "dlss_actor.vert's SPIR-V header is stale"
    );
    let _ = fs::remove_dir_all(dir);
}
