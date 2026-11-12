// The Gameplay tab's Field of view is wired through the option API and the
// renderer's replay table to host/oot_fov.c (upstream NotRexed/Hookshot
// ba472ea3). Game-free: the real sources are read, nothing is compiled or run.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// An enum's value, from `enum { NAME = value` in vk_api.h.
fn option(api: &str, name: &str) -> i32 {
    let at = api
        .find(name)
        .unwrap_or_else(|| panic!("{name} is not declared"));
    let (_, after) = api[at..]
        .split_once('=')
        .unwrap_or_else(|| panic!("{name} has no value"));
    after
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or_default()
        .parse()
        .unwrap_or_else(|_| panic!("{name} has no number"))
}

#[test]
fn field_of_view_is_wired_to_the_menu_and_the_host() {
    let api = source("gpu/vk/vk_api.h");
    let options = source("gpu/vk/vk_menu.h");
    let menu = source("gpu/vk/vk_menu.cpp");
    let replay = source("gpu/vk/vk_replay.cpp");
    let host = source("host/oot_fov.c");

    assert_eq!(option(&api, "HOS_OPT_FOV"), 51);
    assert!(api.contains("49 and 50 are reserved for Walk mode"));
    let ids = [
        option(&api, "HOS_OPT_FOV"),
        option(&api, "HOS_OPT_PROMPTS"),
        option(&api, "HOS_OPT_CAMERA_SPEED"),
    ];
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        ids.len(),
        "FOV must not alias prompts or camera speed"
    );

    assert!(options.contains("int fov = 100;"));
    assert!(menu.contains(
        "const char* tx_fov(int v) { return v == 100 ? \"100% (original)\" : tx_percent(v); }"
    ));
    assert!(menu.contains("\"Field of view\", &opts.fov, 50, 150, 5, tx_fov"));
    for name in ["is_slider", "stops_at_ends"] {
        let line = menu[menu.find(&format!("bool {name}(")).expect(name)..]
            .split('\n')
            .next()
            .unwrap_or_default();
        assert!(
            line.contains("&opts.fov"),
            "{name} must treat FOV as a scale"
        );
    }
    assert!(replay.contains("if (id == HOS_OPT_FOV) return opts.fov;"));

    assert!(host.contains("void hos_oot_fov(Ctx *c, uint64_t camera)"));
    assert!(host.contains("rx_option(HOS_OPT_FOV, 100)"));
    assert!(
        host.contains("if (percent == 100 || percent < 50 || percent > 150 || !camera) return;")
    );
    assert!(
        !host.contains("recomp_store32(c, camera") && !host.contains("recomp_store64(c, camera"),
        "the authored camera angle must stay untouched"
    );
}
