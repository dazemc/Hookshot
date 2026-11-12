// Steam Deck shader preparation on a Linux PC (nx2/steamdeck_shaders.py, nx2/make_game.py --steamdeck,
// gpu/vk/vk_draw.cpp, vk_main.cpp, vk_progcache.cpp; upstream NotRexed/Hookshot 42f6beab, #2800). Game-free and opt-in:
// the tool reads only a built game folder, never a dump, and the tests read its flags and the renderer's batch hooks.
// Nothing is downloaded, run on a Deck, or read from the game.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn steamdeck_preparation_is_opt_in_and_game_free() {
    let repo = root();
    assert!(repo.join("nx2/steamdeck_shaders.py").is_file());
    let script = source("nx2/steamdeck_shaders.py");
    for flag in ["--game", "--deck", "--driver", "--cache"] {
        assert!(script.contains(flag), "the tool has no {flag}");
    }
    for part in [
        "def fetch_driver(",
        "def check_worker(",
        "def prepare_game(",
        "HOOKSHOT_DECK_PREPARE",
        "HOOKSHOT_DECK_BATCH_COUNT",
        "HOOKSHOT_DECK_BATCH_INDEX",
        "RX_VK_PREPARE_AGAIN",
        "hookshot_steamdeck_cache",
    ] {
        assert!(script.contains(part), "the tool misses {part}");
    }
    // (the folder's caches are checked against the Deck's GPU and pipeline-cache UUID before they are published)
    assert!(
        script.contains("cache_uuid")
            && script.contains("pipeline cache belongs to another GPU or driver")
    );
    // (no game data: a built folder's shader list and caches only)
    for forbidden in [
        "romfs",
        "exefs",
        ".nup",
        "image.bin",
        "OOT_DUMP",
        "game dump",
    ] {
        assert!(
            !script.contains(forbidden),
            "Steam Deck preparation reads game data ({forbidden})"
        );
    }

    let make_game = source("nx2/make_game.py");
    assert!(make_game.contains("\"--steamdeck\""));
    assert!(make_game.contains("--steamdeck cannot be combined with --no-prepare-shaders"));
    assert!(make_game.contains("steamdeck_shaders.prepare_game("));
    assert!(make_game.contains("env[\"RX_STEAMDECK\"] = \"1\""));
    assert!(make_game.contains("steamdeck_shaders.fetch_driver("));

    // (the renderer's side: the batches, the limited workers, the portable cache before the driver starts)
    let draw = source("gpu/vk/vk_draw.cpp");
    assert!(draw.contains("prepare_key_selected(p.key)"));
    assert!(draw.contains("std::erase_if(predicted, [](const auto& note) { return !prepare_key_selected(note->prog); });"));
    assert!(draw.contains("static bool portable_cache_ready()"));
    let cache = source("gpu/vk/vk_progcache.cpp");
    assert!(cache.contains("bool prepare_key_selected(uint64_t key)"));
    assert!(cache.contains("program_cache_release_loaded()"));
    assert!(cache.contains("program_cache_precompile_keys("));
    let main = source("gpu/vk/vk_main.cpp");
    assert!(main.contains("portable_driver_cache();"));
    assert!(main.contains("Preparing Steam Deck shaders"));

    let docs = source("docs/development.md");
    assert!(docs.contains("Steam Deck shaders on a Linux PC (opt-in)"));
    assert!(docs.contains("never a game dump"));
}
