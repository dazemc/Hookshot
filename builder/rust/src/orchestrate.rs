//! One Rust build call: lift, compile, link the host and stage the folder.
//! The caller brings an already unpacked exefs/romfs pair.

use serde_json::{Value, json};

pub fn all_json(
    exefs: &str,
    romfs: &str,
    out: &str,
    work: &str,
    assets: &str,
    vk: &str,
    language: &str,
) -> Result<Value, String> {
    all_json_with(
        exefs,
        romfs,
        out,
        work,
        assets,
        vk,
        language,
        &mut |_, _| Ok(()),
    )
}

/// The same call, saying each stage as it starts. The stream CLI prints those
/// lines for the Flutter builder; the FFI keeps the silent form above.
#[allow(clippy::too_many_arguments)]
pub fn all_json_with(
    exefs: &str,
    romfs: &str,
    out: &str,
    work: &str,
    assets: &str,
    vk: &str,
    language: &str,
    say_stage: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<Value, String> {
    if exefs.is_empty() {
        return Err("build.all needs an exefs folder".into());
    }
    say_stage("read", "Reading your game files")?;
    let driver =
        crate::driver::plan_json_with(exefs, romfs, out, work, 0, true, true, true, say_stage)?;
    if driver["stage"] == "lift" {
        // (no translated chunks: never stage a folder that cannot play)
        return Ok(json!({"stage": "lift", "driver": driver}));
    }
    let compiled = &driver["compiled"];
    if compiled["failed"]
        .as_array()
        .is_some_and(|failed| !failed.is_empty())
    {
        return Ok(json!({"stage": "compile", "driver": driver}));
    }
    say_stage("compile", "Compiling the host")?;
    let host = crate::host_build::link_json(exefs, work, out)?;
    if !host["link"]["ok"].as_bool().unwrap_or(false) {
        return Ok(json!({"stage": "host-link", "driver": driver, "host": host}));
    }
    say_stage("finish", "Putting the game folder together")?;
    let package = crate::package::package_json(work, out, romfs, vk, assets, language)?;
    Ok(json!({
        "stage": "packaged",
        "driver": driver,
        "host": host,
        "package": package,
    }))
}
