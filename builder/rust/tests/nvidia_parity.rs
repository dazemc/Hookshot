// NVIDIA DLSS/DLSS-G parity with upstream NotRexed/Hookshot. These are game-free source checks and compiled fixtures:
// no game, dump, NVIDIA GPU or NGX runtime is needed.
//
// Ported here:
//  - upstream 1bf4284a (#2658): a DLSS feature that was not made is asked for again when the quality mode, preset or
//    output changes, a failing evaluation is logged when its result changes and every 600th time, and after 30 failures
//    in a row the feature is let go once. Frame generation reads the multiplier its images were made for.
//  - upstream 6cdee4c1: a fixed multiplier makes no more frames than the display can show, and the display thread leaves
//    a made frame out when the game's next frame is already waiting.
//  - upstream f3dd4486 (#2145): switching from scaled DLSS to DLAA waits up to three scene frames for the native scene
//    instead of making a DLAA feature for a queued small one, and the feature's native-AA intent is part of its reuse.
//
// Already present and pinned here: Reflex submit IDs (8caa6749), V-Sync Auto following DLSS-G's real frame limit
// (ee29dd55), and both NGX runtimes in the game folder (55b976c0).

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
    let candidates = std::env::var("CXX")
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

/// Compile a fixture and run it; the fixture prints PASS on success and exits non-zero on a failed check.
fn compile_and_run(cxx: &str, dir: &Path, name: &str, source: &str, includes: &[&str]) -> String {
    let src = dir.join(format!("{name}.cpp"));
    let exe = dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    });
    fs::write(&src, source).expect("write fixture");
    let mut command = Command::new(cxx);
    command.args(["-std=c++20", "-Wall", "-Wextra", "-Werror"]);
    for include in includes {
        command.arg(format!("-I{include}"));
    }
    let build = command
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", src.display());
    let output = Command::new(&exe).output().expect("run the fixture");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "fixture failed:\n{text}");
    text
}

#[test]
fn a_refused_dlss_feature_is_retried_and_a_failing_evaluation_is_not_latched() {
    let dlss = read("gpu/vk/vk_dlss.cpp");
    let dlss_h = read("gpu/vk/vk_dlss.h");
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let framegen = read("gpu/vk/vk_framegen.cpp");

    // A creation that failed is held off only for the same output, quality mode, preset and HDR: the quality mode is in
    // the key, so the Render resolution row asks again, and DLSS/DLAA switching does too.
    let prepare = function(&dlss, "int prepare(");
    assert!(
        prepare.contains("(uint64_t)((int)mode & 7) << 8"),
        "the DLSS refusal key does not include the quality mode"
    );
    assert!(
        prepare.contains("if (s.refused == key) return *fail = \"its feature was not made\", 0;"),
        "a refused feature is not keyed"
    );

    // Choosing another engine, or the chain going idle, clears the refusal: coming back asks for a new feature.
    let release = function(&dlss, "void release() {");
    let clear = release
        .find("s.refused = 0;")
        .expect("release does not clear the refusal");
    let early = release
        .find("if (!s.feature) return;")
        .expect("release's early return");
    assert!(clear < early, "release must clear the refusal before its early return");

    // A failed evaluation is retried next frame (TemporalEngine::retry); its log is spaced, and 30 in a row remake it.
    let evaluate = function(&dlss, "bool evaluate(");
    assert!(evaluate.contains("++s.eval_repeats % 600 == 0"));
    assert!(evaluate.contains("if (r != s.eval_result) s.eval_result = r, s.eval_repeats = 0;"));
    assert!(evaluate.contains("if (s.eval_repeats == 30)"));
    assert!(evaluate.contains("release();"));
    assert!(evaluate.contains("s.eval_result = NVSDK_NGX_Result_Success;"));
    let state = dlss
        .split_once("struct State {")
        .expect("DLSS State")
        .1
        .split_once("\n};")
        .expect("State end")
        .0;
    assert!(state.contains("NVSDK_NGX_Result eval_result"));
    assert!(state.contains("uint64_t eval_repeats"));

    // The menu's scaling rows clear the chain's latch and DLSS's own refusal, with the chain's lock held.
    assert!(dlss_h.contains("void dlss_retry();"));
    assert!(dlss.contains("void dlss_retry() { st().refused = 0; }"));
    assert!(dlss.contains("void dlss_retry() {}"), "the build without NGX needs the stub");
    let reset = function(&temporal, "void temporal_reset_failure()");
    assert!(reset.contains("dlss_retry();"));

    // Frame generation reads the multiplier its sets were made for once, not the row again after ensure_size.
    let record = function(&framegen, "bool framegen_record(");
    assert!(record.contains("m = fg.mult;"));
    assert!(record.contains("auto_rate = opts.framegen == 1;"));
    assert!(record.contains("if (auto_rate)"));
    assert!(
        !record.contains("m = multiplier()"),
        "framegen_record still rereads the menu's multiplier"
    );
}

const FIXED_FRAMES_FIXTURE: &str = r#"
#include <algorithm>
#include <cstdint>
#include <cstdio>
/* FIXED_FRAMES */
#define CHECK(x) do { if (!(x)) { std::printf("FAIL: %s\n", #x); return 1; } } while (0)
int main() {
    const int64_t f60 = 16666667, f30 = 33333333, f45 = 22222222;
    CHECK(fixed_frames(8, 180, f60) == 3);                   /* 8x at 60 fps on 180 Hz: 3 shown, 2 made */
    CHECK(fixed_frames(8, 180, 15873016) == 3);              /* 63 fps: still 3 */
    CHECK(fixed_frames(2, 180, f60) == 2);                   /* below the display: as asked */
    CHECK(fixed_frames(8, 60, f60) == 1);                    /* at the refresh: the game's frame alone */
    CHECK(fixed_frames(4, 60, f30) == 2);                    /* 30 fps on 60 Hz: 2x */
    CHECK(fixed_frames(8, 144, f45) == 3);                   /* 45 fps on 144 Hz: 3 (135 a second) */
    CHECK(fixed_frames(8, 240, f30) == 8);                   /* 30 fps on 240 Hz: all 8 */
    CHECK(fixed_frames(8, 0, f60) == 8 && fixed_frames(8, 180, 0) == 8);   /* unknown refresh or frame time: as asked */
    CHECK(fixed_frames(1, 180, f60) == 1 && fixed_frames(0, 180, f60) == 1);
    std::puts("PASS: a fixed multiplier makes no more frames than the display shows");
    return 0;
}
"#;

#[test]
fn a_fixed_frame_generation_multiplier_keeps_the_games_rate() {
    let framegen = read("gpu/vk/vk_framegen.cpp");
    let fixed = function(&framegen, "int fixed_frames(");
    assert!(fixed.contains("const int fit = (int)((double)hz * (double)game_ns / 1e9 + 0.2);"));
    assert!(fixed.contains("return std::clamp(fit, 1, m);"));

    let record = function(&framegen, "bool framegen_record(");
    assert!(record.contains("RX_VK_FRAMEGEN_UNCAPPED"));
    assert!(record.contains("fixed_frames(m, (display_refresh_mhz() + 500) / 1000, fg.game_ns)"));
    assert!(record.contains("if (set->made == 0 && blend && mk == 2 && fsr3fi::frames_wanted())"));
    assert!(record.contains("for (int k = 1; k < mk; k++)"));
    assert!(record.contains("set->made = mk - 1;"));
    assert!(
        !record.contains("for (int k = 1; k < m; k++)"),
        "the built-in interpolation still makes the asked multiplier, not the display's frames"
    );

    // The display thread leaves a made frame out when the game's next frame is already waiting, so it never holds the
    // game back. A set's game frame is always shown; present ahead's copies are already on their way.
    assert!(framegen.contains("!fg.jobs.empty()"));
    assert!(framegen.contains("stat.skipped++"));
    assert!(framegen.contains("made ones left out (the game's next"));

    let Some(cxx) = compiler() else {
        eprintln!("skipping the fixed_frames fixture: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let dir = scratch("fixed-frames");
    let fixture = FIXED_FRAMES_FIXTURE.replace("/* FIXED_FRAMES */", &fixed);
    let output = compile_and_run(&cxx, &dir, "fixed_frames", &fixture, &[]);
    assert!(output.contains("PASS"));
    let _ = fs::remove_dir_all(dir);
}

const DLAA_FIXTURE: &str = r#"
#include <cstdint>
#include <cstdio>
#include "gpu/vk/upscaler_rows.h"
namespace ur = mkwvk::upscaler_rows;

struct TestEngine {
    const char* name = "fixture";
    bool feature = false, feature_aa = false;
    bool feature_hdr = false, requested_hdr = false;
    uint32_t feature_ow = 0, feature_oh = 0, feature_rw = 0, feature_rh = 0;
    uint32_t min_rw = 0, min_rh = 0;
    unsigned feature_preset = 0, creations = 0;

    int prepare(int, uint32_t rw, uint32_t rh, uint32_t ow, uint32_t oh, bool native_aa,
                uint32_t* in_w, uint32_t* in_h, const char**) {
        auto& s = *this;
        const unsigned preset = 0;
        const bool hdr = requested_hdr;
        (void)native_aa;
        /* REUSE */

        // Model a scaled feature that can accept full-size input, just as NGX can.
        feature = true;
        feature_hdr = hdr;
        feature_aa = rw == ow && rh == oh;
        feature_ow = feature_rw = ow;
        feature_oh = feature_rh = oh;
        min_rw = feature_aa ? ow : ow / 2;
        min_rh = feature_aa ? oh : oh / 2;
        ++creations;
        return 2;
    }

    void release() { feature = false; }
};

struct Engine { TestEngine* e; };
struct State {
    TestEngine* drawing = nullptr;
    uint64_t last_frame = 0;
    int drawn_mode = ur::GAME;
    bool dlaa_waiting = false;
    uint64_t dlaa_wait_frame = 0;
};
static uint64_t fixture_frame = 0;
static uint64_t texture_frame_no() { return fixture_frame; }
static void descriptors_disturbed() {}
template <class... Args> static void logf(const char*, Args...) {}

static void step(State& s, Engine engine, int mode, uint32_t rw, uint32_t rh, bool advance = true) {
    if (advance) ++fixture_frame;
    const uint32_t ow = 3440, oh = 1440;
    const int cmd = 0;
    /* DISPATCH */
    (void)ready;
    /* DRAWN_MODE */
}

int main() {
    State state;
    TestEngine ngx, fsr;
    step(state, {&ngx}, ur::DLSS, 1720, 720);
    step(state, {&ngx}, ur::DLAA, 1720, 720);
    step(state, {&ngx}, ur::DLAA, 2294, 960);
    if (ngx.creations != 1 || ngx.feature_ow != 3440 || ngx.feature_oh != 1440) {
        std::fprintf(stderr, "The queued scaled frames changed the DLSS feature\n");
        return 1;
    }
    step(state, {&ngx}, ur::DLAA, 3440, 1440);
    step(state, {&ngx}, ur::DLAA, 3440, 1440);
    if (ngx.creations != 2 || !ngx.feature_aa) return 2;
    step(state, {&fsr}, ur::FSR4_AA, 1720, 720);
    if (ngx.feature || fsr.feature_ow != 1720) return 3;

    State auto_native;
    TestEngine already_native;
    step(auto_native, {&already_native}, ur::DLSS, 3440, 1440);
    step(auto_native, {&already_native}, ur::DLAA, 3440, 1440);
    if (already_native.creations != 1 || !already_native.feature_aa) return 4;

    State startup;
    TestEngine startup_ngx;
    step(startup, {&startup_ngx}, ur::DLAA, 1720, 720);
    if (startup_ngx.creations != 1 || startup_ngx.feature_ow != 1720) return 5;

    State undersized;
    TestEngine small;
    step(undersized, {&small}, ur::DLSS, 1720, 720);
    for (int i = 0; i < 3; ++i) step(undersized, {&small}, ur::DLAA, 1720, 720);
    if (small.creations != 1 || undersized.drawn_mode != ur::DLSS) return 6;
    step(undersized, {&small}, ur::DLAA, 1720, 720);
    step(undersized, {&small}, ur::DLAA, 1720, 720);
    if (small.creations != 2 || !small.feature_aa || small.feature_ow != 1720 || undersized.drawn_mode != ur::DLAA) return 7;

    already_native.requested_hdr = true;
    step(auto_native, {&already_native}, ur::DLAA, 3440, 1440);
    if (already_native.creations != 2 || !already_native.feature_hdr) return 8;
    already_native.requested_hdr = false;
    step(auto_native, {&already_native}, ur::DLAA, 3440, 1440);
    if (already_native.creations != 3 || already_native.feature_hdr) return 9;

    State repeated;
    TestEngine same_frame;
    step(repeated, {&same_frame}, ur::DLSS, 1720, 720);
    step(repeated, {&same_frame}, ur::DLAA, 1720, 720);
    for (int i = 0; i < 5; ++i) step(repeated, {&same_frame}, ur::DLAA, 1720, 720, false);
    if (same_frame.creations != 1 || repeated.drawn_mode != ur::DLSS) return 10;
    step(repeated, {&same_frame}, ur::DLSS, 1720, 720);
    for (int i = 0; i < 3; ++i) step(repeated, {&same_frame}, ur::DLAA, 1720, 720);
    if (same_frame.creations != 1 || repeated.drawn_mode != ur::DLSS) return 11;
    step(repeated, {&same_frame}, ur::DLAA, 1720, 720);
    if (same_frame.creations != 2 || repeated.drawn_mode != ur::DLAA) return 12;
    std::puts("PASS: queued scenes, bounded native wait, reuse, HDR and engine switch");
    return 0;
}
"#;

#[test]
fn switching_to_dlaa_waits_for_the_native_scene() {
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let temporal_h = read("gpu/vk/vk_temporal.h");
    let dlss = read("gpu/vk/vk_dlss.cpp");
    let fsr3 = read("gpu/vk/vk_fsr3.cpp");
    let fsr4 = read("gpu/vk/vk_fsr4.cpp");

    // The chain keeps the mode it drew last and waits at most three scene frames for the native scene.
    let state = temporal
        .split_once("struct State {")
        .expect("temporal State")
        .1
        .split_once("\n};")
        .expect("State end")
        .0;
    assert!(state.contains("int drawn_mode = ur::GAME;"));
    assert!(state.contains("bool dlaa_waiting = false;"));
    assert!(state.contains("uint64_t dlaa_wait_frame = 0;"));
    let upscale = function(&temporal, "bool temporal_upscale(");
    assert!(upscale.contains("const bool entering_dlaa = mode == ur::DLAA && s.drawing == engine.e && s.drawn_mode == ur::DLSS;"));
    assert!(upscale.contains("frame - s.dlaa_wait_frame >= 3"));
    assert!(upscale.contains("const int draw_mode = waiting_for_native ? ur::DLSS : mode;"));
    assert!(upscale.contains("const bool native = ur::full_scale(draw_mode);"));
    assert!(upscale.contains("engine.e->prepare(cmd, rw, rh, dw, dh, native, &iw, &ih, &why);"));
    assert!(upscale.contains("s.drawn_mode = draw_mode;"));

    // The engine's prepare knows the native-AA intent, and DLSS reuses a feature only when it was made for native AA.
    assert!(temporal_h.contains("bool native_aa"));
    let prepare = function(&dlss, "int prepare(");
    assert!(prepare.contains("bool native_aa"));
    assert!(prepare.contains("(!native_aa || s.feature_aa)"));
    assert!(prepare.contains("s.feature_aa = mode == NVSDK_NGX_PerfQuality_Value_DLAA;"));
    assert!(prepare.contains("the native anti-aliasing mode was selected"));
    let state = dlss
        .split_once("struct State {")
        .expect("DLSS State")
        .1
        .split_once("\n};")
        .expect("State end")
        .0;
    assert!(state.contains("bool feature_aa = false;"));

    // FSR 3.1 and FSR 4 take the extra argument without changing their behaviour.
    assert!(function(&fsr3, "int chain_prepare(").contains("bool, uint32_t* in_w"));
    assert!(function(&fsr4, "int prepare(").contains("bool /*native_aa*/"));

    let Some(cxx) = compiler() else {
        eprintln!("skipping the DLAA transition fixture: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    // The real dispatch and feature-reuse decisions are cut out of the source, as upstream's Python test did.
    let from = temporal
        .find("if (!dview) return fail(\"no view of the scene depth\");")
        .expect("the depth view check");
    let chain = &temporal[from..];
    let dispatch_end = chain.find("    if (ready < 0)").expect("the prepare call");
    let dispatch = &chain[chain.find('\n').expect("dispatch start")..dispatch_end];
    let reuse_start = dlss
        .find("    if (s.feature && s.feature_ow == ow")
        .expect("the DLSS feature reuse");
    let reuse_end = dlss[reuse_start..]
        .find("    // (why it is made:")
        .expect("the reuse end")
        + reuse_start;
    let reuse = &dlss[reuse_start..reuse_end];
    assert!(chain.contains("s.drawn_mode = draw_mode;"));
    let fixture = DLAA_FIXTURE
        .replace("/* DISPATCH */", dispatch)
        .replace("/* REUSE */", reuse)
        .replace("/* DRAWN_MODE */", "s.drawn_mode = draw_mode;");
    let dir = scratch("dlaa-native");
    let output = compile_and_run(&cxx, &dir, "dlaa_native", &fixture, &[root().to_str().unwrap()]);
    assert!(output.contains("PASS"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn reflex_submit_ids_vsync_auto_and_ngx_runtimes_stay_as_ported() {
    let reflex = read("gpu/vk/vk_reflex.cpp");
    let reflex_h = read("gpu/vk/vk_reflex.h");
    let replay = read("gpu/vk/vk_replay.cpp");

    // Reflex: each queue submit carries the game frame its GPU work belongs to; a batch and a present's copy follow the
    // same rule, and RX_VK_REFLEX_SUBMIT_IDS=0 leaves them untagged for comparisons.
    assert!(reflex.contains("VK_STRUCTURE_TYPE_LATENCY_SUBMISSION_PRESENT_ID_NV"));
    assert!(reflex.contains("RX_VK_REFLEX_SUBMIT_IDS"));
    assert!(reflex.contains("VK_LATENCY_MARKER_RENDERSUBMIT_END_NV"));
    assert!(reflex_h.contains("const void* reflex_submit_id(const void* next, uint64_t frame = kReflexThisFrame);"));
    for site in [
        "si.pNext = reflex_submit_id(nullptr, pend.reflex);",
        "si.pNext = reflex_submit_id(nullptr);",
        "si.pNext = reflex_submit_id(nullptr, reflex_present_frame);",
    ] {
        assert!(replay.contains(site), "a Reflex submit site is missing: {site}");
    }

    // V-Sync Auto asks DLSS-G for the multiplier less one and follows the runtime's real limit, known at its first
    // present; the renderer's own frame generation then keeps Auto's mode.
    let main = read("gpu/vk/vk_main.cpp");
    let swap = function(&main, "bool swapchain_dlssg()");
    assert!(swap.contains("dlss_fg_runs(opts.framegen - 1)"));
    let dlss = read("gpu/vk/vk_dlss.cpp");
    let runs = function(&dlss, "bool dlss_fg_runs(int frames)");
    assert!(runs.contains("most_known"));
    assert!(runs.contains("frames <= (int)most"));
    assert!(dlss.contains("RX_VK_DLSS_FG_MOST"));
    assert!(dlss.contains("constexpr int kFgRefusalsOff = 3;"));
    assert!(dlss.contains("g.refusals = 0;"));

    // Both NGX runtimes are fetched and placed beside DLSS's, and the game looks for the same names.
    let sdk = read("builder/rust/src/ngx_sdk.rs");
    assert!(sdk.contains("FrameGeneration"));
    assert!(sdk.contains("libnvidia-ngx-dlssg.so.310.9.16"));
    assert!(sdk.contains("nvngx_dlssg.dll"));
    assert!(sdk.contains("RuntimeKind::ALL.map(|kind| platform.runtime(kind))"));
    assert!(sdk.contains("for kind in RuntimeKind::ALL"));
    let package = read("builder/rust/src/package.rs");
    assert!(package.contains("crate::ngx_sdk::install_runtime(sdk, ngx, &out.join(\"ngx\"))"));
    assert!(package.contains("crate::ngx_sdk::remove_runtimes(&out.join(\"ngx\"), ngx)"));
    assert!(dlss.contains("constexpr const char* kFgRuntime = \"nvngx_dlssg.dll\";"));
    assert!(dlss.contains("constexpr const char* kFgRuntime = \"libnvidia-ngx-dlssg.so\";"));
}
