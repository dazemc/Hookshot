//! Build-driver orchestration over the stages already in Rust: check the
//! read game files, generate the runtime and compiled-in assets, and stop at
//! the first stage that is not built yet.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use sha1::Digest;

use crate::kit;
use crate::logo_rgba;
use crate::nso;
use crate::rtgen;
use crate::ui_fonts;
use crate::win_manifest;

fn take_function(source: &str, signature: &str) -> Option<String> {
    let a = source.find(signature)?;
    let b = source[a..].find("\n}\n")? + a + 3;
    Some(source[a..b].to_owned())
}

fn stamp_one(text: &mut String, path: &Path) {
    let modified = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_nanos())
        .unwrap_or(0);
    let len = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    text.push_str(&format!("{} {} {modified}\n", path.display(), len));
}

/// What a link was made from: its objects and the compiler, by path, size and
/// modification time. A cached build whose objects did not change can keep
/// the 400 MB library instead of linking it again.
fn link_stamp(objects: &[String], cc: &Path, lib: &Path) -> String {
    let mut text = format!("hookshot-link-v1 {}\n", env!("CARGO_PKG_VERSION"));
    let mut linker = sha1::Sha1::new();
    linker.update(include_bytes!("linker.rs"));
    text.push_str(&format!("linker {:x}\n", linker.finalize()));
    stamp_one(&mut text, cc);
    let mut objects = objects.to_vec();
    objects.sort();
    for object in &objects {
        stamp_one(&mut text, Path::new(object));
    }
    text.push_str(&lib.to_string_lossy());
    text
}

/// The names the lift and the build write into the work folder: the files a
/// compile failure can blame on the translation, not on a repository source.
pub fn generated_names(work: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for folder in [
        work.join("sa").join("gen"),
        work.join("rt64"),
        work.join("build").join("gen"),
    ] {
        if let Ok(entries) = fs::read_dir(folder) {
            names.extend(
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned()),
            );
        }
    }
    names
}

/// Whether a failed build keeps its translation (#2675): when every file that
/// did not compile is a repository source, the translation is not what failed,
/// and the next build compiles only the files not compiled yet. A lifted
/// `code_*` or a generated file that failed may be a lift cut short (issue
/// 1197): the next build translates the game again.
pub fn keeps_lift(failed: &[String], generated: &[String]) -> bool {
    !failed
        .iter()
        .any(|name| name.starts_with("code_") || generated.iter().any(|owned| owned == name))
}

/// Forget the lift: its C and the tables made from it. The next build finds no
/// translated chunk and translates the game again.
pub fn forget_lift(game_gen: &Path) {
    let Ok(entries) = fs::read_dir(game_gen) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name.starts_with("code_") && name.ends_with(".c"))
            || matches!(name.as_str(), "a64_table.c" | "a64_hooks.c" | "a64_fpx.h")
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// A compile result's failed entries as the names Python's build.py prints
/// ("FAILED <name>"): the source's file name. None when a failure says no
/// source, which is no repository source either.
fn failure_names(failed: &[Value]) -> Option<Vec<String>> {
    failed
        .iter()
        .map(|failure| {
            Path::new(failure["src"].as_str()?)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect()
}

/// Apply the rule to a stage's failed list: keep the translation for a
/// repository source that did not compile, forget it for a lifted or generated
/// file. Says whether it was kept.
pub fn settle_failure(work: &Path, failed: &[Value]) -> bool {
    let keep = failure_names(failed)
        .map(|names| keeps_lift(&names, &generated_names(work)))
        .unwrap_or(false);
    if !keep {
        forget_lift(&work.join("sa").join("gen"));
    }
    keep
}

/// The game code's compile jobs: every numbered chunk as the compile units
/// `chunks::write_units` cut it into (the chunks themselves are not compiled),
/// and a hot chunk (split_hot's pieces) as it is. `obj_dir` gets no job: it is
/// read only to drop the objects of units an earlier build made.
#[allow(clippy::too_many_arguments)]
fn game_code_jobs(
    game_gen: &Path,
    units: &Path,
    obj_dir: &Path,
    cache: &Path,
    cc: &str,
    flags: &[String],
    deps_file: &Path,
    budget: u64,
) -> Result<(Vec<Value>, usize), String> {
    let (unit_sources, _costs) = crate::chunks::write_units(game_gen, units, budget)?;
    let count = unit_sources.len();
    let mut jobs = Vec::with_capacity(count);
    for src in &unit_sources {
        let obj = obj_dir.join(format!("{}.o", src.file_name().unwrap().to_string_lossy()));
        jobs.push(json!({
            "cc": cc,
            "src": src.to_string_lossy(),
            "obj": obj.to_string_lossy(),
            "cache": cache.to_string_lossy(),
            "flags": flags,
            "deps_file": deps_file,
        }));
    }
    if let Ok(entries) = fs::read_dir(game_gen) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("code_") || !name.ends_with(".c") {
                continue;
            }
            if crate::chunks::is_compile_unit_source(&name) {
                continue; // (compiled as its units above)
            }
            let src = entry.path();
            let obj = obj_dir.join(format!("{name}.o"));
            jobs.push(json!({
                "cc": cc,
                "src": src.to_string_lossy(),
                "obj": obj.to_string_lossy(),
                "cache": cache.to_string_lossy(),
                "flags": flags,
                "deps_file": deps_file,
            }));
        }
    }
    Ok((jobs, count))
}

/// Forget the objects of lifted sources this build does not compile: a game
/// update leaves fewer chunks, and a chunk whose cuts moved leaves pieces that
/// are not in the link list. The link takes every object in the folder
/// (`fs::read_dir(work/obj)` below), so a stale one would be linked in.
fn forget_stale_objects(obj_dir: &Path, jobs: &[Value]) {
    let keep: std::collections::HashSet<&str> = jobs
        .iter()
        .filter_map(|job| job["obj"].as_str())
        .filter_map(|obj| Path::new(obj).file_name())
        .filter_map(|name| name.to_str())
        .collect();
    let Ok(entries) = fs::read_dir(obj_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("code_") || !name.ends_with(".o") || keep.contains(name.as_str()) {
            continue;
        }
        let _ = fs::remove_file(entry.path());
        let _ = fs::remove_file(entry.path().with_extension("o.sha1"));
    }
}

fn suyu_c() -> Result<String, String> {
    let rc = crate::emitter::runtime_text(1)?;
    let aes_start = rc
        .find("static uint8_t recomp_gmul")
        .ok_or("suyu AES start missing")?;
    let sbox = rc[aes_start..]
        .find("const uint8_t* recomp_aes_sbox")
        .ok_or("suyu AES sbox missing")?
        + aes_start;
    let aes_end = rc[sbox..].find("\n}\n").ok_or("suyu AES end missing")? + sbox + 3;
    let umulh = take_function(&rc, "uint64_t recomp_umulh(uint64_t a,uint64_t b){")
        .ok_or("suyu umulh missing")?;
    let smulh = take_function(&rc, "uint64_t recomp_smulh(uint64_t a,uint64_t b){")
        .ok_or("suyu smulh missing")?;
    Ok(format!(
        "/* Generated by the Hookshot Rust engine from suyu/arm64_to_c.h (GPL-2.0-or-later). Do not edit. */\n#define A64_RT_IMPL\n#include \"a64.h\"\n{}\n{}\n{}\n{}\n{}",
        &rc[aes_start..aes_end],
        umulh,
        smulh,
        crate::emitter::runtime_text(2)?,
        crate::emitter::runtime_text(3)?
    ))
}

pub fn plan_json(
    exefs: &str,
    romfs: &str,
    out: &str,
    work: &str,
    lift_limit: usize,
    lift_all: bool,
    standalone: bool,
    compile: bool,
) -> Result<Value, String> {
    plan_json_with(
        exefs,
        romfs,
        out,
        work,
        lift_limit,
        lift_all,
        standalone,
        compile,
        &mut |_, _| Ok(()),
    )
}

/// The same build, saying which stage begins as it goes (the builder's
/// `--stream` CLI shows those lines while the build runs).
#[allow(clippy::too_many_arguments)]
pub fn plan_json_with(
    exefs: &str,
    romfs: &str,
    out: &str,
    work: &str,
    lift_limit: usize,
    lift_all: bool,
    standalone: bool,
    compile: bool,
    say_stage: &mut dyn FnMut(&str, &str) -> Result<(), String>,
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
    let read = crate::timing::Stage::new("driver.read");
    let modules = nso::check_modules(exefs)?;
    let build = nso::build_id(&exefs.join("main"))?;
    read.done();
    if !build.starts_with(nso::SUPPORTED_BUILD) {
        return Err(format!(
            "This is build {}. Hookshot supports build {}.",
            &build[..8.min(build.len())],
            nso::SUPPORTED_BUILD
        ));
    }
    say_stage("read", "Reading your game files")?;
    let assets = crate::timing::Stage::new("driver.assets");
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
    assets.done();
    let game_gen = work.join("sa").join("gen");
    let mut lift_needed = (lift_all || lift_limit > 0) && !game_gen.join("code_000.c").is_file();
    if standalone && game_gen.join("code_000.c").is_file() {
        let hooks = fs::read_to_string(game_gen.join("a64_hooks.c")).unwrap_or_default();
        if !hooks.contains("TAILCALL(") {
            if lift_all || lift_limit > 0 {
                forget_lift(&game_gen);
                lift_needed = true;
            } else {
                return Err(format!(
                    "{} holds a lift made without the host hooks; remove its code_*.c files or use another work folder.",
                    game_gen.display()
                ));
            }
        }
    }
    if lift_needed {
        say_stage("lift", "Translating the game to PC code")?;
        let limit = if lift_all { 0 } else { lift_limit };
        crate::lift::write_json(
            exefs.to_str().unwrap_or_default(),
            game_gen.to_str().unwrap_or_default(),
            limit,
            24 << 20,
            standalone,
        )?;
    }
    let mut standalone_info = Value::Null;
    if standalone {
        let standalone_stage = crate::timing::Stage::new("driver.standalone");
        fs::create_dir_all(&game_gen).map_err(|e| format!("{}: {e}", game_gen.display()))?;
        crate::standalone::write_json(
            exefs.to_str().unwrap_or_default(),
            game_gen.to_str().unwrap_or_default(),
            "",
        )?;
        let image_dir = work.join("sa").join("image");
        standalone_info = crate::standalone::image_json(
            exefs.to_str().unwrap_or_default(),
            image_dir.to_str().unwrap_or_default(),
            "",
        )?;
        standalone_stage.done();
    }
    if game_gen.is_dir() {
        let mut suyu_h = crate::emitter::runtime_text(0)?;
        if let (Some(a), Some(b)) = (
            suyu_h.find("typedef struct RecompHostMem {"),
            suyu_h.find("/* Where this module is actually loaded"),
        ) {
            suyu_h.replace_range(a..b, "");
        }
        suyu_h = suyu_h.replace("extern uint64_t g_module_base;\n", "");
        for proto in [
            "int  recomp_save_init",
            "int  recomp_save_write",
            "int  recomp_save_read",
            "int  recomp_save_delete",
            "int  recomp_save_exists",
            "int  recomp_load_segments",
        ] {
            if let Some(a) = suyu_h.find(proto) {
                if let Some(b) = suyu_h[a..].find('\n') {
                    suyu_h.replace_range(a..a + b + 1, "");
                }
            }
        }
        let noesc = regex::Regex::new(r"GuestContext\s*\*\s*").unwrap();
        suyu_h = noesc
            .replace_all(&suyu_h, "GuestContext *A64_NOESC ")
            .into_owned();
        suyu_h = format!(
            "/* Generated by the Hookshot Rust engine from suyu/arm64_to_c.h (GPL-2.0-or-later). Do not edit. */\n{suyu_h}"
        );
        let suyu_c = suyu_c()?;
        fs::write(game_gen.join("a64_suyu.h"), suyu_h)
            .map_err(|e| format!("{}: {e}", game_gen.join("a64_suyu.h").display()))?;
        fs::write(game_gen.join("a64_suyu.c"), suyu_c)
            .map_err(|e| format!("{}: {e}", game_gen.join("a64_suyu.c").display()))?;
        if !game_gen.join("a64_fpx.h").is_file() {
            fs::write(
                game_gen.join("a64_fpx.h"),
                "/* generated by the Rust engine */\n#pragma once\n",
            )
            .map_err(|e| e.to_string())?;
        }
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
    let mut stage = if chunks == 0 { "lift" } else { "compile" };
    let mut compiled = Value::Null;
    let mut lift_kept = Value::Null;
    let mut unit_count = 0usize;
    let mut parallel = 0usize;
    if compile && chunks > 0 {
        let compile_stage = crate::timing::Stage::new("driver.compile");
        say_stage("compile", "Compiling")?;
        let cc = crate::toolchain::find("clang", false);
        // (the planning is the sources read and hashed for their object keys,
        // before the compilers start: where a cold build's first seconds go)
        let planning = crate::timing::Stage::new("compile.planning");
        // (diagnostic: reuse the old empty-deps keys while validating a
        // hand-made cache; normal builds always hash the generated headers)
        let game_deps = if std::env::var("HOOKSHOT_NO_GAME_DEPS").ok().as_deref() == Some("1") {
            std::path::PathBuf::new()
        } else {
            let path = work.join("build").join("game_deps.bin");
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let mut deps = Vec::new();
            for folder in [&rt64, &gen_dir, &game_gen] {
                let mut stack = vec![folder.clone()];
                while let Some(dir) = stack.pop() {
                    let Ok(entries) = fs::read_dir(&dir) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if entry_path.is_dir() {
                            stack.push(entry_path);
                        } else if entry_path.extension().and_then(|ext| ext.to_str()) == Some("h") {
                            deps.extend(fs::read(&entry_path).unwrap_or_default());
                            deps.push(0);
                        }
                    }
                }
            }
            fs::write(&path, deps).map_err(|e| format!("{}: {e}", path.display()))?;
            path
        };
        let code_flags = {
            let mut flags = vec!["-O1".to_owned(), "-g0".to_owned()];
            if !crate::toolchain::windows() {
                flags.push("-fPIC".to_owned());
            }
            flags.extend([
                "-Wno-everything".to_owned(),
                "-ffp-contract=off".to_owned(),
                "-mcx16".to_owned(),
                "-DA64_POLLING".to_owned(),
                "-DA64_WRITE_TRACK".to_owned(),
            ]);
            // (a Windows cross build makes COFF objects for the same
            // host link: nx2/build.py's flags() starts with target())
            let target = crate::toolchain::native_target(false);
            if target.is_empty() && crate::toolchain::windows() {
                // (discovered from PATH: no HOOKSHOT_MINGW to name the target)
                flags.extend(crate::host_build::mingw_target_flags());
            } else {
                flags.extend(target);
            }
            // (the level this PC runs, and a Steam Deck's Zen 2 tune: #2492)
            flags.extend(crate::host::cpu_flags());
            flags.extend([
                format!("-I{}", rt64.display()),
                format!("-I{}", gen_dir.display()),
                format!("-I{}", game_gen.display()),
            ]);
            flags
        };
        let (mut jobs, units) = game_code_jobs(
            &game_gen,
            &work.join("units"),
            &work.join("obj"),
            &work.join("objcache"),
            &cc.to_string_lossy(),
            &code_flags,
            &game_deps,
            crate::chunks::UNIT_BYTES,
        )?;
        unit_count = units;
        // (an earlier build's whole-chunk or unit objects are not in this
        // list: the link takes every object in work/obj, so they go)
        forget_stale_objects(&work.join("obj"), &jobs);
        let mut runtime_files = vec!["a64_suyu.c", "a64_rt.c", "a64_table.c", "a64_hooks.c"];
        if standalone {
            runtime_files.extend(["a64_stubs.c", "hos_image.c", "nvn2_exports.c"]);
        }
        for name in runtime_files {
            let src = if name == "a64_rt.c" {
                rt64.join(name)
            } else {
                game_gen.join(name)
            };
            if !src.is_file() {
                continue;
            }
            let obj = work.join("obj").join(format!("{name}.o"));
            jobs.push(json!({
                "cc": cc.to_string_lossy(),
                "src": src.to_string_lossy(),
                "obj": obj.to_string_lossy(),
                "cache": work.join("objcache").to_string_lossy(),
                "flags": code_flags,
                "deps_file": game_deps,
            }));
        }
        // (the whole list: the runtime's files (a64_table.c is 16 MB of tables
        // and peaks at ~0.5 GB) count too)
        let needs: Vec<u64> = jobs
            .iter()
            .filter_map(|job| job["src"].as_str())
            .map(|src| crate::compile::compile_need_mb(Path::new(src)))
            .collect();
        // (RX_JOBS still overrides; else the memory available, not the total,
        // decides how many of these fit, wave by wave: the giant functions
        // run first at their 2-3 GB each, then the 6 MB units at ~0.5 GB
        // share the cores the giants held back)
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let available = crate::toolchain::available_mb();
        let asked = std::env::var("RX_JOBS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .map(|jobs| jobs.max(1));
        let memory_scheduled = asked.is_none() && available.is_some();
        parallel = match asked {
            Some(jobs) => jobs,
            None if memory_scheduled => cores,
            None => crate::compile::parallel_for(&needs, available, cores),
        };
        let cache_path = work.join("objcache");
        let cache = cache_path.to_string_lossy();
        let request = if memory_scheduled {
            json!({"jobs": jobs, "cache": cache, "parallel": cores, "needs": needs, "available_mb": available})
        } else {
            json!({"jobs": jobs, "cache": cache, "parallel": parallel})
        };
        planning.done();
        let compiling = crate::timing::Stage::new("compile.compiling");
        compiled = crate::compile::many_json(&request, &root)?;
        compiling.done();
        crate::timing::count("chunks", chunks as u64);
        crate::timing::count("units", unit_count as u64);
        crate::timing::count("parallel", parallel as u64);
        crate::timing::count("game_compiled", compiled["compiled"].as_u64().unwrap_or(0));
        crate::timing::count("game_cached", compiled["cached"].as_u64().unwrap_or(0));
        crate::timing::count(
            "game_failed",
            compiled["failed"]
                .as_array()
                .map_or(0, |failed| failed.len() as u64),
        );
        crate::timing::set_compile_seconds(compiled["seconds"].as_f64().unwrap_or(0.0));
        let failed = compiled["failed"].as_array().cloned().unwrap_or_default();
        if failed.is_empty() {
            let objects: Vec<String> = fs::read_dir(work.join("obj"))
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "o"))
                        .map(|entry| entry.path().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            let lib = work.join("build").join("hookshot_game.so");
            let link_stage = crate::timing::Stage::new("driver.link");
            let cc = crate::toolchain::find("clang", false);
            let stamp_path = lib.with_extension("link");
            let stamp = link_stamp(&objects, &cc, &lib);
            let cached = lib.is_file()
                && fs::read_to_string(&stamp_path)
                    .map(|old| old == stamp)
                    .unwrap_or(false);
            let link = if cached {
                json!({"ok": true, "out": lib.to_string_lossy(), "output": "", "undefined": [], "cached": true})
            } else {
                crate::linker::link_json(&json!({
                    "cc": cc.to_string_lossy(),
                    "out": lib.to_string_lossy(),
                    "objects": objects,
                    "args": ["-shared", "-fPIC", "-o", lib.to_string_lossy()],
                }))?
            };
            if link["ok"].as_bool().unwrap_or(false) && !cached {
                let _ = fs::write(&stamp_path, &stamp);
            }
            link_stage.done();
            let copy_stage = crate::timing::Stage::new("driver.copy");
            if link["ok"].as_bool().unwrap_or(false) {
                let package = out.join("bin");
                fs::create_dir_all(&package).map_err(|e| e.to_string())?;
                if lib.is_file() {
                    fs::copy(&lib, package.join("hookshot_game.so")).map_err(|e| e.to_string())?;
                }
                fs::write(
                    package.join("build_manifest.json"),
                    serde_json::to_vec_pretty(
                        &json!({"stage":"package","chunks":chunks,"library":lib.to_string_lossy()}),
                    )
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                stage = "package";
                lift_kept = json!(true);
            } else {
                // (lifted C another builder left, or a cached runtime object
                // from another version: the next build translates again)
                forget_lift(&game_gen);
                lift_kept = json!(false);
            }
            copy_stage.done();
            compiled["link"] = link;
        } else {
            // (a host/ source that did not compile is no reason to translate
            // the game again; a lifted code_* that did is: #2675)
            lift_kept = json!(settle_failure(&work, &failed));
        }
        compile_stage.done();
    }
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
        "units": unit_count,
        "parallel": parallel,
        "standalone": standalone,
        "standalone_info": standalone_info,
        "lift_kept": lift_kept,
        "compiled": compiled,
        "romfs_entries": romfs_files,
        "ready": stage == "compile",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(base: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let exefs = base.join("exefs");
        let romfs = base.join("romfs");
        fs::create_dir_all(&exefs).unwrap();
        fs::create_dir_all(&romfs).unwrap();
        let mut main = vec![0u8; 0x60];
        main[..4].copy_from_slice(b"NSO0");
        main[0x40..0x48].copy_from_slice(&[0xe0, 0xf3, 0xcc, 0xfa, 0, 0, 0, 0]);
        fs::write(exefs.join("main"), main).unwrap();
        (exefs, romfs)
    }

    fn base(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "hookshot-rust-driver-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// A compile-only run writes the suyu runtime, which needs the vendored
    /// translator built (`python -I third_party/a64-kit/lift/build_emit.py`).
    fn has_emitter() -> bool {
        crate::emitter::runtime_text(0).is_ok()
    }

    #[test]
    fn a_repository_source_that_failed_keeps_the_lift() {
        let generated = vec![
            "hos_image.c".to_owned(),
            "a64_rt.c".to_owned(),
            "a64_table.c".to_owned(),
            "ui_fonts.cpp".to_owned(),
        ];
        assert!(keeps_lift(&["hos_update.c".to_owned()], &generated));
        assert!(keeps_lift(
            &["hos_main.c".to_owned(), "vk_texture.cpp".to_owned()],
            &generated
        ));
        assert!(keeps_lift(&[], &generated));
    }

    #[test]
    fn a_lifted_or_generated_file_that_failed_forgets_it() {
        let generated = vec!["hos_image.c".to_owned(), "a64_rt.c".to_owned()];
        for name in ["code_021.c", "code_hot_3.c", "hos_image.c", "a64_rt.c"] {
            assert!(
                !keeps_lift(&["hos_update.c".to_owned(), name.to_owned()], &generated),
                "{name}"
            );
        }
    }

    #[test]
    fn a_failed_stage_forgets_the_lift_unless_a_repository_source_failed() {
        let base = base("settle-failure");
        let work = base.join("work");
        let game_gen = work.join("sa").join("gen");
        fs::create_dir_all(&game_gen).unwrap();
        fs::create_dir_all(work.join("rt64")).unwrap();
        fs::create_dir_all(work.join("build").join("gen")).unwrap();
        for name in [
            "code_000.c",
            "code_hot_1.c",
            "a64_table.c",
            "a64_hooks.c",
            "a64_fpx.h",
            "a64_suyu.c",
            "notes.txt",
        ] {
            fs::write(game_gen.join(name), "").unwrap();
        }
        let source =
            json!([{"index": 0, "src": "/repo/host/hos_update.c", "error": "error: nope"}]);
        assert!(settle_failure(&work, source.as_array().unwrap()));
        assert!(game_gen.join("code_000.c").is_file());

        let lifted = json!([{
            "index": 0,
            "src": game_gen.join("code_000.c").to_string_lossy(),
            "error": "error: nope",
        }]);
        assert!(!settle_failure(&work, lifted.as_array().unwrap()));
        for gone in [
            "code_000.c",
            "code_hot_1.c",
            "a64_table.c",
            "a64_hooks.c",
            "a64_fpx.h",
        ] {
            assert!(!game_gen.join(gone).exists(), "{gone} was not forgotten");
        }
        // (the driver's own generated runtime is not the lift)
        for kept in ["a64_suyu.c", "notes.txt"] {
            assert!(game_gen.join(kept).is_file(), "{kept} was removed");
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_failure_that_names_no_source_forgets_the_lift() {
        let base = base("unnamed-failure");
        let work = base.join("work");
        let game_gen = work.join("sa").join("gen");
        fs::create_dir_all(&game_gen).unwrap();
        fs::write(game_gen.join("code_000.c"), "").unwrap();
        let failure = json!([{"index": 0, "error": "the compiler could not start"}]);
        assert!(!settle_failure(&work, failure.as_array().unwrap()));
        assert!(!game_gen.join("code_000.c").exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_completed_lift_is_not_translated_again() {
        if !has_emitter() {
            return; // (no vendored translator here: the compile stage cannot run)
        }
        let base = base("second-build");
        let (exefs, romfs) = dump(&base);
        let work = base.join("work");
        let game_gen = work.join("sa").join("gen");
        fs::create_dir_all(&game_gen).unwrap();
        fs::write(game_gen.join("code_000.c"), "/* authored lifted C */\n").unwrap();
        fs::write(game_gen.join("a64_hooks.c"), "/* TAILCALL */\n").unwrap();
        fs::write(game_gen.join("a64_table.c"), "/* authored table */\n").unwrap();
        let answer = plan_json(
            exefs.to_str().unwrap(),
            romfs.to_str().unwrap(),
            base.join("out").to_str().unwrap(),
            work.to_str().unwrap(),
            0,
            true,
            false,
            false,
        )
        .unwrap();
        assert_eq!(answer["stage"], "compile");
        assert_eq!(
            fs::read_to_string(game_gen.join("code_000.c")).unwrap(),
            "/* authored lifted C */\n"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_lifted_chunk_that_does_not_compile_is_translated_again() {
        if !has_emitter() {
            return; // (no vendored translator here: the compile stage cannot run)
        }
        let base = base("failed-chunk");
        let (exefs, romfs) = dump(&base);
        let work = base.join("work");
        let game_gen = work.join("sa").join("gen");
        fs::create_dir_all(&game_gen).unwrap();
        fs::write(
            game_gen.join("code_000.c"),
            "#error \"authored lift failure\"\n",
        )
        .unwrap();
        fs::write(game_gen.join("a64_hooks.c"), "/* TAILCALL */\n").unwrap();
        fs::write(game_gen.join("a64_table.c"), "/* authored table */\n").unwrap();
        let answer = plan_json(
            exefs.to_str().unwrap(),
            romfs.to_str().unwrap(),
            base.join("out").to_str().unwrap(),
            work.to_str().unwrap(),
            0,
            false,
            false,
            true,
        )
        .unwrap();
        assert_eq!(answer["stage"], "compile");
        assert_eq!(answer["lift_kept"], false);
        assert!(!game_gen.join("code_000.c").exists());
        let again = plan_json(
            exefs.to_str().unwrap(),
            romfs.to_str().unwrap(),
            base.join("out").to_str().unwrap(),
            work.to_str().unwrap(),
            0,
            false,
            false,
            false,
        )
        .unwrap();
        assert_eq!(again["stage"], "lift");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn the_game_code_is_compiled_as_compile_units() {
        let base = base("units");
        let game_gen = base.join("gen");
        let units = base.join("units");
        let obj = base.join("obj");
        let cache = base.join("objcache");
        fs::create_dir_all(&game_gen).unwrap();
        fs::create_dir_all(&obj).unwrap();
        let mut chunk = String::from("#include \"a64.h\"\n");
        let padding = format!("    /* {} */\n", "x".repeat(900));
        for i in 0..400u32 {
            chunk.push_str(&format!(
                "void f_{:08x}(Ctx *__restrict A64_NOESC c) {{\n    c->x0 += {};\n{}}}\n",
                0x80000000 + i * 16,
                i,
                padding
            ));
        }
        fs::write(game_gen.join("code_000.c"), &chunk).unwrap();
        fs::write(game_gen.join("code_hot_0.c"), "/* split_hot's */\n").unwrap();
        let flags = ["-O1".to_owned()];
        let (jobs, count) = game_code_jobs(
            &game_gen,
            &units,
            &obj,
            &cache,
            "clang",
            &flags,
            Path::new(""),
            200_000,
        )
        .unwrap();
        assert!(
            count > 1,
            "{count} compile units of a chunk of 400 functions"
        );
        assert_eq!(
            jobs.len(),
            count + 1,
            "the hot chunk is no unit of the split"
        );
        // (one object each, named after its piece, and nothing collides)
        let objects: std::collections::HashSet<String> = jobs
            .iter()
            .map(|job| job["obj"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(objects.len(), jobs.len());
        // (the chunk itself is as the lift wrote it)
        assert_eq!(
            fs::read_to_string(game_gen.join("code_000.c")).unwrap(),
            chunk
        );
        let _ = fs::remove_dir_all(base);
    }

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
            false,
            false,
            false,
        )
        .unwrap();
        assert_eq!(answer["stage"], "lift");
        assert_eq!(answer["ready"], false);
        assert!(base.join("work/rt64/a64.h").is_file());
        assert!(base.join("work/build/gen/ui_fonts.cpp").is_file());
        let _ = fs::remove_dir_all(base);
    }
}
