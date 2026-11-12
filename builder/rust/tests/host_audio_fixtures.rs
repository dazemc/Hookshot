// The audio renderer's Aux send/return and its per-voice and per-splitter biquad history, against authored signals
// (host/nn_audio.c; upstream NotRexed/Hookshot 40c501ab). No game, GPU or audio device: the real HLE entry points, mixer
// and DSP code are cut out of the source with the same boundaries the upstream Python checks used, compiled, and run with
// synthetic memory and a worker that applies a known gain. Skips when there is no C compiler.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiler() -> Option<String> {
    let candidates =
        std::env::var("CC")
            .ok()
            .into_iter()
            .chain(["cc".into(), "gcc".into(), "clang".into()]);
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

/// A function's definition from the source, braces matched, as the upstream checks cut them.
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

fn slice(source: &str, from: &str, to: &str) -> String {
    let start = source
        .find(from)
        .unwrap_or_else(|| panic!("missing {from}"));
    let end = source[start..]
        .find(to)
        .unwrap_or_else(|| panic!("missing {to} after {from}"))
        + start;
    source[start..end].to_string()
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

fn run(cc: &str, args: &[String], what: &str) {
    let built = Command::new(cc)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("run {cc}: {e}"));
    assert!(
        built.status.success(),
        "{what}: {cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
}

#[test]
fn aux_buffers_reach_the_games_workers_and_underruns_are_silent() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the Aux fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/nn_audio.c")).expect("host/nn_audio.c");
    let size_api = function(
        &source,
        "HLE(_ZN2nn5audio43GetRequiredBufferSizeForAuxSendReturnBuffer",
    );
    let dir = scratch("audio-aux");
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };

    // (the size check runs first, so an unmodified source fails behaviorally before missing new APIs matter)
    let sizing = dir.join("sizing.c");
    fs::write(
        &sizing,
        format!(
            "#include <stdint.h>\n#include <stdio.h>\ntypedef struct {{ uint64_t x[8]; }} Ctx;\n\
             #define HLE(name) void size_api(Ctx *c)\n#define ARG(n) c->x[n]\n\
             #define RET(v) (c->x[0] = (uint64_t)(v))\n#define G32(p) (*(uint32_t *)(uintptr_t)(p))\n\
             {size_api}\n\
             int main(void) {{\n    uint32_t parameter[] = {{48000, 240}};\n\
             \x20   Ctx c = {{(uintptr_t)parameter, 6, 5}};\n    size_api(&c);\n\
             \x20   if (c.x[0] != 28928) {{\n\
             \x20       fprintf(stderr, \"FAIL: expected 28928 bytes for 6 channels x 5 frames, got %llu\\n\",\n\
             \x20               (unsigned long long)c.x[0]);\n        return 1;\n    }}\n    return 0;\n}}\n"
        ),
    )
    .expect("write sizing fixture");
    let size_exe = dir.join(format!("sizing{exe_suffix}"));
    run(
        &cc,
        &[
            "-std=c11".into(),
            sizing.display().to_string(),
            "-o".into(),
            size_exe.display().to_string(),
        ],
        "building the Aux size fixture",
    );
    let sized = Command::new(&size_exe)
        .output()
        .expect("run the Aux size fixture");
    assert!(
        sized.status.success(),
        "{}",
        String::from_utf8_lossy(&sized.stderr)
    );

    let mut chunks = vec![
        slice(&source, "#define FRAME", "/* ---- wave buffers"),
        function(&source, "static void release_wb("),
        slice(&source, "/* ---- I3DL2 reverb (", "/* ---- the mixer"),
        slice(
            &source,
            "/* ---- parameter updates",
            "/* The mixer's next deadline.",
        ),
        size_api,
    ];
    // (also accepts the old stubs, so the first size and roundtrip checks show the original failure)
    let aux_start = if source.contains("/* ---- Aux API") {
        source.find("/* ---- Aux API").unwrap()
    } else {
        source
            .find("/* ---- effects: accepted")
            .expect("an Aux section")
    };
    let aux_end = source[aux_start..]
        .find("HLE(_ZN2nn5audio17IsReverbRemovable")
        .expect("the Aux section's end")
        + aux_start;
    chunks.push(source[aux_start..aux_end].to_string());
    chunks.push(slice(
        &source,
        "static Reverb *reverb_of(",
        "static void ret_float(",
    ));
    fs::write(dir.join("audio_aux.inc"), chunks.join("\n")).expect("write audio_aux.inc");

    let exe = dir.join(format!("audio_aux{exe_suffix}"));
    let mut args: Vec<String> = [
        "-std=c11",
        "-O1",
        "-g",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-Wno-unused-function",
        "-Wno-unused-variable",
        "-Wno-unused-parameter",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("-I{}", dir.display()));
    // (env_on, the switches' rule: the cut-out reverb reads HOS_AUDIO_NO_REVERB with it)
    args.push("-include".into());
    args.push(
        repo.join("gpu/vk/compat/hookshot_env.h")
            .display()
            .to_string(),
    );
    args.push(repo.join("tests/audio_aux_test.c").display().to_string());
    if !cfg!(windows) {
        args.push("-lm".into());
    }
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the Aux fixture");
    let ran = Command::new(&exe).output().expect("run the Aux fixture");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn splitter_filter_history_is_per_voice_and_destination() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the biquad fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/nn_audio.c")).expect("host/nn_audio.c");
    let mut chunks = vec![
        slice(&source, "#define FRAME", "static int mix_depth("),
        slice(
            &source,
            "/* ---- parameter updates",
            "/* The mixer's next deadline.",
        ),
        slice(&source, "/* ---- splitters ", "/* ---- the device sink:"),
        function(
            &source,
            "HLE(_ZN2nn5audio29SetVoiceBiquadFilterParameterEPNS0_9VoiceTypeEiRKNS0_21BiquadFilterParameterE)",
        ),
    ];
    let voice = "_ZN2nn5audio29SetVoiceBiquadFilterParameterEPNS0_9VoiceTypeEiRKNS0_22TBiquadFilterParameterIfEE";
    chunks.push(if source.contains(&format!("HLE({voice})")) {
        function(&source, &format!("HLE({voice})"))
    } else {
        format!("HLE({voice}) {{}}")
    });
    for mix in ["10SubMixType", "12FinalMixType"] {
        let name = format!(
            "_ZN2nn5audio32SetSplitterBiquadFilterParameterEPNS0_12SplitterTypeEiPNS0_{mix}EiRKNS0_22TBiquadFilterParameterIfEE"
        );
        if !source.contains(&format!("HLE({name})")) {
            chunks.push(format!("HLE({name}) {{}}"));
        }
    }

    let dir = scratch("audio-biquad");
    fs::write(dir.join("audio_biquad.inc"), chunks.join("\n")).expect("write audio_biquad.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_biquad.exe"
    } else {
        "audio_biquad"
    });
    let mut args: Vec<String> = [
        "-std=c11",
        "-O1",
        "-g",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-Wno-unused-function",
        "-Wno-unused-variable",
        "-Wno-unused-parameter",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("-I{}", dir.display()));
    // (env_on, the switches' rule: the cut-out reverb reads HOS_AUDIO_NO_REVERB with it)
    args.push("-include".into());
    args.push(
        repo.join("gpu/vk/compat/hookshot_env.h")
            .display()
            .to_string(),
    );
    args.push(repo.join("tests/audio_biquad_test.c").display().to_string());
    if !cfg!(windows) {
        args.push("-lm".into());
    }
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the biquad fixture");
    let ran = Command::new(&exe).output().expect("run the biquad fixture");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}
