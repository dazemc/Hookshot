// FSR 3.1 is the temporal chain's third engine (upstream NotRexed/Hookshot 579c6d48). These are game-free source
// checks: the chain takes the row's FSR 3.1 before its fallback to the game's own, the engine gets slot 2 and its own
// jitter gate, depth is reversed once for FSR's passes (and DLSS-G reads that), and the chain shares DLSS's reactive
// mask. The compiled temporal-jitter fixture is run too when a C++ compiler is present. No game, dump or GPU is needed.

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

fn block(text: &str, marker: &str) -> String {
    function(text, marker)
}

fn index(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"))
}

#[test]
fn fsr3_is_the_temporal_chains_third_engine() {
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let temporal_h = read("gpu/vk/vk_temporal.h");
    let fsr3 = read("gpu/vk/vk_fsr3.cpp");
    let fsr3_h = read("gpu/vk/vk_fsr3.h");

    // The row's FSR 3.1 runs on the chain before the chain falls back to the game's own; FSR 3.1's own inputs are off
    // then, so there is one upscaler and one jitter.
    let mode = function(&temporal, "int temporal_mode()");
    let on_chain = index(
        &mode,
        "if (fsr3_on_chain() && fsr3_chosen()) return ur::FSR3;",
    );
    let not_an_engine = index(
        &mode,
        "if (!ur::is_dlss(v) && !ur::is_fsr4(v)) return ur::GAME;",
    );
    let own_inputs = index(&mode, "if (fsr3_wanted()) return ur::GAME;");
    assert!(
        on_chain < not_an_engine && not_an_engine < own_inputs,
        "temporal_mode must return FSR3 before its GAME fallbacks"
    );
    assert!(mode.contains("return ur::FSR3;"));

    // Slot 2 is FSR 3.1's, in the engine table, engine_of and the jitter sample alike.
    assert!(temporal.contains("Engine engines[3];"));
    let engine_of = function(&temporal, "Engine& engine_of(int mode)");
    assert!(engine_of.contains("mode == ur::FSR3 ? 2"));
    assert!(engine_of.contains("fsr3_engine()"));
    let name_of = function(&temporal, "const char* name_of(int mode)");
    assert!(name_of.contains("case ur::FSR3: return \"AMD FSR 3.1\";"));

    // RX_VK_FSR3_CHAIN=0 keeps the old own-inputs path; fsr3_wanted is its question, fsr3_chosen the row/switch one.
    let chosen = function(&fsr3, "bool fsr3_chosen()");
    assert!(
        chosen.contains("temporal_env_chosen()"),
        "a switch over the row must still win"
    );
    let chain = function(&fsr3, "bool fsr3_on_chain()");
    assert!(chain.contains("env_switch(\"RX_VK_FSR3_CHAIN\") != 0"));
    let wanted = function(&fsr3, "bool fsr3_wanted()");
    assert!(wanted.contains("!fsr3_on_chain() && fsr3_chosen()"));

    // FSR 3.1's images are made and freed by the chain, and its picture is copied into the chain's output after the
    // passes and before the chain's own output pass.
    for name in [
        "prepare_sizes(",
        "release();",
        "memory();",
        "fsr3_chosen();",
        "fsr3_on_chain();",
        "fsr3_engine();",
    ] {
        assert!(fsr3_h.contains(name), "vk_fsr3.h does not declare {name}");
    }
    let prepare = function(&fsr3, "int chain_prepare(");
    assert!(prepare.contains("fsr3::prepare_sizes(rw, rh, ow, oh)"));
    let evaluate = function(&fsr3, "bool chain_evaluate(");
    let mask = index(&evaluate, "reactive::mask(");
    let upscale = index(&evaluate, "fsr3::upscale(cmd, ai)");
    let copy = index(&evaluate, "vkCmdCopyImage(cmd, picture");
    assert!(
        mask < upscale && upscale < copy,
        "the chain evaluates and copies FSR 3.1 in order"
    );
    assert!(evaluate.contains("ai.reactive = m.view;"));

    // A failed evaluation costs this frame only: the next frame starts a new history instead of turning FSR off.
    let engine = fsr3
        .split_once("const TemporalEngine kChainEngine")
        .expect("kChainEngine")
        .1;
    assert!(engine.contains(".fsr_depth = true") && engine.contains(".retry = true"));
    assert!(temporal_h.contains("bool fsr_depth = false;"));
    assert!(temporal_h.contains("bool retry = false;"));
    let upscale_fn = function(&temporal, "bool temporal_upscale(");
    let retry = index(&upscale_fn, "if (engine.e->retry)");
    let failed = upscale_fn[retry..]
        .find("engine.failed = true;")
        .map(|at| at + retry)
        .expect("the non-retrying failure path");
    assert!(
        retry < failed,
        "a retrying engine is not marked failed for good"
    );
}

#[test]
fn fsr3_depth_is_reversed_once_and_frame_generation_follows_it() {
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let temporal_h = read("gpu/vk/vk_temporal.h");
    let shader = read("gpu/vk/shaders/dlss_prep.comp");
    let dlss = read("gpu/vk/vk_dlss.cpp");
    let draw = read("gpu/vk/vk_draw.cpp");
    let fsr3 = read("gpu/vk/vk_fsr3.cpp");
    let jitter_test = read("tests/temporal_jitter_test.cpp");

    // The chain builds FSR's depth constants from the projection without this frame's jitter, reverses the depth it
    // prepares, and tells every later reader through TemporalFrame::depth_reversed.
    let upscale = function(&temporal, "bool temporal_upscale(");
    let unjitter = index(&upscale, "dlss_math::unjitter(projection, cam.jitter)");
    let fsr_depth = index(&upscale, "if (engine.e->fsr_depth)");
    let flip = index(&upscale, "fsr_flip ? 8 : 0");
    let frame = index(&upscale, "f.depth_reversed = fsr_flip;");
    assert!(unjitter < fsr_depth && fsr_depth < flip && flip < frame);
    assert!(upscale.contains("fsr3_math::camera_from_projection(projection, fc)"));
    assert!(
        upscale.contains("VK_IMAGE_USAGE_TRANSFER_DST_BIT"),
        "the chain output accepts FSR's copy"
    );
    assert!(
        fsr3.contains("VK_IMAGE_USAGE_TRANSFER_SRC_BIT"),
        "FSR's picture can be copied out"
    );
    assert!(temporal_h.contains("float projection[16]{};"));
    assert!(temporal_h.contains("uint32_t jitter_phases = 8;"));
    assert!(temporal_h.contains("bool depth_reversed = false;"));

    // The prep shader does the reversal once, and scrubs a NaN before the colour maths.
    assert!(shader.contains("(pc.flags & 8) != 0 ? 1.0 - d : d"));
    let nan = index(&shader, "isnan(e)");
    let square = index(&shader, "kSquare ? e * e : e");
    assert!(nan < square);

    // DLSS Frame Generation is given the inverted depth NGX expects, not a hard-coded false.
    let inputs = function(&dlss, "void dlss_fg_inputs(");
    assert!(inputs.contains("g.depth_reversed = f.depth_reversed;"));
    let frames = function(&dlss, "int dlss_fg_frames(");
    assert!(frames.contains("o.depthInverted = g.depth_reversed;"));

    // FSR 3.1 on the chain has its own jitter switch, and the jitter sample uses slot 2's failure gate.
    let sample = function(&temporal, "MkwVkJitter temporal_jitter_sample()");
    assert!(sample.contains("env_switch(\"RX_VK_FSR_JITTER\") == 0"));
    assert!(sample.contains("mode == ur::FSR3 && fsr3_off"));
    assert!(sample.contains("mode == ur::FSR3 ? 2 : 0"));

    // The network log names FSR 3.1 when the chain draws it.
    assert!(draw.contains("FSR 3.1 (on the temporal chain)"));

    // The compiled fixture covers FSR 3.1's slot, its gate and RX_VK_FSR_JITTER=0 alone.
    assert!(jitter_test.contains("engines[3]"));
    assert!(jitter_test.contains("fsr-jitter-off"));
    assert!(jitter_test.contains("gpu::state.engines[2].failed"));
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
fn temporal_jitter_fixture_covers_fsr3_on_the_chain() {
    let Some(cxx) = compiler() else {
        eprintln!(
            "skipping the compiled temporal-jitter fixture: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let repo = root();
    let dlss = read("gpu/vk/vk_temporal.cpp");
    let fsr = read("gpu/vk/vk_fsr3_inputs.cpp");
    let replay = read("gpu/vk/vk_replay.cpp");
    let host = read("host/rx_backend.c");
    let mut fixture = read("tests/temporal_jitter_test.cpp");

    // The fixture is the actual publishers, readers and replay/host transport, as upstream's Python test assembled it:
    // the real temporal_jitter_sample, its failure gate (slot 2) and the FSR 3.1 own-inputs reader are compiled in.
    let host_lock = block(&host, "int hos_dlss_jitter_clip(").contains("AcquireSRWLockExclusive");
    fixture = fixture.replace(
        "/* HOST_CACHE_LOCKED */",
        &format!(
            "#define HOST_CACHE_LOCKED {}",
            if host_lock { 1 } else { 0 }
        ),
    );
    let reader = format!(
        "{}\n{}",
        block(&dlss, "MkwVkJitter temporal_jitter_sample()"),
        block(&dlss, "uint32_t temporal_jitter_size(int axis)")
    );
    let publish = regex::Regex::new(r"s\.jitter\.publish\([^;]+;")
        .unwrap()
        .find(&dlss)
        .expect("temporal_upscale publishes the jitter")
        .as_str()
        .to_string();
    let fsr_reader = format!(
        "{}\n{}",
        block(&fsr, "MkwVkJitter jitter_sample()"),
        block(&fsr, "uint32_t jitter_size(int axis)")
    );
    let callback = block(&replay, "void jitter_sample(MkwVkJitter* out)");
    assert!(
        block(&replay, "void fill_replay_table(").contains("t.jitter_sample = jitter_sample;"),
        "the public table must install the callback the host consumes"
    );
    for (marker, value) in [
        ("/* DLSS_READER */", reader),
        ("/* DLSS_PUBLISH */", publish),
        ("/* FSR_READER */", fsr_reader),
        ("/* FSR_PUBLISH */", block(&fsr, "void upscaled(")),
        ("/* REPLAY_CALLBACK */", callback),
        (
            "/* HOST_TRANSPORT */",
            block(&host, "int hos_dlss_jitter_clip("),
        ),
    ] {
        assert!(fixture.contains(marker), "fixture marker {marker}");
        fixture = fixture.replace(marker, &value);
    }

    let dir = std::env::temp_dir().join(format!(
        "hookshot-fsr3-chain-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let source = dir.join("jitter.cpp");
    let exe = dir.join(if cfg!(windows) {
        "jitter.exe"
    } else {
        "jitter"
    });
    fs::write(&source, &fixture).expect("write fixture");
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
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .current_dir(&dir)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", source.display());
    for args in [
        Vec::new(),
        vec!["off"],
        vec!["fsr-jitter-off"],
        vec!["parallel-host"],
        vec!["late-host"],
    ] {
        let run = Command::new(&exe)
            .args(&args)
            .output()
            .expect("run the temporal-jitter fixture");
        assert!(
            run.status.success(),
            "{} {}: {}",
            exe.display(),
            args.join(" "),
            String::from_utf8_lossy(&run.stderr)
        );
    }
    let _ = fs::remove_dir_all(dir);
}
