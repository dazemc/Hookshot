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

fn cpp_compiler() -> Option<String> {
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

#[test]
fn a_near_full_waveout_ring_retires_the_oldest_buffer() {
    if cfg!(windows) {
        // (host/posix's waveOut shim is the Linux one; Windows uses winmm)
        return;
    }
    let Some(cc) = compiler() else {
        eprintln!("skipping the waveOut ring fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/posix/win32_posix.c")).expect("win32_posix.c");
    let ring = slice(
        &source,
        "typedef struct {\n    pthread_mutex_t mu;",
        "static void wave_pull",
    );
    let parts = [
        ring,
        function(&source, "static void wave_pull("),
        function(&source, "MMRESULT waveOutWrite("),
        function(&source, "MMRESULT waveOutReset("),
    ];
    assert!(
        parts[2].contains("if (w->count >= 60)"),
        "waveOutWrite no longer retires a buffer before the ring is full"
    );
    let dir = scratch("audio-wave-ring");
    fs::write(dir.join("audio_wave_ring.inc"), parts.join("\n"))
        .expect("write audio_wave_ring.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_wave_ring.exe"
    } else {
        "audio_wave_ring"
    });
    let mut args: Vec<String> = [
        "-std=c11",
        "-O1",
        "-g",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-Wno-unused-function",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("-I{}", dir.display()));
    args.push(
        repo.join("tests/audio_wave_ring_test.c")
            .display()
            .to_string(),
    );
    args.push("-pthread".into());
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the waveOut ring fixture");
    let ran = Command::new(&exe)
        .output()
        .expect("run the waveOut ring fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("PASS"));
}

#[test]
fn the_sink_maps_five_one_to_speakers_and_folds_it_down_to_stereo() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the sink map fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/nn_audio.c")).expect("host/nn_audio.c");
    let body = slice(
        &source,
        "    /* the final mix through the sink's channel map",
        "    const int nch = six ? 6 : 2;",
    );
    assert!(
        body.contains("sink_in[k]") && body.contains("0.7071f"),
        "the sink map no longer reads the six sink inputs or folds them down"
    );
    let inc = format!(
        "static void sink_map(float dst[6][FRAME], int channels) {{\n{body}\n    memcpy(dst, out, sizeof out);\n}}\n"
    );
    let dir = scratch("audio-sink-map");
    fs::write(dir.join("audio_sink_map.inc"), inc).expect("write audio_sink_map.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_sink_map.exe"
    } else {
        "audio_sink_map"
    });
    let mut args: Vec<String> = [
        "-std=c11",
        "-O1",
        "-g",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-Wno-unused-function",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("-I{}", dir.display()));
    args.push(
        repo.join("tests/audio_sink_map_test.c")
            .display()
            .to_string(),
    );
    if !cfg!(windows) {
        args.push("-lm".into());
    }
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the sink map fixture");
    let ran = Command::new(&exe)
        .output()
        .expect("run the sink map fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("PASS"));
}

#[test]
fn voices_resample_by_rate_and_pitch_and_keep_the_fraction() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the resampler fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/nn_audio.c")).expect("host/nn_audio.c");
    let definitions: Vec<&str> = source
        .lines()
        .filter(|line| {
            [
                "FRAME",
                "RATE",
                "MAX_VOICES",
                "MAX_MIXES",
                "MAX_SPLITTERS",
                "MAX_SPLIT_DESTS",
                "MAX_CH",
                "MAX_BUF",
                "MAX_WB",
                "MAX_REL",
            ]
            .iter()
            .any(|name| line.starts_with(&format!("#define {name} ")))
        })
        .collect();
    let types = slice(&source, "enum { K_VOICE", "static CRITICAL_SECTION lock;");
    let render = function(&source, "static void render_voice(");
    assert!(
        render.contains("const double step = (double)v->rate")
            && render.contains("v->frac += step"),
        "render_voice no longer steps by rate x pitch"
    );
    let dir = scratch("audio-resample");
    fs::write(
        dir.join("audio_resample_types.inc"),
        format!("{}\n{types}", definitions.join("\n")),
    )
    .expect("write audio_resample_types.inc");
    fs::write(dir.join("audio_resample_render.inc"), render)
        .expect("write audio_resample_render.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_resample.exe"
    } else {
        "audio_resample"
    });
    let mut args: Vec<String> = [
        "-std=c11",
        "-O1",
        "-g",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-Wno-unused-function",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("-I{}", dir.display()));
    args.push(
        repo.join("tests/audio_resample_test.c")
            .display()
            .to_string(),
    );
    if !cfg!(windows) {
        args.push("-lm".into());
    }
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the resampler fixture");
    let ran = Command::new(&exe)
        .output()
        .expect("run the resampler fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("PASS"));
}

#[test]
fn the_output_picks_the_default_then_the_next_device_and_recovers() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the output device fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/nn_audio.c")).expect("host/nn_audio.c");
    let definitions: Vec<&str> = source
        .lines()
        .filter(|line| line.starts_with("#define FRAME ") || line.starts_with("#define RATE "))
        .collect();
    let actual = slice(&source, "#define NBLOCKS", "/* ---- parameter updates");
    let dir = scratch("audio-output");
    fs::write(
        dir.join("audio_output.inc"),
        format!("{}\n{actual}", definitions.join("\n")),
    )
    .expect("write audio_output.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_output.exe"
    } else {
        "audio_output"
    });
    let mut args: Vec<String> = [
        "-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror", "-include",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(
        repo.join("gpu/vk/compat/hookshot_env.h")
            .display()
            .to_string(),
    );
    args.push(format!("-I{}", dir.display()));
    args.push(
        repo.join("tests/audio_output_errors_test.c")
            .display()
            .to_string(),
    );
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the output device fixture");
    for case in [
        "normal",
        "busy",
        "prepare",
        "write",
        "unprepare",
        "open",
        "bounded",
        "stall",
        "surround",
        "fallback",
        "lost",
        "gone",
        "service",
        "default",
    ] {
        let ran = Command::new(&exe)
            .arg(case)
            .output()
            .unwrap_or_else(|e| panic!("run {case}: {e}"));
        assert!(
            ran.status.success(),
            "{case}: {}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        assert!(String::from_utf8_lossy(&ran.stdout).contains("PASS"));
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn linux_audio_uses_the_bundled_sdl_and_whole_sample_frames() {
    let repo = root();
    let sdl = fs::read_to_string(repo.join("host/hos_sdl.c")).expect("host/hos_sdl.c");
    let posix = fs::read_to_string(repo.join("host/posix/win32_posix.c")).expect("win32_posix.c");
    let load = function(&sdl, "static HMODULE load_one(");
    assert!(
        load.find("GetModuleFileNameA").unwrap() < load.find("LoadLibraryA(dll)").unwrap(),
        "the SDL beside the executable is no longer tried before the system's"
    );
    let load_all = function(&sdl, "int hos_sdl_load(void)");
    assert!(
        load_all.find("load_one(\"SDL3.dll\")").unwrap()
            < load_all.find("load_one(\"SDL2.dll\")").unwrap(),
        "SDL3 is no longer preferred over SDL2"
    );
    let open = function(&posix, "MMRESULT waveOutOpen(");
    assert!(
        open.contains("hos_sdl_audio_open(") && !open.contains("LoadLibraryA"),
        "the audio no longer takes the same SDL as the window and the pads"
    );
    let audio_open = function(&sdl, "int hos_sdl_audio_open(");
    assert!(audio_open.contains("SDL_AUDIO_DEVICE_SAMPLE_FRAMES"));
    assert!(audio_open.contains("env_samples && atoi(env_samples) > 0 ? atoi(env_samples) : 512"));
    assert!(
        audio_open.contains("0xFFFFFFFFu /* the default output */"),
        "the audio no longer opens the system's default output"
    );
    let pull = function(&sdl, "static void pull3(");
    assert!(
        pull.contains("sizeof buf / s->frame * s->frame"),
        "the device buffer no longer takes whole sample frames"
    );
    let channels = function(&sdl, "int hos_sdl_audio_channels(void)");
    assert!(
        channels.contains("SDL_GetAudioDeviceFormat")
            && channels.contains("SDL_GetDefaultAudioInfo"),
        "the default output's channels are no longer read from SDL"
    );
    assert!(channels.contains("return n > 0 ? n : 2;"));
}

#[test]
fn the_sound_job_stays_at_sixty_hertz_above_a_sixty_fps_cap() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the high fps fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let exe = scratch("oot-high-fps").join(if cfg!(windows) {
        "oot_high_fps.exe"
    } else {
        "oot_high_fps"
    });
    let mut args: Vec<String> = ["-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(format!("-I{}", repo.join("host").display()));
    args.push(repo.join("tests/oot_high_fps_test.c").display().to_string());
    if !cfg!(windows) {
        args.push("-lm".into());
    }
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the high fps fixture");
    let ran = Command::new(&exe)
        .output()
        .expect("run the high fps fixture");
    let _ = fs::remove_dir_all(exe.parent().unwrap());
    assert!(
        ran.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("PASS"));
}

#[test]
fn a_microphone_name_reads_back_without_its_line_end_or_trailing_spaces() {
    let Some(cxx) = cpp_compiler() else {
        eprintln!(
            "skipping the microphone name fixture: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let repo = root();
    let menu = fs::read_to_string(repo.join("gpu/vk/vk_menu.cpp")).expect("gpu/vk/vk_menu.cpp");
    let parts = [
        function(
            &menu,
            "size_t text_char_start(const std::string& text, size_t at)",
        ),
        function(&menu, "std::string mic_key(const char* name)"),
    ];
    assert!(
        parts[1].contains("while (!k.empty() && k.back() == ' ') k.pop_back();"),
        "mic_key no longer drops the spaces an options line's end leaves"
    );
    let dir = scratch("audio-mic-key");
    fs::write(dir.join("audio_mic_key.inc"), parts.join("\n")).expect("write audio_mic_key.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_mic_key.exe"
    } else {
        "audio_mic_key"
    });
    let mut args: Vec<String> = ["-std=c++17", "-O1", "-g", "-Wall", "-Wextra", "-Werror"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(format!("-I{}", dir.display()));
    args.push(
        repo.join("tests/audio_mic_key_test.cpp")
            .display()
            .to_string(),
    );
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cxx, &args, "building the microphone name fixture");
    let ran = Command::new(&exe)
        .output()
        .expect("run the microphone name fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        ran.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("PASS"));
    assert!(menu.contains("mic_want = mic_key(eq + 1);"));
    assert!(
        !menu.contains("mic_want.back() == '\\n'") && !menu.contains("mic_want.back() == '\\r'"),
        "the load still strips line ends after mic_key"
    );
}

#[test]
fn the_game_is_told_the_menu_and_devices_speakers() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the speaker selection fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/nn_audio.c")).expect("host/nn_audio.c");
    let actual = slice(
        &source,
        "static int au_menu_output(void) {",
        "/* The device's format,",
    );
    assert!(
        actual.contains("const int menu = want && *want ? 0 : au_menu_output();")
            && actual.contains("n = device >= 6 ? 6 : 2;"),
        "au_speakers no longer prefers HOS_AUDIO_CHANNELS, then the menu, then the default output"
    );
    let dir = scratch("audio-speakers");
    fs::write(dir.join("audio_speakers.inc"), actual).expect("write audio_speakers.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_speakers.exe"
    } else {
        "audio_speakers"
    });
    let mut args: Vec<String> = ["-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(format!("-I{}", dir.display()));
    args.push(
        repo.join("tests/audio_speakers_test.c")
            .display()
            .to_string(),
    );
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the speaker selection fixture");
    // (saved audio_output or "-", the default output's channels, HOS_AUDIO_CHANNELS or "-", the speakers the game is
    //  told, whether the device was asked, the log)
    let cases = [
        ("-", 6, "-", 6, 1, "the default output has 6 channels"),
        ("-", 2, "-", 2, 1, "the default output has 2 channels"),
        ("-", 8, "-", 6, 1, "the default output has 8 channels"),
        ("0", 8, "-", 6, 1, "the default output has 8 channels"),
        ("0", 2, "-", 2, 1, "the default output has 2 channels"),
        ("1", 6, "-", 2, 0, "the Insert menu's Audio output"),
        ("2", 2, "-", 6, 0, "the Insert menu's Audio output"),
        ("3", 2, "-", 6, 0, "the Insert menu's Audio output"),
        ("-1", 6, "-", 6, 1, "the default output has 6 channels"),
        ("2", 6, "2", 2, 0, "HOS_AUDIO_CHANNELS"),
        ("1", 2, "6", 6, 0, "HOS_AUDIO_CHANNELS"),
        ("-", 2, "6", 6, 0, "HOS_AUDIO_CHANNELS"),
    ];
    for (saved, device, env, want, asked, log) in cases {
        let ini = dir.join("hookshot_options.ini");
        if ini.exists() {
            fs::remove_file(&ini).expect("remove the old options file");
        }
        let ran = Command::new(&exe)
            .current_dir(&dir)
            .args([
                saved,
                &device.to_string(),
                &want.to_string(),
                &asked.to_string(),
                env,
                log,
            ])
            .output()
            .unwrap_or_else(|e| panic!("run {saved}/{device}/{env}: {e}"));
        assert!(
            ran.status.success(),
            "{saved}/{device}/{env}: {}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_late_mixer_wake_mixes_the_owed_frames_on_the_old_grid() {
    let Some(cc) = compiler() else {
        eprintln!("skipping the mixer deadline fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let repo = root();
    let source = fs::read_to_string(repo.join("host/nn_audio.c")).expect("host/nn_audio.c");
    let deadline = function(&source, "static int64_t audio_mix_deadline(");
    assert!(
        deadline.contains("3000000000LL"),
        "the mixer no longer keeps its old grid through a short stall"
    );
    let dir = scratch("audio-deadline");
    fs::write(dir.join("audio_deadline.inc"), deadline).expect("write audio_deadline.inc");
    let exe = dir.join(if cfg!(windows) {
        "audio_deadline.exe"
    } else {
        "audio_deadline"
    });
    let mut args: Vec<String> = ["-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(format!("-I{}", dir.display()));
    args.push(
        repo.join("tests/audio_deadline_test.c")
            .display()
            .to_string(),
    );
    args.push("-o".into());
    args.push(exe.display().to_string());
    run(&cc, &args, "building the mixer deadline fixture");
    let ran = Command::new(&exe)
        .output()
        .expect("run the mixer deadline fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        ran.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("PASS"));
}
