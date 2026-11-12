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
    if exefs.is_empty() {
        return Err("build.all needs an exefs folder".into());
    }
    let driver = crate::driver::plan_json(exefs, romfs, out, work, 0, true, true, true)?;
    let compiled = &driver["compiled"];
    if compiled["failed"]
        .as_array()
        .is_some_and(|failed| !failed.is_empty())
    {
        return Ok(json!({"stage": "compile", "driver": driver}));
    }
    let host = crate::host_build::link_json(exefs, work, out)?;
    if !host["link"]["ok"].as_bool().unwrap_or(false) {
        return Ok(json!({"stage": "host-link", "driver": driver, "host": host}));
    }
    let package = crate::package::package_json(work, out, romfs, vk, assets, language)?;
    Ok(json!({
        "stage": "packaged",
        "driver": driver,
        "host": host,
        "package": package,
    }))
}
