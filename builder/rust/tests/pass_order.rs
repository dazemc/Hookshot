// Back-to-back passes and dispatches ordered by the game's barriers (upstream #2559, RX_VK_GAME_ORDER). The rule is
// gpu/vk/barrier_order.h: tests/barrier_order_test.cpp replays authored pass/dispatch sequences against it, game-free.
// These checks pin the renderer's use of it (the NVIDIA default, the switches over it, the conservative full barrier)
// and the frame log. No game, dump or GPU.

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
fn the_switch_defaults_and_the_conservative_path_are_wired() {
    let texture = read("gpu/vk/vk_texture.cpp");
    let draw = read("gpu/vk/vk_draw.cpp");
    let replay = read("gpu/vk/vk_replay.cpp");
    let order = read("gpu/vk/barrier_order.h");
    let header = read("gpu/vk/vk_texture.h");

    // The rule: NVIDIA by default, RX_VK_GAME_ORDER over it, and a parallel replay or an indirect dispatch keeps the barrier.
    assert!(
        order.contains("constexpr uint32_t kNvidia = 0x10de;"),
        "the rule does not name the vendor the game's barriers were placed for"
    );
    assert!(
        order.contains("return value ? !env_value_off(value) : vendor_id == kNvidia;"),
        "the game-order default is not NVIDIA with the switch over it"
    );
    assert!(
        order.contains("return !indirect && same_command_buffer && nothing_recorded_since && on && !parallel;"),
        "a dispatch right after another is not limited to direct, same-command-buffer, no-work, serial work"
    );
    assert!(
        order.contains("return same_command_buffer && nothing_recorded_since && on && !parallel;"),
        "a pass right after another is not limited to the same command buffer, no work since and a serial replay"
    );

    let game = block(&texture, "bool game_order() {");
    assert!(
        game.contains("getenv(\"RX_VK_GAME_ORDER\")")
            && game.contains("barrier_order::enabled(e, dev.props.vendorID)"),
        "game_order does not take RX_VK_GAME_ORDER through the vendor default"
    );
    assert!(
        game.contains("passes and dispatches right after others ordered by the game's barriers"),
        "game_order does not log its choice"
    );

    let pass = block(&texture, "void pass_start_barrier(");
    assert!(
        pass.contains("e ? !strcmp(e, \"game\") : game_order()"),
        "RX_VK_PASS_BARRIER=game or full no longer chooses the pass part over the game-order default"
    );
    assert!(
        pass.contains("barrier_order::pass_after_pass(cmd == last_pass_end_cmd,")
            && pass.contains("work_epoch.load(std::memory_order_relaxed) == last_pass_end_epoch, game,")
            && pass.contains("par_active.load(std::memory_order_relaxed)"),
        "pass_start_barrier no longer asks the rule with the pass end, the work count and a parallel replay"
    );
    assert!(
        pass.contains("full_barrier(cmd, \"barrier (pass start)\");"),
        "the conservative pass-start barrier is gone"
    );
    assert!(
        header.contains("RX_VK_GAME_ORDER") && header.contains("RX_VK_PASS_BARRIER=full"),
        "vk_texture.h does not document the switch and the forced-full pass barrier"
    );

    // A direct dispatch right after another: only the rule's case leaves out the full barrier; every other keeps it.
    let at = draw
        .find("const bool after_dispatch = barrier_order::dispatch_after_dispatch(")
        .expect("dispatch does not ask the back-to-back rule");
    let decision = &draw[at..at + 700];
    assert!(
        decision.contains("indirect != 0, cmd == last_dispatch_cmd")
            && decision.contains("work_epoch.load(std::memory_order_relaxed) == last_dispatch_epoch"),
        "the dispatch decision does not test the dispatch's kind, command buffer and work count"
    );
    assert!(
        decision.contains("game_order(), par_active.load(std::memory_order_relaxed))"),
        "the dispatch decision does not take the switch and a parallel replay"
    );
    assert!(
        decision.contains("if (work_since_barrier && !no_order && !after_dispatch) full_barrier(cmd, \"barrier (dispatch)\");"),
        "every other dispatch no longer gets its full barrier"
    );
    assert!(
        decision.contains("else if (after_dispatch && ++st_dispatch_unordered % 50000 == 1)"),
        "the dispatches left out are not counted"
    );
    assert!(
        draw.contains("static const bool no_order = env_on(\"RX_VK_NO_DISPATCH_ORDER\");"),
        "RX_VK_NO_DISPATCH_ORDER no longer keeps every dispatch barrier"
    );

    // The count is noted after the dispatch's own command, and a new recording forgets the previous one's.
    let note = draw
        .find("last_dispatch_cmd = cmd, last_dispatch_epoch = work_epoch.load(std::memory_order_relaxed);")
        .expect("the dispatch's work count is not noted");
    let before = &draw[..note];
    let direct = before.rfind("vkCmdDispatch(cmd, x, y, z);").expect("no direct dispatch");
    let indirect = before
        .rfind("vkCmdDispatchIndirect(cmd, args.buffer, args.offset);")
        .expect("no indirect dispatch");
    assert!(
        note - direct < 400 && note - indirect < 400,
        "the work count is not noted right after the dispatch commands"
    );
    let begin = block(&draw, "void draw_begin_recording(");
    assert!(
        begin.contains("last_dispatch_cmd = VK_NULL_HANDLE, last_dispatch_epoch = 0;"),
        "a new recording keeps the previous one's dispatch"
    );

    // The frame log counts both relaxations, so a run says whether they happened.
    assert!(
        replay.contains("draw_stats5(&unordered)")
            && replay.contains("dispatches right after another with no barrier of their own"),
        "vk_replay.cpp's frame log does not count the dispatches left out"
    );
    assert!(
        replay.contains("RX_VK_GAME_ORDER / RX_VK_PASS_BARRIER=game"),
        "vk_replay.cpp's frame log still names only RX_VK_PASS_BARRIER"
    );
}

#[test]
fn the_switch_and_the_rules_are_documented() {
    let development = read("docs/development.md");
    for text in [
        "RX_VK_GAME_ORDER=0",
        "RX_VK_GAME_ORDER=1",
        "On by default on NVIDIA",
        "RX_VK_PASS_BARRIER=game",
        "RX_VK_NO_DISPATCH_ORDER=1",
    ] {
        assert!(
            development.contains(text),
            "docs/development.md does not document {text}"
        );
    }

    let fork = read("docs/fork.md");
    assert!(
        fork.contains("#2559") && fork.contains("RX_VK_GAME_ORDER"),
        "docs/fork.md does not record the port and its switch"
    );
    assert!(
        fork.contains("tests/barrier_order_test.cpp") && fork.contains("builder/rust/tests/pass_order.rs"),
        "docs/fork.md does not name the fixtures"
    );
}

#[test]
fn the_barrier_order_rule_replays_authored_sequences() {
    let Some(cxx) = compiler() else {
        eprintln!("skipping the barrier order fixture: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let repo = root();
    let dir = std::env::temp_dir().join(format!(
        "hookshot-barrier-order-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let source = repo.join("tests/barrier_order_test.cpp");
    let exe = dir.join(if cfg!(windows) {
        "barrier_order_test.exe"
    } else {
        "barrier_order_test"
    });
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", repo.join("gpu/vk").display()))
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cxx} failed on {}:\n{}",
        source.display(),
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the fixture");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
    assert!(run.status.success(), "fixture failed:\n{text}");
    assert!(
        text.contains("PASS: back-to-back passes and dispatches ordered by the game's barriers"),
        "fixture did not pass:\n{text}"
    );
}
