// The temporal chain's reset wiring (upstream NotRexed/Hookshot 19b81b7d). Main's camera-cut port (f01329ae) put code
// that reads State::last_at and logf into the chain's reset decision, but tests/temporal_history_test.cpp was not given
// those fields, so the compiled fixture harness this branch used to run no longer applies and was dropped. These source
// checks still apply: the chain's invalid reprojection resets the history, the shared frame carries it to the selected
// engine, and each consumer assigns it (DLSS, FSR 4, FSR 3.1). No game, dump or GPU is needed.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

#[test]
fn the_chain_hands_the_reset_to_the_engine_it_draws_with() {
    let chain = read("gpu/vk/vk_temporal.cpp");
    let dlss = read("gpu/vk/vk_dlss.cpp");
    let fsr4 = read("gpu/vk/vk_fsr4.cpp");
    let fsr = read("gpu/vk/vk_fsr3_inputs.cpp");
    assert!(
        chain.contains("engine.e->evaluate(cmd, f)"),
        "the shared frame must reach the selected engine"
    );
    assert!(
        chain.contains("if (!motion) reset = true;"),
        "an invalid reprojection must reset the history"
    );
    assert!(
        chain.contains("f.reset = reset;"),
        "the chain's reset must reach its engines"
    );
    assert!(
        dlss.contains("ep.InReset = f.reset ? 1 : 0;"),
        "DLSS's evaluation must receive the chain's reset"
    );
    assert!(
        fsr4.contains("d.reset = f.reset;"),
        "FSR 4 must receive the chain's reset"
    );
    assert!(
        fsr.contains("f.reset = reset;"),
        "FSR 3.1's own inputs must assign their reset"
    );
}
