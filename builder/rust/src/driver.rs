//! Build-driver orchestration over the stages already in Rust: check the
//! read game files, generate the runtime and compiled-in assets, and stop at
//! the first stage that is not built yet.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::kit;
use crate::logo_rgba;
use crate::nso;
use crate::rtgen;
use crate::ui_fonts;
use crate::win_manifest;

pub fn plan_json(
    exefs: &str,
    romfs: &str,
    out: &str,
    work: &str,
    lift_limit: usize,
) -> Result<Value, String> {
    let exefs = Path::new(exefs);
    let romfs = Path::new(romfs);
    let out = Path::new(out);
    let work = if work.is_empty() {
        kit::work()
    } else {
        Path::new(work).to_path_buf()
    };
    let root = kit::root();
    let modules = nso::check_modules(exefs)?;
    let build = nso::build_id(&exefs.join("main"))?;
    if !build.starts_with(nso::SUPPORTED_BUILD) {
        return Err(format!(
            "This is build {}. Hookshot supports build {}.",
            &build[..8.min(build.len())],
            nso::SUPPORTED_BUILD
        ));
    }
    let rt64 = work.join("rt64");
    rtgen::write(&rt64, &kit::kit())?;
    let gen_dir = work.join("build").join("gen");
    fs::create_dir_all(&gen_dir).map_err(|e| format!("{}: {e}", gen_dir.display()))?;
    ui_fonts::write(&gen_dir.join("ui_fonts.cpp"), &root)?;
    logo_rgba::write(
        &gen_dir.join("hookshot_logo.h"),
        &root.join("assets/hookshot.png"),
    )?;
    win_manifest::write(&gen_dir.join("hookshot_manifest.res"))?;
    let game_gen = work.join("sa").join("gen");
    if lift_limit > 0 && !game_gen.join("code_000.c").is_file() {
        crate::lift::write_json(
            exefs.to_str().unwrap_or_default(),
            game_gen.to_str().unwrap_or_default(),
            lift_limit,
            24 << 20,
        )?;
    }
    let chunks = fs::read_dir(&game_gen)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry.file_name().to_string_lossy().starts_with("code_")
                        && entry.file_name().to_string_lossy().ends_with(".c")
                })
                .count()
        })
        .unwrap_or(0);
    let romfs_files = fs::read_dir(romfs)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0);
    let stage = if chunks == 0 { "lift" } else { "compile" };
    Ok(json!({
        "stage": stage,
        "build": build,
        "modules": modules,
        "exefs": exefs.to_string_lossy(),
        "romfs": romfs.to_string_lossy(),
        "out": out.to_string_lossy(),
        "work": work.to_string_lossy(),
        "rt64": rt64.to_string_lossy(),
        "chunks": chunks,
        "romfs_entries": romfs_files,
        "ready": stage == "compile",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_at_the_lift_and_writes_the_assets() {
        let base =
            std::env::temp_dir().join(format!("hookshot-rust-driver-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let exefs = base.join("exefs");
        let romfs = base.join("romfs");
        fs::create_dir_all(&exefs).unwrap();
        fs::create_dir_all(&romfs).unwrap();
        let mut main = vec![0u8; 0x60];
        main[..4].copy_from_slice(b"NSO0");
        main[0x40..0x48].copy_from_slice(&[0xe0, 0xf3, 0xcc, 0xfa, 0, 0, 0, 0]);
        fs::write(exefs.join("main"), main).unwrap();
        let answer = plan_json(
            exefs.to_str().unwrap(),
            romfs.to_str().unwrap(),
            base.join("out").to_str().unwrap(),
            base.join("work").to_str().unwrap(),
            0,
        )
        .unwrap();
        assert_eq!(answer["stage"], "lift");
        assert_eq!(answer["ready"], false);
        assert!(base.join("work/rt64/a64.h").is_file());
        assert!(base.join("work/build/gen/ui_fonts.cpp").is_file());
        let _ = fs::remove_dir_all(base);
    }
}
