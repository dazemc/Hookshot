// The Insert menu's Aspect ratio and Output resolution rows from the screen's display modes (gpu/vk/display_modes.h,
// vk_menu.cpp, host/hos_wide_size.h, host/hos_main.c; upstream NotRexed/Hookshot 511198c5, #2566). Game-free: the plain
// rules are compiled and run, and the menu and host wiring is read from the sources. No game, GPU or dump is needed.

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

fn cxx() -> Option<String> {
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
#include "display_modes.h"
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

using namespace mkwvk::display_modes;
#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL %d: %s\n", __LINE__, #x); return 1; } } while (0)

static bool has(const std::vector<Size>& v, uint32_t w, uint32_t h) {
    for (Size s : v) if (s.w == w && s.h == h) return true;
    return false;
}

int main() {
    // (the screen's modes: sides rounded down to 8, only the sizes the game accepts, once, by width then height)
    std::vector<Size> modes = {{1366, 768}, {1920, 1080}, {1920, 1080}, {2560, 1440}, {3440, 1440}, {640, 360}};
    std::vector<Size> offered_sizes = offered(modes);
    CHECK(has(offered_sizes, 1360, 768));
    CHECK(has(offered_sizes, 1920, 1080));
    CHECK(has(offered_sizes, 2560, 1440));
    CHECK(has(offered_sizes, 3440, 1440));
    CHECK(has(offered_sizes, 640, 360));
    CHECK(offered_sizes.size() == 5);   // (the duplicate 1920x1080 is once)
    for (size_t i = 1; i < offered_sizes.size(); i++)
        CHECK(offered_sizes[i - 1].w < offered_sizes[i].w ||
              (offered_sizes[i - 1].w == offered_sizes[i].w && offered_sizes[i - 1].h <= offered_sizes[i].h));

    // (the common sizes of the shapes the screen has: 1440p and 4K at 16:9 without DSR)
    std::vector<Size> all = with_common(offered({{1920, 1080}}));
    CHECK(has(all, 1920, 1080) && has(all, 2560, 1440) && has(all, 3840, 2160) && has(all, 5120, 2880));
    CHECK(!has(all, 3440, 1440));   // (a shape this screen has none of gets none)

    // (the shapes' names and order, narrowest first)
    CHECK(ratio_name({1280, 1024}) == "5:4");
    CHECK(ratio_name({1024, 768}) == "4:3");
    CHECK(ratio_name({1920, 1280}) == "3:2");
    CHECK(ratio_name({1920, 1200}) == "16:10");
    CHECK(ratio_name({1920, 1080}) == "16:9");
    CHECK(ratio_name({3440, 1440}) == "21:9");
    CHECK(ratio_name({3840, 1080}) == "32:9");
    CHECK(ratio_name({1600, 1024}) == "25:16");
    std::vector<Shape> shapes_v = shapes(all);
    CHECK(!shapes_v.empty() && shapes_v.front().name == "16:9");

    // (choosing a shape takes the screen's own size when it has that shape, else the height nearest the screen's)
    CHECK(pick(all, "16:9", {1920, 1080}) >= 0);
    std::vector<Size> wide = with_common(offered({{1920, 1080}, {2560, 1080}, {3440, 1440}}));
    const int at = pick(wide, "21:9", {1920, 1080});
    CHECK(at >= 0 && wide[at].w == 2560 && wide[at].h == 1080);
    std::vector<Size> three_two = with_common(offered({{1920, 1080}, {720, 480}}));
    const int at32 = pick(three_two, "3:2", {1920, 1080});
    CHECK(at32 >= 0 && three_two[at32].w == 1920 && three_two[at32].h == 1280);

    // (HOS_DISPLAY_MODES: the sizes in it, anything else skipped)
    std::vector<Size> listed = parse_list("1920x1080, 2560x1080;3440x1440, nope, 1280x720");
    CHECK(listed.size() == 4 && listed[1].w == 2560 && listed[2].w == 3440 && listed[3].h == 720);
    puts("ok");
    return 0;
}
"#;

#[test]
fn display_modes_offer_the_screens_sizes_and_their_shapes() {
    let repo = root();
    let Some(cxx) = cxx() else {
        eprintln!(
            "skipping the compiled display modes: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-display-modes-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let file = dir.join("display_modes.cpp");
    let exe = dir.join(if cfg!(windows) { "modes.exe" } else { "modes" });
    fs::write(&file, FIXTURE).expect("write fixture");
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", repo.join("gpu/vk").display()))
        .arg(format!("-I{}", repo.join("host").display()))
        .arg(&file)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", file.display());
    let run = Command::new(&exe).output().expect("run the fixture");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_menu_and_host_use_the_display_modes_and_keep_saved_options() {
    let menu = source("gpu/vk/vk_menu.cpp");
    let api = source("gpu/vk/vk_api.h");
    let host = source("host/hos_main.c");
    let wide = source("host/hos_wide_size.h");

    let aspect = menu
        .find("{TAB_GRAPHICS, \"Aspect ratio\", &opts.ultrawide")
        .expect("the Aspect ratio row is missing");
    let output = menu
        .find("{TAB_GRAPHICS, \"Output resolution\", &opts.output_res")
        .expect("the Output resolution row is missing");
    assert!(
        aspect < output,
        "the Aspect ratio row is not above the Output resolution"
    );
    assert!(
        menu.contains("outputs_sync();") && menu.contains("outputs_follow_screen();"),
        "the rows do not follow the screen's sizes at the start and while running"
    );
    assert!(
        menu.contains("getenv(\"HOS_DISPLAY_MODES\")"),
        "HOS_DISPLAY_MODES is not read"
    );
    assert!(
        menu.contains("if (r.value == &opts.ultrawide) return \"ultrawide\";"),
        "the Aspect ratio row is not saved as ultrawide="
    );
    assert!(
        menu.contains("aspect_follows_host(v) ? 1 : 0"),
        "the Aspect ratio row does not keep ultrawide= 0/1"
    );
    assert!(
        menu.contains("v >= 1 && v < (int)std::size(kOutputs) ? output_choice_of(kOutputs[v].w, kOutputs[v].h) : 0"),
        "the old numeric output_res values are not read as their sizes"
    );
    assert!(api.contains("#define MKWVK_API_VERSION 20"));
    assert!(
        api.contains("int (*screen_size)(int *width, int *height);"),
        "MkwVkHost has no screen_size"
    );
    assert!(
        host.contains("if (hos_output_own_shape(output_w, output_h)) want = (float)output_w / (float)output_h;"),
        "host/hos_main.c does not take an output size's own shape"
    );
    assert!(
        wide.contains("static inline int hos_output_own_shape(uint32_t w, uint32_t h)"),
        "host/hos_wide_size.h has no hos_output_own_shape"
    );
}
