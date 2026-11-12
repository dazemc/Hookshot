// Frame generation by and the FPS counter name who makes the frames (upstream NotRexed/Hookshot 8296d07c):
// vk_framegen.cpp notes each set's maker, vk_dlss.cpp names why DLSS-G did not make it, and vk_menu.cpp shows one
// "<engine> FG" name on the row and in the counter. The saved row's values stay 0 Auto / 1 Built-in, and no game, dump
// or GPU is read.

use std::fs;
use std::path::{Path, PathBuf};

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

#[test]
fn the_row_and_the_counter_use_one_name_per_engine() {
    let options = source("gpu/vk/vk_menu.h");
    assert!(
        options.contains("int framegen_by = 0;")
            && options.contains("0 Auto (NVIDIA DLSS Frame Generation where it runs")
            && options.contains("1 the renderer's own"),
        "the saved Frame generation by values changed: 0 must stay Auto and 1 the renderer's own"
    );
    let menu = source("gpu/vk/vk_menu.cpp");
    assert!(
        menu.contains(
            "{TAB_GRAPHICS, \"Frame generation by\", &opts.framegen_by, 0, 1, 1, tx_framegen_by,"
        ),
        "the Frame generation by row no longer keeps its 0..1 values"
    );
    let tx = block(&menu, "const char* tx_framegen_by(int v) {");
    assert!(
        tx.contains("\"Built-in FG\""),
        "the chosen built-in value is not named Built-in FG"
    );
    assert!(tx.contains("\"Auto\""), "the row's Auto text is gone");
    assert!(
        tx.contains("snprintf(b, sizeof b, \"Auto: %s\", who)"),
        "Auto does not name the maker"
    );
    let name = block(&menu, "const char* framegen_maker_name() {");
    for maker in ["\"DLSS FG\"", "\"FSR 3.1 FG\"", "\"Built-in FG\""] {
        assert!(name.contains(maker), "framegen_maker_name misses {maker}");
    }
    assert!(
        name.contains("opts.framegen < 1"),
        "a name is shown with frame generation off"
    );

    let sub = menu[menu.find("char fps_sub[96] = \"\";").expect("fps_sub")..]
        .split('\n')
        .next()
        .unwrap();
    assert!(
        sub.contains("UTF-8"),
        "the counter's second line is not sized for a UTF-8 name"
    );
    let tick = block(&menu, "void menu_tick() {");
    assert!(
        tick.contains("snprintf(fps_sub, sizeof fps_sub, \"Base %.0f, %s\", game, who)")
            && tick.contains("snprintf(fps_sub, sizeof fps_sub, \"Base %.0f\", game)"),
        "the FPS counter's second line does not say Base and the maker"
    );
    assert!(
        !tick.contains("\"Built-in\""),
        "the old Built-in text is still written"
    );
    assert!(
        block(
            &menu,
            "const char* framegen_skip_text(int why, unsigned most) {"
        )
        .contains("\"Not DLSS FG now: %s\"")
            || block(&menu, "const char* hn_framegen_by(int v, int variant) {")
                .contains("\"Not DLSS FG now: %s\""),
        "the row's help does not say why another maker has the frames"
    );
    for reason in [
        "FG_SKIP_OFF",
        "FG_SKIP_DEVICE",
        "FG_SKIP_START",
        "FG_SKIP_TOO_MANY",
        "FG_SKIP_INPUTS",
        "FG_SKIP_REFUSED",
        "FG_SKIP_NOT_BUILT",
        "FG_SKIP_COUNT",
    ] {
        assert!(menu.contains(reason), "the row cannot name {reason}");
    }
    assert!(
        source("readme.md").contains("game 60, DLSS FG")
            && source("readme.md").contains("FSR 3.1 FG"),
        "readme.md does not document the row and the counter"
    );
}

#[test]
fn the_maker_is_noted_and_dlss_names_its_reasons() {
    let framegen = source("gpu/vk/vk_framegen.cpp");
    let header = source("gpu/vk/vk_framegen.h");
    assert!(
        header.contains("enum class FgMaker { None, BuiltIn, Dlss, Fsr3 };")
            && header.contains("FgMaker framegen_maker();"),
        "vk_framegen.h does not declare the makers"
    );
    let maker = block(&framegen, "FgMaker framegen_maker() {");
    assert!(
        maker.contains("500000000")
            && maker.contains("FgMaker::Dlss")
            && maker.contains("FgMaker::Fsr3"),
        "framegen_maker does not pick the most noted maker or time out"
    );
    let note = block(&framegen, "void note_maker(FgMaker m) {");
    assert!(
        note.contains("maker_count[slot].fetch_sub")
            && note.contains("maker_count[slot].fetch_add"),
        "note_maker does not keep the ring's counts"
    );
    let record = block(&framegen, "bool framegen_record(");
    assert!(
        record.contains("note_maker(flow ? FgMaker::BuiltIn : FgMaker::None);"),
        "Auto's renderer-made frames are not noted as Built-in"
    );
    assert!(
        record.contains("FgMaker maker = set->made ? FgMaker::Dlss : FgMaker::None;")
            && record.contains("if (maker == FgMaker::None && set->made) maker = FgMaker::Fsr3;")
            && record.contains("maker = FgMaker::BuiltIn;")
            && record.contains("note_maker(maker);"),
        "a fixed multiplier's set does not note DLSS-G, FSR 3.1 or the built-in maker"
    );

    let dlss_h = source("gpu/vk/vk_dlss.h");
    assert!(
        dlss_h.contains("int dlss_fg_skip(unsigned* most = nullptr);")
            && dlss_h.contains("FG_SKIP_NOT_BUILT"),
        "vk_dlss.h does not declare the skip reasons"
    );
    let dlss = source("gpu/vk/vk_dlss.cpp");
    let frames = block(&dlss, "int dlss_fg_frames(");
    for call in [
        "skip(FG_SKIP_OFF)",
        "skip(dlss_why() != DLSS_OK ? FG_SKIP_DEVICE : FG_SKIP_START)",
        "skip(FG_SKIP_TOO_MANY)",
        "skip(FG_SKIP_INPUTS)",
        "skip(FG_SKIP_REFUSED)",
        "skip(FG_SKIP_NONE)",
    ] {
        assert!(frames.contains(call), "dlss_fg_frames does not name {call}");
    }
    assert!(
        frames.contains("if (!blend) return g.not_blended++, 0;"),
        "a set with nothing to blend from must leave the reason as it was"
    );
    assert!(
        block(&dlss, "int dlss_fg_skip(unsigned* most) {").contains("fg_skip.load")
            && block(&dlss, "int dlss_fg_skip(unsigned* most) {").contains("fg_skip_most.load"),
        "dlss_fg_skip does not give the last reason and the runtime's most"
    );
    let stub = &dlss[dlss
        .find("bool dlss_fg_wanted() { return false; }")
        .expect("the no-NGX stubs")..];
    assert!(
        stub.contains("return FG_SKIP_NOT_BUILT;"),
        "a build without the NGX SDK does not say so"
    );
}

#[test]
fn the_naming_reads_no_game_data() {
    let menu = source("gpu/vk/vk_menu.cpp");
    for marker in [
        "const char* framegen_maker_name() {",
        "const char* tx_framegen_by(int v) {",
        "const char* framegen_skip_text(int why, unsigned most) {",
        "const char* hn_framegen_by(int v, int variant) {",
    ] {
        let text = block(&menu, marker);
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
                !text.contains(forbidden),
                "{marker} reads {forbidden}: naming must stay independent of the game's data"
            );
        }
    }
    let framegen = source("gpu/vk/vk_framegen.cpp");
    let maker = block(&framegen, "FgMaker framegen_maker() {");
    assert!(
        !maker.contains("fopen") && !maker.contains("romfs") && !maker.contains("work/dump"),
        "framegen_maker reads game data"
    );
}
