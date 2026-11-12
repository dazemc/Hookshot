// The Insert menu's Bug report (gpu/vk/vk_bug_report.cpp, vk_menu.cpp, vk_replay.cpp, vk_main.cpp and
// host/hos_main.c; upstream NotRexed/Hookshot 6f7ab2a7). Game-free: the writer is compiled alone with
// tests/bug_report_test.cpp and writes reports from made-up game folders in a made-up home folder. The menu row and
// the host's map line are checked in the sources. No game, GPU or Vulkan headers are needed.

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

/// A C/C++ function's definition from its first line, braces matched.
fn definition(text: &str, marker: &str) -> String {
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
fn the_menu_row_and_the_host_log_the_place() {
    let menu = source("gpu/vk/vk_menu.cpp");
    let at = menu
        .find("{TAB_GAMEPLAY, \"Bug report\", nullptr")
        .expect("the Gameplay tab has no Bug report row");
    let row = &menu[at..at + menu[at..].find("},").expect("the row's end")];
    for word in [
        "report.txt",
        "logs",
        "picture",
        "user name",
        "No saves",
        "nothing is sent",
    ] {
        assert!(row.contains(word), "the row's help does not say {word}");
    }
    let restart = menu
        .find("{TAB_GAMEPLAY, \"Restart\", nullptr")
        .expect("no Restart row");
    assert!(
        at < restart,
        "the Bug report row must be before Restart, with the system rows"
    );
    assert!(
        menu.contains("constexpr int kBugReportRow = kRestartRow - 1;"),
        "the Bug report row is not the one before Restart"
    );
    assert!(
        menu.contains("&selr == &rows[kBugReportRow] ? \"Save\" : \"Open\""),
        "the menu's prompt does not say Save for the row"
    );
    assert!(
        menu.contains("&r == &rows[kBugReportRow]) bug_report_save()"),
        "A or Enter on the row does not save the report"
    );
    assert!(
        menu.contains("&r == &rows[kBugReportRow]) choice_cell(c, va, vb, bug_report_cell()"),
        "the row's value does not show Press to save / Saving... / Saved / Not saved"
    );

    let save = definition(&menu, "void bug_report_save() {");
    for call in [
        "fill_here(in)",
        "device_report_text()",
        "changed_rows_text()",
        "ini_path()",
        "bug_report::start(",
    ] {
        assert!(save.contains(call), "bug_report_save does not call {call}");
    }
    let changed = definition(&menu, "std::string changed_rows_text() {");
    for part in [
        "static const Options defaults{}",
        "restart_changed(r)",
        "r.value == &opts.mic && *r.value >= 2",
    ] {
        assert!(changed.contains(part), "changed_rows_text misses {part}");
    }
    let help = definition(&menu, "float measure_help(ui::Ctx& c, float width) {");
    assert!(
        help.contains("bug_report_saved_text("),
        "measure_help makes no room for the report's saved line"
    );
    let picture = definition(
        &menu,
        "bool bug_report_picture(const char* path, uint64_t ticket) {",
    );
    assert!(
        picture.contains("shot_ask_next(path,") && picture.contains("bug_report::picture_written("),
        "the report's picture is not asked of shot_ask_next"
    );

    let replay = source("gpu/vk/vk_replay.cpp");
    let next = replay
        .find("int shot_ask_next(")
        .expect("shot_ask_next is missing");
    let line = replay[next..].split('\n').next().unwrap_or_default();
    assert!(
        line.contains("return shot_ask(path, -1, done, ctx);"),
        "shot_ask_next does not ask for the next present"
    );
    let record = definition(&replay, "void shot_ask_record(");
    assert!(
        record.contains("blit_crop(cmd, src, sw, sh, crop, ask_image,"),
        "the report's picture is not copied from the game's frame"
    );
    assert!(
        !record.contains("menu"),
        "the menu must not be drawn into the report's picture"
    );

    let main = source("gpu/vk/vk_main.cpp");
    assert!(
        definition(&main, "std::string device_report_text() {")
            .contains("VkPhysicalDeviceDriverProperties"),
        "the GPU line does not read the driver's name and version"
    );

    let host = source("host/hos_main.c");
    let refresh = definition(
        &host,
        "static void oot_inventory_refresh(Ctx *c, int64_t now) {",
    );
    assert!(
        refresh.contains("hos_log(\"map: %s\", line)") && refresh.contains("0x1d6189dau"),
        "the host does not log the map (Sequence_CurrentBanc) the report's place comes from"
    );

    // the switches the report lists are the names hookshot_env.txt takes (env_line_allowed's prefixes)
    let allowed = source("host/hos_main.c");
    let allowed = allowed
        .split("static int env_line_allowed(")
        .nth(1)
        .expect("env_line_allowed");
    let allowed = allowed
        .split("prefixes[] = {")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .expect("env_line_allowed's prefixes");
    let report = source("gpu/vk/vk_bug_report.cpp");
    let listed = report
        .split("std::string switches(")
        .nth(1)
        .and_then(|s| s.split("prefixes[] = {").nth(1))
        .and_then(|s| s.split('}').next())
        .expect("the report's prefixes");
    assert_eq!(
        listed, allowed,
        "the report lists switches by {listed}, hookshot_env.txt takes {allowed}"
    );
}

#[test]
fn the_report_writer_holds_no_game_data_or_keys() {
    let report = source("gpu/vk/vk_bug_report.cpp");
    for forbidden in [
        "hookshot_options.ini",
        "romfs",
        "image.bin",
        ".nup",
        "save/",
    ] {
        assert!(
            !report.contains(forbidden),
            "the report writer mentions {forbidden}: it must not copy game files"
        );
    }
    for secret in ["TOKEN", "KEY", "ROOM", "SECRET", "PASS", "SERVER", "USER"] {
        assert!(
            report.contains(secret),
            "the report's switch-value rule no longer names {secret}"
        );
    }
}

#[test]
fn the_report_writer_writes_a_folder_with_no_private_data() {
    let Some(cxx) = compiler() else {
        eprintln!("bug report: no C++ compiler, skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-bug-report-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) {
        "bug_report_test.exe"
    } else {
        "bug_report_test"
    });
    let mut build = Command::new(&cxx);
    build
        .arg("-std=c++20")
        .arg("-O1")
        .args(if cfg!(windows) {
            &[][..]
        } else {
            &["-pthread"][..]
        })
        .args(["-Wall", "-Wextra", "-Wpedantic", "-Werror"])
        .arg("-D_CRT_SECURE_NO_WARNINGS")
        .arg("-I")
        .arg(root().join("gpu/vk"))
        .arg(root().join("tests/bug_report_test.cpp"))
        .arg(root().join("gpu/vk/vk_bug_report.cpp"))
        .arg("-o")
        .arg(&exe);
    let built = build.output().expect("run the C++ compiler");
    assert!(
        built.status.success(),
        "compiling the bug report fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    let run_dir = dir.join("run");
    let ran = Command::new(&exe)
        .arg(&run_dir)
        .output()
        .expect("run the bug report fixture");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        ran.status.success(),
        "the bug report fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("bug report: ok"),
        "the fixture did not finish: {}",
        String::from_utf8_lossy(&ran.stdout)
    );
}
