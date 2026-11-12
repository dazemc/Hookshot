// The temporal chain's actor motion for every bone layout OoT draws (upstream NotRexed/Hookshot dfc8fa5e): four
// influences, two (RG8 / RG8UI), one in a second set (R8 / R8UI or R16UI), and a rigid model on one bone (a joint and no
// weight). These are game-free source checks, and the compiled fixture covers the classification, the weights and the
// motion with authored vertices and bone palettes. A camera cut starts a new upscaler history too (upstream 7a0c89f0):
// its source check and the compiled math fixture are here as well. No game, dump or GPU is needed.

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

fn glslang() -> Option<String> {
    let candidates = std::env::var("GLSLANG")
        .ok()
        .into_iter()
        .chain(["glslangValidator".into(), "glslang".into()]);
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

/// The glslang version a --vn header names on its first line (`// 1116.6.0`), or None.
fn generator(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("// "))
}

/// The SPIR-V words of a glslang --vn header or of a binary header's text.
fn words(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 9 < bytes.len() {
        if bytes[i] == b'0'
            && bytes[i + 1] == b'x'
            && (2..10).all(|k| bytes[i + k].is_ascii_hexdigit())
        {
            out.push(text[i..i + 10].to_ascii_lowercase());
            i += 10;
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn actor_motion_has_every_bone_layout() {
    let header = read("gpu/vk/dlss_actor.h");
    let draw = read("gpu/vk/vk_draw.cpp");
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let shader = read("gpu/vk/shaders/dlss_actor.vert");

    // The formats bits are the shader's and the CPU's: one influence, a pair, and a joint without a weight.
    for bit in [
        "constexpr uint32_t kSingle = 8;",
        "constexpr uint32_t kPair = 32;",
        "constexpr uint32_t kNoWeight = 128;",
    ] {
        assert!(header.contains(bit), "dlss_actor.h lacks {bit}");
    }

    // The classification reads the draw's formats: a second set's one influence, a rigid one-bone model, a pair, and
    // the four-influence sets the path had before.
    let classify = function(&header, "inline SkinLayout classify(");
    assert!(classify.contains("set == 1 && w == kR8"));
    assert!(classify.contains("j == kR8UI || j == kR16UI"));
    assert!(classify.contains("set == 0 && w == kNone"));
    assert!(classify.contains("(kSingle | kNoWeight) << set"));
    assert!(classify.contains("w == kRG8 && j == kRG8UI"));
    assert!(classify.contains("w != kRGBA8 || (j != kRGBA16UI && j != kRGBA8UI)"));
    assert!(classify.contains("a.weight_offset[set] % 4 || a.joint_offset[set] % 4"));

    // A weight is its byte / 255; a set without weights weighs its joint 1; one, two or four influences are read.
    let influences = function(&header, "inline int influences(");
    assert!(
        influences
            .contains("(k.formats & (kSingle << set)) ? 1 : (k.formats & (kPair << set)) ? 2 : 4")
    );
    assert!(influences.contains("(float)vertex[k.weights[set] + i] / 255.0f"));
    assert!(influences.contains("no_weight ? 1.0f"));
    assert!(influences.contains("byte_joints"));
    let bones = function(&header, "inline uint32_t bones_used(");
    assert!(bones.contains("influences(q, k, set, joints, weights)"));
    assert!(bones.contains("if (weights[j] > 0.0f)"));

    // vk_draw.cpp's skin_check maps NVN's formats to the classification, and vk_temporal.cpp's bones_used reads the
    // classified layout instead of four joints a set.
    let skin = function(&draw, "static void skin_check(");
    assert!(skin.contains("dlss_actor::classify(sa)"));
    for format in [
        "NVN_FORMAT_R8",
        "NVN_FORMAT_R8UI",
        "NVN_FORMAT_R16UI",
        "NVN_FORMAT_RG8",
        "NVN_FORMAT_RG8UI",
        "NVN_FORMAT_RGBA8",
        "NVN_FORMAT_RGBA8UI",
        "NVN_FORMAT_RGBA16UI",
    ] {
        assert!(skin.contains(format), "skin_check does not map {format}");
    }
    assert!(temporal.contains("return dlss_actor::bones_used("));
    assert!(!temporal.contains("for (int j = 0; j < 4; j++)"));

    // The shader reads a set's influences at any byte offset, one, two or four, and weighs a rigid joint 1.
    for token in [
        "8u << s",
        "32u << s",
        "128u << s",
        "byte_at(vb, pc.joints[s] + k)",
        "short_at(vb, pc.joints[s] + 2u * k)",
        "/ 255.0",
        "no_weight ? 1.0",
    ] {
        assert!(shader.contains(token), "dlss_actor.vert lacks {token}");
    }
}

#[test]
fn actor_motion_fixture_covers_classification_weights_and_motion() {
    let Some(cxx) = compiler() else {
        eprintln!(
            "skipping the compiled actor-motion fixture: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let repo = root();
    let dir = std::env::temp_dir().join(format!(
        "hookshot-actor-motion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) { "actor.exe" } else { "actor" });
    let source = repo.join("tests/test_dlss_actor.cpp");
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", repo.display()))
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .current_dir(&dir)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", source.display());
    let run = Command::new(&exe)
        .output()
        .expect("run the actor-motion fixture");
    assert!(
        run.status.success(),
        "{}: {}",
        exe.display(),
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn camera_cut_starts_a_new_upscaler_history() {
    let math = read("gpu/vk/dlss_math.h");
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let pose = function(&math, "inline bool camera_pose(");
    assert!(pose.contains("std::isfinite"));
    assert!(pose.contains("Gauss-Jordan"));
    let cut = function(&math, "inline bool camera_cut(");
    for token in [
        "*move > 5.0 * k",
        "*turn > 30.0 * k",
        "*move > 0.5 * k",
        "k < 0.25 ? 0.25",
        "k > 4 ? 4",
    ] {
        assert!(cut.contains(token), "camera_cut lacks {token}");
    }
    assert!(temporal.contains("dlss_math::camera_cut(s.last_vp, cam.current, ms, &move, &turn)"));
    assert!(temporal.contains("RX_VK_TEMPORAL_CUT"));
    assert!(temporal.contains("std::chrono::steady_clock::time_point last_at"));
}

#[test]
fn camera_cut_fixture_runs() {
    let Some(cxx) = compiler() else {
        eprintln!(
            "skipping the compiled camera-cut fixture: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let repo = root();
    let dir = std::env::temp_dir().join(format!(
        "hookshot-camera-cut-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) { "math.exe" } else { "math" });
    let source = repo.join("tests/test_dlss_math.cpp");
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", repo.display()))
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .current_dir(&dir)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", source.display());
    let run = Command::new(&exe)
        .output()
        .expect("run the camera-cut fixture");
    assert!(
        run.status.success(),
        "{}: {}",
        exe.display(),
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_actor_vertex_spirv_header_is_made_from_its_shader() {
    let Some(glslang) = glslang() else {
        eprintln!("skipping the SPIR-V check: no glslangValidator or glslang");
        return;
    };
    let repo = root();
    let dir = std::env::temp_dir().join(format!(
        "hookshot-actor-spirv-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let out = dir.join("dlss_actor_vert_spv.h");
    let build = Command::new(&glslang)
        .args([
            "-V",
            "--quiet",
            "--target-env",
            "vulkan1.3",
            "--vn",
            "DLSS_ACTOR_VERT_SPV",
            "-o",
        ])
        .arg(&out)
        .arg(repo.join("gpu/vk/shaders/dlss_actor.vert"))
        .status()
        .expect("run glslangValidator");
    assert!(build.success(), "{glslang} failed on dlss_actor.vert");
    let made = fs::read_to_string(&out).expect("glslang's header");
    let checked = read("gpu/vk/shaders/dlss_actor_vert_spv.h");
    if generator(&made) != generator(&checked) {
        eprintln!(
            "skipping the SPIR-V comparison: this glslang is {}, the checked-in header {} (another glslang may emit \
             equivalent but different SPIR-V)",
            generator(&made).unwrap_or("unknown"),
            generator(&checked).unwrap_or("unknown")
        );
        let _ = fs::remove_dir_all(dir);
        return;
    }
    assert_eq!(
        words(&made),
        words(&checked),
        "gpu/vk/shaders/dlss_actor_vert_spv.h is not made from dlss_actor.vert as it is now (glslangValidator -V \
         --target-env vulkan1.3 --vn DLSS_ACTOR_VERT_SPV -o dlss_actor_vert_spv.h dlss_actor.vert)"
    );
    let _ = fs::remove_dir_all(dir);
}
