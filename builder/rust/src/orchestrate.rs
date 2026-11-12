//! One Rust build call: lift, compile, link the host and stage the folder.
//! The caller brings an already unpacked exefs/romfs pair.

use serde_json::{Value, json};

use crate::timing::{self, Stage};

#[allow(clippy::too_many_arguments)]
pub fn all_json(
    exefs: &str,
    romfs: &str,
    out: &str,
    work: &str,
    assets: &str,
    vk: &str,
    language: &str,
    prepare: bool,
    dlss: bool,
    ngx: &str,
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
        dlss,
        ngx,
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
    dlss: bool,
    ngx: &str,
    say_stage: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<Value, String> {
    if exefs.is_empty() {
        return Err("build.all needs an exefs folder".into());
    }
    // (a portable context is checked before the long stages: an invalid or
    // missing marker stops here instead of after an hour of compiling)
    crate::portable::check_env()?;
    // (NVIDIA's DLSS, before the long stages: both platforms compile and
    // link their Vulkan back end with NGX, and a DLSS build with no SDK
    // folder cannot go on. HOOKSHOT_NGX_SDK alone asks for it too, as it
    // does in build.py)
    let requested_ngx = (!ngx.is_empty()).then(|| std::path::Path::new(ngx));
    let requested_ngx = crate::vk_build::ngx_folder(requested_ngx);
    let dlss = dlss || requested_ngx.is_some();
    let dlss_sdk = if dlss {
        Some(requested_ngx.ok_or(
            "NVIDIA DLSS needs NVIDIA's SDK folder: set HOOKSHOT_NGX_SDK, or pass the request's ngx \
             folder (build.ngx_fetch fetches it).",
        )?)
    } else {
        None
    };
    // (the build's clock, for build_times.json and the lines it ends with)
    timing::begin("build");
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
    let host_stage = Stage::new("host.build");
    let host = crate::host_build::link_json(exefs, work, out)?;
    host_stage.done();
    record_compile_counts("host", &host["compiled"]);
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
    // (a prebuilt backend has no NVIDIA DLSS in it: a DLSS build compiles the
    // backend instead, as make_game.py drops HOOKSHOT_VK_PREBUILT for one)
    let prebuilt = if dlss_sdk.is_some() {
        None
    } else if !vk.is_empty() {
        Some(vk.to_owned())
    } else {
        std::env::var("HOOKSHOT_VK_PREBUILT")
            .ok()
            .filter(|path| !path.is_empty())
    };
    let mut sdk_fetch = Value::Null;
    if let Some(sdk) = &dlss_sdk {
        // (every file is checked against its SHA-256: what is there and right
        // is not downloaded again, and a missing header is fetched too)
        say_stage("dlss", "Getting NVIDIA's DLSS SDK")?;
        sdk_fetch = crate::ngx_sdk::fetch(sdk, crate::ngx_sdk::Platform::current(), true)?;
    }
    let vk_stage = Stage::new("vk.build");
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
        let options = crate::vk_build::Options {
            ngx: dlss_sdk.clone(),
        };
        crate::vk_build::build_at_with_options(&crate::kit::root(), work_folder, &options)?
    };
    vk_stage.done();
    if vk_build["compiled"].is_object() {
        record_compile_counts("vk", &vk_build["compiled"]);
    }
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
    let ngx_text = dlss_sdk
        .as_ref()
        .map(|sdk| sdk.to_string_lossy().into_owned())
        .unwrap_or_default();
    // (with no language asked for, a launcher's first build takes the system's: make_game's rule)
    let language = crate::package::build_language(std::path::Path::new(out), language, prepare);
    let package_stage = Stage::new("package");
    let package = crate::package::package_json(
        work_text,
        out,
        romfs,
        prebuilt.as_deref().unwrap_or(""),
        assets,
        &language,
        &ngx_text,
    )?;
    package_stage.done();
    // (a DLSS build whose runtime did not land in <out>/ngx is not a DLSS build)
    if dlss_sdk.is_some() && package["ngx"]["placed"].as_bool() != Some(true) {
        return Ok(json!({
            "stage": "dlss-runtime",
            "driver": driver,
            "host": host,
            "vk": vk_build,
            "package": package,
        }));
    }
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
    let shader_stage = Stage::new("shaders");
    let shaders = match crate::shader::census(
        std::path::Path::new(romfs),
        Some(&std::path::Path::new(out).join("hookshot_shaders.bin")),
    ) {
        Ok(value) => value,
        Err(error) => json!({"error": error}),
    };
    shader_stage.done();
    let finish = if prepare {
        say_stage("prepare", "Preparing the game's shaders")?;
        let prepare_stage = Stage::new("prepare");
        let exe = package["exe"].as_str().unwrap_or_default();
        let finish = Some(crate::prepare::finish(
            &crate::kit::root(),
            std::path::Path::new(romfs),
            std::path::Path::new(out),
            std::path::Path::new(exe),
        ));
        prepare_stage.done();
        finish
    } else {
        None
    };
    let times = match timing::finish() {
        Some(mut times) => {
            // (a diagnostic file never stops a build: a read-only work folder
            // or a full disk leaves the build's own result alone)
            let (previous, error) = match timing::write(work_folder, &mut times) {
                Ok(previous) => (previous, Value::Null),
                Err(error) => (None, json!(error)),
            };
            json!({
                "total_seconds": times.total_seconds,
                "compile_seconds": times.compile_seconds,
                "previous_seconds": previous,
                "summary": times.summary_lines(),
                "error": error,
            })
        }
        None => Value::Null,
    };
    Ok(json!({
        "stage": "packaged",
        "driver": driver,
        "host": host,
        "vk": vk_build,
        "dlss": {
            "sdk": dlss_sdk.as_ref().map(|sdk| sdk.to_string_lossy().into_owned()),
            "fetched": sdk_fetch,
        },
        "package": package,
        "shaders": shaders,
        "prepare": finish,
        "times": times,
    }))
}

/// The compile result's counts, under a stage prefix (`host_compiled`, ...).
fn record_compile_counts(prefix: &str, compiled: &Value) {
    timing::count(
        &format!("{prefix}_compiled"),
        compiled["compiled"].as_u64().unwrap_or(0),
    );
    timing::count(
        &format!("{prefix}_cached"),
        compiled["cached"].as_u64().unwrap_or(0),
    );
    timing::count(
        &format!("{prefix}_failed"),
        compiled["failed"]
            .as_array()
            .map_or(0, |failed| failed.len() as u64),
    );
}
