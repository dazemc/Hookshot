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
    prepare: bool,
) -> Result<Value, String> {
    all_json_with(
        exefs,
        romfs,
        out,
        work,
        assets,
        vk,
        language,
        prepare,
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
    prepare: bool,
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
    // (the driver's work folder: the one it used, which is kit's when none was given)
    let work_folder = std::path::Path::new(driver["work"].as_str().unwrap_or(work));
    if let Some(failed) = host["compiled"]["failed"]
        .as_array()
        .filter(|failed| !failed.is_empty())
    {
        // (a host/ source that did not compile keeps the lift; a generated
        // file among them may be a bad lift, and the next build translates
        // again: #2675)
        let kept = crate::driver::settle_failure(work_folder, failed);
        return Ok(
            json!({"stage": "host-compile", "driver": driver, "host": host, "lift_kept": kept}),
        );
    }
    if !host["link"]["ok"].as_bool().unwrap_or(false) {
        // (as Python's forget_failed_build: a link that failed forgets the lift)
        crate::driver::forget_lift(&work_folder.join("sa").join("gen"));
        return Ok(
            json!({"stage": "host-link", "driver": driver, "host": host, "lift_kept": false}),
        );
    }
    // The Vulkan back end, as build.py's standalone build makes it: an
    // explicit path (the AppImage's prebuilt) is used as built; otherwise
    // the sources are compiled, and no header set is a message and a game
    // folder without a renderer, not a silent black window.
    let prebuilt = if !vk.is_empty() {
        Some(vk.to_owned())
    } else {
        std::env::var("HOOKSHOT_VK_PREBUILT")
            .ok()
            .filter(|path| !path.is_empty())
    };
    let vk_build = if let Some(prebuilt) = &prebuilt {
        let message = if std::path::Path::new(prebuilt).is_file() {
            format!("the Vulkan back end: {prebuilt}, as built")
        } else {
            format!("the Vulkan back end was not built: {prebuilt} is not there")
        };
        json!({
            "stage": "vulkan",
            "built": false,
            "reason": "prebuilt",
            "library": prebuilt,
            "message": message,
        })
    } else {
        say_stage("vulkan", "Building the Vulkan back end")?;
        crate::vk_build::build_at(&crate::kit::root(), work_folder)?
    };
    if !vk_build["built"].as_bool().unwrap_or(false)
        && matches!(
            vk_build["stage"].as_str(),
            Some("vulkan-compile" | "vulkan-link" | "vulkan-symbols")
        )
    {
        return Ok(json!({
            "stage": vk_build["stage"],
            "driver": driver,
            "host": host,
            "vk": vk_build,
        }));
    }
    say_stage("finish", "Putting the game folder together")?;
    let work_text = work_folder.to_str().unwrap_or(work);
    let package = crate::package::package_json(
        work_text,
        out,
        romfs,
        prebuilt.as_deref().unwrap_or(""),
        assets,
        language,
    )?;
    // The game's shader list, as Python's finish stage makes it: the first
    // start translates it instead of compiling each program as it meets it.
    // A failure leaves that work to the first start, never stops the build.
    // (the game's code as read, kept in the folder as Python's read stage
    // does: an update builds from it without asking for the game files again)
    let kept = std::path::Path::new(out).join(".hookshot_exefs");
    let source = std::path::Path::new(exefs);
    if !kept.is_dir() && source.is_dir() && source != kept {
        let mut linked = 0;
        crate::package::link_tree(source, &kept, &mut linked)?;
    }
    let shaders = match crate::shader::census(
        std::path::Path::new(romfs),
        Some(&std::path::Path::new(out).join("hookshot_shaders.bin")),
    ) {
        Ok(value) => value,
        Err(error) => json!({"error": error}),
    };
    let finish = if prepare {
        say_stage("prepare", "Preparing the game's shaders")?;
        let exe = package["exe"].as_str().unwrap_or_default();
        Some(crate::prepare::finish(
            &crate::kit::root(),
            std::path::Path::new(romfs),
            std::path::Path::new(out),
            std::path::Path::new(exe),
        ))
    } else {
        None
    };
    Ok(json!({
        "stage": "packaged",
        "driver": driver,
        "host": host,
        "vk": vk_build,
        "package": package,
        "shaders": shaders,
        "prepare": finish,
    }))
}
