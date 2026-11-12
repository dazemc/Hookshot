// The Quality preset Auto (this PC) (upstream NotRexed/Hookshot f6cfd987): auto_settings.h picks the tier, upscaler,
// render resolution and output from authored hardware profiles, and vk_menu.cpp applies that only to a first start or
// to a saved Auto preset whose hardware fingerprint changed. A saved Custom or named preset is never changed. The rule
// is compiled and run from tests/test_auto_settings.cpp; no game, dump or GPU is read.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
fn the_menu_reaches_auto_and_keeps_a_saved_preset() {
    let menu = source("gpu/vk/vk_menu.cpp");
    for needle in [
        "{TAB_GRAPHICS, \"Quality preset\", &opts.preset, 0, 6, 1, tx_preset,",
        "\"Auto (this PC)\"",
        "if (first) opts.preset = kAutoPreset;",
        "::auto_settings::choose(hw)",
        "::auto_settings::should_apply(first, opts.preset, auto_for, fp)",
        "fprintf(f, \"auto_for=%016llx\\n\"",
        "|| key == \"memory_budget\" || key == \"auto_for\" ||",
        "key == \"auto_tuned\" || key == \"auto_output\" || key == \"auto_steps\";",
        "::auto_settings::tune(tuning,",
        "fprintf(f, \"auto_tuned=%016llx\\n\"",
        "fprintf(f, \"auto_steps=%d,%d\\n\"",
        "if (!strcmp(line, \"auto_steps\"))",
        "fprintf(f, \"auto_output=%ux%u\\n\"",
        "apply_auto(!first);",
        "apply_auto(false);",
    ] {
        assert!(menu.contains(needle), "vk_menu.cpp no longer has: {needle}");
    }
    let options = source("gpu/vk/vk_menu.h");
    assert!(options.contains("constexpr int kAutoPreset = 6;"));
    assert!(
        !menu.contains("constexpr int kAutoPreset"),
        "vk_menu.cpp defines kAutoPreset again"
    );

    // (the upscaler is set before the tier's rows: apply_preset snaps the Render resolution to it)
    let auto = block(&menu, "void apply_auto(bool keep_own) {");
    assert!(
        auto.find("opts.dlss = c.upscaler;").unwrap() < auto.find("apply_preset(c.tier);").unwrap(),
        "apply_auto: opts.dlss must be set before apply_preset"
    );
    for needle in [
        "auto_output.w = o.w, auto_output.h = o.h;",
        "!(set.w == auto_output.w && set.h == auto_output.h)",
    ] {
        assert!(auto.contains(needle), "apply_auto misses: {needle}");
    }
    // (the Output resolution Auto chose is one of the menu's choices, now the screen's display modes; the player's custom
    // size is never taken)
    assert!(
        auto.contains("for (int i = 1; i < custom_index(); i++)")
            && auto.contains("outputs()[i].w == c.output_w")
            && auto.contains("outputs()[i].h == c.output_h"),
        "apply_auto does not find the chosen output among the display modes"
    );

    // (the CPU holding the frame rate ends the tuning for this run only: no auto_tuned for it)
    let tune = block(
        &menu,
        "void menu_auto_tune(double gpu_ms, double draws, double fps) {",
    );
    assert!(tune.contains("::auto_settings::resolution_for(c, ur::engine(u), v,"));
    let cpu = tune[tune
        .find("verdict == ::auto_settings::CPU_BOUND) {")
        .expect("the CPU-bound branch")..]
        .to_string();
    let cpu = &cpu[..cpu.find("} else {").expect("the settled branch")];
    assert!(
        !cpu.contains("auto_tuned") && cpu.contains("return;"),
        "menu_auto_tune: the CPU-bound verdict must end this run's tuning without auto_tuned"
    );

    // (a saved named preset or Custom is left alone: only a first start, or a saved Auto whose fingerprint changed, applies)
    let profile = block(&menu, "void menu_auto_for_gpu(");
    let not_auto = profile
        .find("if (!::auto_settings::should_apply(first, opts.preset, auto_for, fp)) {")
        .expect("the saved-preset guard");
    let apply = profile.find("apply_auto(!first);").expect("apply_auto");
    assert!(
        not_auto < apply,
        "menu_auto_for_gpu applies Auto before checking the saved preset"
    );
    let guard = &profile[not_auto..apply];
    assert!(
        guard.contains("the settings chosen for this PC before")
            && guard.contains("would be:")
            && guard.contains("return;"),
        "a saved preset is not left alone: menu_auto_for_gpu does not log and return"
    );
    assert!(
        profile.contains("if (first) opts.preset = kAutoPreset;"),
        "menu_auto_for_gpu does not mark only a first start as Auto"
    );

    // (the tiers' own Render resolution in auto_settings.h is apply_preset's)
    let preset = block(&menu, "void apply_preset(int p) {");
    let mut own = std::collections::BTreeMap::new();
    let mut tier: Option<usize> = None;
    for line in preset.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("case ") {
            tier = rest
                .split(':')
                .next()
                .and_then(|n| n.trim().parse::<usize>().ok());
        }
        if let (Some(t), Some(res)) = (tier, line.split("opts.resolution = ").nth(1)) {
            let v: i32 = res
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap()
                .parse()
                .unwrap();
            own.insert(t, v);
            tier = None;
        }
    }
    let rules = source("gpu/vk/auto_settings.h");
    let table = rules
        .split("constexpr int kTierResolution[] = {")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .expect("kTierResolution");
    let values: Vec<i32> = table
        .split(',')
        .map(|s| s.trim().parse().unwrap())
        .collect();
    assert_eq!(own.len(), 5, "apply_preset's five tiers not found");
    for (tier, resolution) in own {
        assert_eq!(
            values[tier], resolution,
            "auto_settings.h kTierResolution disagrees with apply_preset tier {tier}"
        );
    }

    let main = source("gpu/vk/vk_main.cpp");
    for needle in [
        "auto_profile();   // (the Quality preset Auto (this PC)",
        "menu_auto_for_gpu(p, hw);",
        "hw.mesh_native = dev.mesh_capable;",
        "hw.mesh_as_compute = dev.mesh_capable && !dev.mesh_shader;",
        "dev.mesh_capable = m.meshShader && m.taskShader;",
    ] {
        assert!(main.contains(needle), "vk_main.cpp no longer has: {needle}");
    }
    assert!(
        main.find("dev.mesh_capable = m.meshShader && m.taskShader;")
            .unwrap()
            < main
                .find("if (mesh_mode_used == 2) dev.mesh_shader = false;")
                .unwrap(),
        "vk_main.cpp: dev.mesh_capable must be read before the Mesh shaders mode is applied"
    );
    assert!(
        !main.contains("menu_first_start_for_gpu"),
        "the old memory-only first-start choice is still called"
    );
    assert!(
        source("gpu/vk/vk_replay.cpp").contains("menu_auto_tune(gpu_ms, draws, 120.0 / secs);"),
        "the replay's 120-present windows do not reach menu_auto_tune"
    );
    let api = source("gpu/vk/vk_api.h");
    assert!(
        api.contains("#define MKWVK_WINDOW_VERSION 3")
            && api.contains("void (*desktop_size)(void *window, int *width, int *height);"),
        "vk_api.h does not pass the desktop size"
    );
    assert!(
        source("host/posix/display_sdl.c").contains("refresh_mhz, desktop_size};"),
        "the Linux window bridge does not install desktop_size"
    );
    assert!(
        source("docs/development.md").contains("RX_VK_AUTO_PRESET=0")
            && source("readme.md").contains("Auto (this PC)"),
        "the Auto switches or preset are not documented"
    );
}

#[test]
fn the_rules_compile_for_authored_hardware_and_read_no_game_data() {
    let rules = source("gpu/vk/auto_settings.h");
    let fixture = source("tests/test_auto_settings.cpp");
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
            !rules.contains(forbidden) && !fixture.contains(forbidden),
            "the Auto rule or its fixture reads {forbidden}: it must stay pure and game-free"
        );
    }
    // (the authored profiles the fixture covers: RTX 12 GB at 1440p, RDNA 4 at 4K, 8 GB on 4K, 6 GB, iGPU, Deck,
    // no mesh shaders, a card run as compute, 10/11/12 GB, low RAM, a moved screen and the tuning verdicts)
    for profile in [
        "RTX, 12 GB, 1440p",
        "RX 9070 XT, 4K",
        "8 GB on a 4K screen",
        "RTX 6 GB, 1080p",
        "an integrated GPU",
        "a Steam Deck",
        "no mesh shaders",
        "RX 6800 on RADV, mesh shaders run as compute",
        "RTX 3080 10 GB",
        "RTX 2080 Ti 11 GB",
        "a 12 GB card reporting 12,000 MB",
        "8 GB of RAM",
        "another GPU: another fingerprint",
        "another screen: another fingerprint",
        "a saved Custom preset: Auto touches nothing",
        "a saved named preset: Auto touches nothing",
        "a saved Auto on the same hardware: nothing changes",
        "a saved Auto on another card or screen: chosen again",
        "the CPU holds it",
        "at most two steps",
    ] {
        assert!(
            fixture.contains(profile),
            "the Auto fixture no longer covers {profile}"
        );
    }

    let Some(cxx) = compiler() else {
        eprintln!("skipping the compiled Auto rule: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-auto-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) { "auto.exe" } else { "auto" });
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", root().join("gpu/vk").display()))
        .arg(root().join("tests/test_auto_settings.cpp"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the Auto rule");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).starts_with("auto settings:"));
}
