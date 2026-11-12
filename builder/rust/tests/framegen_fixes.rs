// Frame generation fixes ported after the FSR 3.1 chain work (upstream 1bf4284a and e2057b2b): the multiplier a set was
// made for is the one its copy targets, and AMD Anti-Lag 2 keeps pacing the game's frames when frame generation shows them
// from its display thread. These are game-free source checks; no game, dump or GPU is needed.

use std::fs;
use std::path::{Path, PathBuf};

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

#[test]
fn frame_generation_copies_into_the_multiplier_its_sets_were_made_for() {
    let source = read("gpu/vk/vk_framegen.cpp");
    let record = function(&source, "bool framegen_record(");
    assert!(
        record.contains("const int cur = fg.cur, prev = cur ^ 1, m = fg.mult;"),
        "framegen_record must use the multiplier ensure_size made the sets for"
    );
    assert!(
        !record.contains("m = multiplier()"),
        "framegen_record reads the menu's multiplier again after its sets were sized"
    );
    assert!(
        record.contains("const bool auto_rate = opts.framegen == 1;"),
        "the Auto test is read once, with the multiplier"
    );
    assert!(
        record.contains("if (auto_rate) {"),
        "Auto's branch must use the same reading"
    );
}

#[test]
fn anti_lag_2_keeps_pacing_with_frame_generation() {
    let antilag = read("gpu/vk/vk_antilag.cpp");
    let header = read("gpu/vk/vk_antilag.h");
    let framegen = read("gpu/vk/vk_framegen.cpp");
    let framegen_h = read("gpu/vk/vk_framegen.h");
    let replay = read("gpu/vk/vk_replay.cpp");
    let menu = read("gpu/vk/vk_menu.cpp");

    // Anti-Lag 2 no longer turns itself off with frame generation. The input stage is the game thread's; the present stage
    // is the frame the display thread shows, with the set's own game frame, and a made frame gives none.
    assert!(
        !antilag.contains("framegen_on"),
        "Anti-Lag 2 still turns itself off with frame generation"
    );
    assert!(antilag.contains("thread_local uint64_t antilag_present_frame = kAntiLagThisFrame;"));
    assert!(antilag.contains("uint64_t antilag_frame()"));
    assert!(antilag.contains("uint64_t since = 0;"));
    assert!(antilag.contains("f <= a.since"));
    assert!(header.contains("kAntiLagThisFrame"));
    assert!(header.contains("extern thread_local uint64_t antilag_present_frame;"));

    // Every set keeps the game frame it was made at; both present paths set the stage's frame as Reflex does.
    let record = function(&framegen, "bool framegen_record(");
    assert!(record.contains("set->antilag_id = antilag_frame();"));
    assert_eq!(
        framegen.matches("antilag_present_frame = (timed ? j.set->game[k] : k == j.set->made) ? j.set->antilag_id : 0;")
            .count(),
        2,
        "both display paths must set the set's Anti-Lag frame"
    );
    assert!(framegen_h.contains("uint64_t antilag_frame = ~0ull;"));
    assert!(replay.contains("present_defer_into->antilag_frame = antilag_present_frame;"));
    assert!(replay.contains("antilag_present_frame = p.antilag_frame;"));
    assert!(replay.contains("antilag_present_frame = was_antilag;"));

    // The row's hint says it works with frame generation.
    assert!(menu.contains("With frame generation too: it paces the "));
    assert!(menu.contains("\"game's frames.\", hn_antilag"));
    assert!(!menu.contains("Not used with frame generation."));
}

#[test]
fn a_failed_dlss_feature_is_asked_for_again() {
    let dlss = read("gpu/vk/vk_dlss.cpp");
    let dlss_h = read("gpu/vk/vk_dlss.h");
    let temporal = read("gpu/vk/vk_temporal.cpp");
    let prepare = function(&dlss, "int prepare(");
    assert!(
        prepare.contains("(uint64_t)((int)mode & 7) << 8"),
        "the quality mode is part of the key a refused feature is held by"
    );
    let release = function(&dlss, "void release() {");
    assert!(
        release.contains("s.refused = 0;"),
        "release must let the next ask retry"
    );
    assert!(dlss.contains("void dlss_retry() { st().refused = 0; }"));
    assert!(dlss_h.contains("void dlss_retry();"));
    assert!(function(&temporal, "void temporal_reset_failure()").contains("dlss_retry();"));
    let evaluate = function(&dlss, "bool evaluate(");
    assert!(evaluate.contains("s.eval_result") && evaluate.contains("s.eval_repeats"));
    assert!(evaluate.contains("if (s.eval_repeats == 30)"));
}
