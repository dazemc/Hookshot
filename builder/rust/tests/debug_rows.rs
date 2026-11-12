// The Debug tab's rows, game-free: the menu's real cheat_press, cheat_confirm and age_shown are compiled with stubs.
// Switch age is offered only once a save has told the game side Link's age (gpu/vk/vk_menu.cpp): with no save loaded the
// row used to queue OOT_AGE_ACTION, which waited and switched Link's age right after the next save was loaded. No game
// or GPU is needed; a machine with no C++ compiler skips it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

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

const FIXTURE: &str = r#"
#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cstring>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL %d: %s\n", __LINE__, #x); return 1; } } while (0)

constexpr int kCheatRows = 12;
constexpr int kLinkSpeeds = 5;
constexpr int kPouchItems = 10;
std::atomic<int> protect_hearts{0}, player_flight{-1}, free_camera{0}, link_age{-1}, cheat_action{0}, link_speed{0};
int clock_hour = 12, pouch_item = 0, pouch_delta = 1, page = 0;
bool open_now = true, teleport_follow = true;
void choose_inventory_filter(int) {}

/* AGE_SHOWN */
/* CONFIRM */
/* PRESS */

int main() {
    bool press[8] = {};
    bool changed = false;

    // (no save loaded: the age is not known, so confirm does nothing and offers no Switch)
    link_age.store(-1);
    press[7] = true;
    cheat_press(9, press, changed);
    CHECK(cheat_action.load() == 0);
    CHECK(open_now && !changed);
    CHECK(cheat_confirm(9) == nullptr);

    // (a save's child: confirm asks for the switch)
    link_age.store(0);
    cheat_press(9, press, changed);
    CHECK(cheat_action.load() == 999);
    CHECK(!open_now && changed);
    CHECK(cheat_confirm(9) && !std::strcmp(cheat_confirm(9), "Switch"));

    // (Left to the child while Link is the adult asks too; already the child asks for nothing)
    cheat_action.store(0);
    open_now = true, changed = false;
    press[7] = false, press[3] = true;
    link_age.store(1);
    cheat_press(9, press, changed);
    CHECK(cheat_action.load() == 999 && !open_now && changed);
    cheat_action.store(0);
    open_now = true, changed = false;
    link_age.store(0);
    cheat_press(9, press, changed);
    CHECK(cheat_action.load() == 0 && open_now && !changed);

    // (the other tools' rows still work: hearts and movement speed)
    cheat_action.store(0);
    press[3] = false, press[7] = true;
    cheat_press(0, press, changed);
    CHECK(protect_hearts.load() == 1);
    press[7] = false, press[3] = false, press[4] = true;
    cheat_press(11, press, changed);
    CHECK(link_speed.load() == 1);
    std::puts("ok");
    return 0;
}
"#;

#[test]
fn switch_age_is_offered_only_once_the_age_is_known() {
    let repo = root();
    let menu = fs::read_to_string(repo.join("gpu/vk/vk_menu.cpp")).expect("gpu/vk/vk_menu.cpp");
    let mut fixture = FIXTURE.to_string();
    for (marker, function) in [
        ("/* AGE_SHOWN */", block(&menu, "bool age_shown(int i) {")),
        ("/* CONFIRM */", block(&menu, "const char* cheat_confirm(int i) {")),
        ("/* PRESS */", block(&menu, "void cheat_press(int i, const bool* press, bool& changed) {")),
    ] {
        fixture = fixture.replace(marker, &function);
    }
    let Some(cxx) = compiler() else {
        eprintln!("skipping the Debug row fixture: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-debug-rows-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let source = dir.join("debug_rows.cpp");
    let exe = dir.join(if cfg!(windows) {
        "debug_rows.exe"
    } else {
        "debug_rows"
    });
    fs::write(&source, fixture).expect("write fixture");
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the Debug row fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}
