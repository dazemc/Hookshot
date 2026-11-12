//! The build's read stage: check that a dump is the game and build Hookshot
//! supports, extracting its exefs first when the user chose a .nup.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::nso;
use crate::nup;
use crate::nup::{HEADS_MAGIC, HEADS_NAME};

/// A stamp is the .nup's name and size, written once the package was read.
fn read_stamp(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

fn write_stamp(path: &Path, stamp: &str) -> Result<(), String> {
    fs::write(path, stamp).map_err(|e| format!("{}: {e}", path.display()))
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| format!("{}: {e}", target.display()))?;
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry.map_err(|e| e.to_string())?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|e| e.to_string())?;
        let path = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            fs::copy(entry.path(), &path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn remove(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// Reject generated cache paths that are links or reparse points, or that
/// leave the output folder.
fn check_cache_paths(out: &Path, paths: &[&Path]) -> Result<(), String> {
    let root = fs::canonicalize(out).unwrap_or_else(|_| out.to_path_buf());
    for path in paths {
        let existing = fs::symlink_metadata(path).ok();
        if let Some(meta) = &existing {
            #[cfg(unix)]
            let linked = meta.file_type().is_symlink();
            #[cfg(windows)]
            let linked = {
                use std::os::windows::fs::MetadataExt;
                meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0
            };
            #[cfg(not(any(unix, windows)))]
            let linked = meta.file_type().is_symlink();
            if linked {
                return Err(format!(
                    "The generated cache path {} is not a normal path inside the build folder (symbolic links and reparse points are not supported). Choose a different output folder.",
                    path.display()
                ));
            }
        }
        let resolved = fs::canonicalize(path).or_else(|_| {
            path.parent()
                .map(|parent| {
                    fs::canonicalize(parent)
                        .unwrap_or_else(|_| parent.to_path_buf())
                        .join(path.file_name().unwrap_or_default())
                })
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        if let Ok(resolved) = resolved {
            if !resolved.starts_with(&root) {
                return Err(format!(
                    "The generated cache path {} is not a normal path inside the build folder (symbolic links and reparse points are not supported). Choose a different output folder.",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn keep_heads(out: &Path, game: &Path, stamp: &str) {
    let _ = fs::remove_file(out.join(".hookshot_romfs_heads"));
    let Ok(text) = fs::read_to_string(game.join(HEADS_NAME)) else {
        return;
    };
    let Some((first, rest)) = text.split_once('\n') else {
        return;
    };
    if first != HEADS_MAGIC {
        return;
    }
    let _ = fs::write(
        out.join(".hookshot_romfs_heads"),
        format!("{}\n{rest}", stamp.trim()),
    );
}

/// Replace a damaged dump's textures with whole ones of the same kind, once
/// per unpack. The note in the game folder is what the builder shows.
fn stand_in_damaged_files(out: &Path, romfs: &Path) -> Result<(Vec<String>, usize), String> {
    let good = out.join(".hookshot_romfs");
    let checked = out.join(".hookshot_romfs_checked");
    if !good.is_file() || !romfs.is_dir() {
        return Ok((Vec::new(), 0));
    }
    check_cache_paths(out, &[&good, &checked, romfs])?;
    let stamp = read_stamp(&good);
    let checked_stamp = format!("resolved-v2\n{stamp}"); // (v2: also files still in the .nup's zstd frame, #2361)
    if read_stamp(&checked) == checked_stamp {
        return Ok((Vec::new(), 0));
    }
    let _ = fs::remove_file(&checked);
    let heads = out.join(".hookshot_romfs_heads");
    let known = fs::read_to_string(&heads).ok().and_then(|text| {
        let (first, rest) = text.split_once('\n')?;
        if first == stamp.trim() {
            Some(
                rest.lines()
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect(),
            )
        } else {
            None
        }
    });
    let done = nup::stand_in_damaged(romfs, known)?;
    let note = out.join("hookshot_damaged_files.txt");
    let legacy = out.join("damaged_files.txt");
    if done.is_empty() {
        let _ = fs::remove_file(&note);
        let _ = fs::remove_file(&legacy);
        write_stamp(&checked, &checked_stamp)?;
        let _ = fs::remove_file(&heads);
        return Ok((Vec::new(), 0));
    }
    let damaged = done
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let unresolved = done.iter().filter(|(_, other)| other.is_none()).count();
    let mut text = format!(
        "Your game dump is incomplete: {} game file{} in it {} damaged (a complete dump is about 23.8 GB, an incomplete one 23.3 GB). Similar files are used where available, with different textures in those places. Dump the game again for the original files:\n",
        done.len(),
        if done.len() == 1 { "" } else { "s" },
        if done.len() == 1 { "is" } else { "are" }
    );
    for (path, other) in &done {
        text.push_str(&format!(
            "  {path}: {}\n",
            match other {
                Some(name) => format!("in its place {name}"),
                None => "no replacement found; build cannot continue".to_owned(),
            }
        ));
    }
    for name in [&note, &legacy] {
        fs::write(name, &text).map_err(|e| format!("{}: {e}", name.display()))?;
    }
    if unresolved > 0 {
        return Err(format!(
            "The game dump still has {unresolved} damaged file{} with no replacement. The build stopped before compiling. Dump the game again and select the complete dump in Hookshot Builder. The affected files are listed above.",
            if unresolved == 1 { "" } else { "s" }
        ));
    }
    write_stamp(&checked, &checked_stamp)?;
    let _ = fs::remove_file(&heads);
    Ok((damaged, unresolved))
}

/// An extracted dump holds only files and folders: a link in it would be
/// followed into the user's folder.
fn refuse_dump_links(folder: &Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(folder) {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.depth() == 0 {
            continue;
        }
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        #[cfg(unix)]
        let linked = meta.file_type().is_symlink();
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0
        };
        #[cfg(not(any(unix, windows)))]
        let linked = meta.file_type().is_symlink();
        if linked {
            return Err(format!(
                "The game files at {} contain a link ({}): copy them without links and choose that folder.",
                folder.display(),
                entry.path().display()
            ));
        }
    }
    Ok(())
}

/// The romfs by hard links where the disk allows it: 26 GB not written twice.
fn link_or_copy(source: &Path, target: &Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry.map_err(|e| e.to_string())?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|e| e.to_string())?;
        let path = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            continue;
        }
        if !entry.file_type().is_file() || path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        if fs::hard_link(entry.path(), &path).is_err() {
            fs::copy(entry.path(), &path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// The read stage for a folder dump: copy the game's code, rewrite ZBIC
/// modules as plain NSOs, and hard-link the romfs beside the output.
pub fn prepare_dump_json(exefs: &str, romfs: &str, out: &str, work: &str) -> Result<Value, String> {
    let dump_exefs = Path::new(exefs);
    let dump_romfs = Path::new(romfs);
    if !dump_exefs.is_dir() || !dump_romfs.is_dir() {
        return Err("The game folder must hold exefs/ and romfs/ folders.".into());
    }
    let out = Path::new(out);
    let work = if work.is_empty() {
        crate::kit::work()
    } else {
        PathBuf::from(work)
    };
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let work_exefs = work.join("exefs");
    let kept = out.join(".hookshot_exefs");
    let out_romfs = out.join("romfs");
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    check_cache_paths(out, &[&kept, &out_romfs])?;
    refuse_dump_links(dump_exefs)?;
    refuse_dump_links(dump_romfs)?;
    remove(&work_exefs);
    copy_tree(dump_exefs, &work_exefs)?;
    for name in nso::NAMES {
        let path = work_exefs.join(name);
        if path.is_file() {
            nso::unpack_nso(&path)?;
        }
    }
    nso::check_modules(&work_exefs)?;
    let build = nso::build_id(&work_exefs.join("main"))?;
    if !build.starts_with(nso::SUPPORTED_BUILD) {
        return Err(format!(
            "This is build {}. Hookshot supports build {}.",
            &build[..8.min(build.len())],
            nso::SUPPORTED_BUILD
        ));
    }
    remove(&out_romfs);
    link_or_copy(dump_romfs, &out_romfs)?;
    remove(&kept);
    copy_tree(&work_exefs, &kept)?;
    Ok(json!({
        "reused": false,
        "exefs": work_exefs.to_string_lossy(),
        "romfs": out_romfs.to_string_lossy(),
        "build": build,
        "modules": nso::check_modules(&work_exefs)?,
    }))
}

/// The message a missing .nup gets, shared by the reader and the streaming
/// command that starts it.
pub fn missing_package(path: &Path) -> String {
    format!(
        "Could not access your .nup file {}: it is not there. Check that the file is still there, reconnect its drive if needed, or choose the file again in Hookshot Builder.",
        path.display()
    )
}

/// The read stage for `--nup`: extract into the output folder once, keep its
/// exefs for updates, and reuse both when the same package is built again.
pub fn prepare_json(nup_path: &str, out: &str, work: &str) -> Result<Value, String> {
    prepare_json_with_progress(nup_path, out, work, &mut |_, _| {})
}

/// The same read, reporting how many package members of all of them are
/// written. A build that reuses the package's earlier extraction reports none.
pub fn prepare_json_with_progress(
    nup_path: &str,
    out: &str,
    work: &str,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Value, String> {
    let nup_path = Path::new(nup_path);
    let out = Path::new(out);
    let work = if work.is_empty() {
        crate::kit::work()
    } else {
        PathBuf::from(work)
    };
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let exefs = work.join("exefs");
    let game = out.join(".game");
    let good = out.join(".hookshot_romfs");
    let kept = out.join(".hookshot_exefs");
    let romfs = out.join("romfs");
    if !nup_path.is_file() {
        return Err(missing_package(nup_path));
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let size = fs::metadata(nup_path)
        .map_err(|e| format!("{}: {e}", nup_path.display()))?
        .len();
    let stamp = format!(
        "unpacked by this builder: {} {size}",
        nup_path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
    );
    check_cache_paths(out, &[&game, &good, &kept, &romfs])?;
    let reused = read_stamp(&good).trim() == stamp && romfs.is_dir() && kept.is_dir();
    if reused {
        remove(&exefs);
        copy_tree(&kept, &exefs)?;
    } else {
        remove(&game);
        let extraction = nup::extract_json_with_progress(
            nup_path.to_str().unwrap_or_default(),
            game.to_str().unwrap_or_default(),
            None,
            None,
            progress,
        );
        let extracted = match extraction {
            Ok(value) => value,
            Err(error) => {
                remove(&game);
                return Err(error);
            }
        };
        let staged_exefs = game.join("exefs");
        if let Err(error) = nso::check_modules(&staged_exefs)
            .and_then(|_| nso::build_id(&game.join("exefs/main")))
            .and_then(|build| {
                if build.starts_with(nso::SUPPORTED_BUILD) {
                    Ok(build)
                } else {
                    Err(format!(
                        "This is build {}. Hookshot supports build {}.",
                        &build[..8.min(build.len())],
                        nso::SUPPORTED_BUILD
                    ))
                }
            })
            .and_then(|_| {
                if game.join("romfs").is_dir() {
                    Ok(())
                } else {
                    Err("The package did not contain a romfs folder.".to_owned())
                }
            })
        {
            remove(&game);
            return Err(error);
        }
        remove(&romfs);
        fs::rename(game.join("romfs"), &romfs).map_err(|e| format!("{}: {e}", romfs.display()))?;
        keep_heads(out, &game, &stamp);
        remove(&exefs);
        fs::rename(game.join("exefs"), &exefs).map_err(|e| format!("{}: {e}", exefs.display()))?;
        remove(&game);
        remove(&kept);
        copy_tree(&exefs, &kept)?;
        write_stamp(&good, &format!("{stamp}\n"))?;
        let _ = extracted;
    }
    nso::check_modules(&exefs)?;
    let (damaged, unresolved) = stand_in_damaged_files(out, &romfs)?;
    Ok(json!({
        "reused": reused,
        "stamp": stamp,
        "exefs": exefs.to_string_lossy(),
        "romfs": romfs.to_string_lossy(),
        "damaged": damaged,
        "unresolved": unresolved,
    }))
}

/// The pipeline list recorded on the development PC, as the game folder's
/// list, so its first start builds them before the title. A list the folder
/// has from the same build is kept (it grows as the player plays).
pub fn seed_pipelines_json(root: &str, out: &str) -> Result<Value, String> {
    let seed = Path::new(root).join("hookshot_pipelines_seed.bin");
    let target = Path::new(out).join("hookshot_pipelines.bin");
    if !seed.is_file() {
        return Ok(json!({"copied": false, "reason": "no seed"}));
    }
    let data = fs::read(&seed).map_err(|e| format!("{}: {e}", seed.display()))?;
    if data.len() >= 8
        && fs::read(&target)
            .map(|old| old.len() >= 8 && old[..8] == data[..8])
            .unwrap_or(false)
    {
        return Ok(json!({"copied": false, "bytes": data.len()}));
    }
    fs::write(&target, &data).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(json!({"copied": true, "bytes": data.len()}))
}

/// `--check`: the answer make_game.py gives the builder before a build.
pub fn check_json(nup: Option<&str>, exefs: Option<&str>, work: &str) -> Result<Value, String> {
    let work = if work.is_empty() {
        crate::kit::work()
    } else {
        PathBuf::from(work)
    };
    let (folder, probe) = match (nup, exefs) {
        (Some(nup), _) if !nup.is_empty() => {
            let probe = work.join("probe");
            let _ = fs::remove_dir_all(&probe);
            nup::extract_json(nup, probe.to_str().unwrap_or_default(), Some("exefs"), None)?;
            (probe.join("exefs"), true)
        }
        (_, Some(exefs)) if !exefs.is_empty() => (Path::new(exefs).to_path_buf(), false),
        _ => return Err("needs a .nup file or an exefs folder".into()),
    };
    let answer = (|| {
        nso::check_modules(&folder)?;
        let build = nso::build_id(&folder.join("main"))?;
        let id = &build[..8.min(build.len())];
        let supported = build.starts_with(nso::SUPPORTED_BUILD);
        let message = if supported {
            format!("This is the version Hookshot supports (build {id}).")
        } else {
            format!(
                "This is build {id}. Hookshot supports build {}.",
                nso::SUPPORTED_BUILD
            )
        };
        Ok(json!({
            "ok": supported,
            "build": build,
            "build_id": id,
            "supported": supported,
            "message": message,
        }))
    })();
    if probe {
        let _ = fs::remove_dir_all(work.join("probe"));
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exefs_with(build: [u8; 8], tag: &str) -> std::path::PathBuf {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-check-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let mut main = vec![0u8; 0x60];
        main[..4].copy_from_slice(b"NSO0");
        for (i, byte) in build.iter().enumerate() {
            main[0x40 + i] = *byte;
        }
        fs::write(folder.join("main"), &main).unwrap();
        fs::write(folder.join("rtld"), b"NSO0").unwrap();
        folder
    }

    #[test]
    fn accepts_the_supported_build() {
        let folder = exefs_with([0xe0, 0xf3, 0xcc, 0xfa, 0, 0, 0, 0], "good");
        let answer = check_json(None, Some(folder.to_str().unwrap()), "/tmp").unwrap();
        assert_eq!(answer["ok"], true);
        assert_eq!(answer["build_id"], "e0f3ccfa");
        assert!(answer["message"].as_str().unwrap().contains("supports"));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn seeds_the_pipeline_list_once() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-seed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("root")).unwrap();
        fs::create_dir_all(base.join("out")).unwrap();
        fs::write(
            base.join("root/hookshot_pipelines_seed.bin"),
            b"STAMP123data",
        )
        .unwrap();
        let first = seed_pipelines_json(
            base.join("root").to_str().unwrap(),
            base.join("out").to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(first["copied"], true);
        assert_eq!(
            fs::read(base.join("out/hookshot_pipelines.bin")).unwrap(),
            b"STAMP123data"
        );
        fs::write(base.join("out/hookshot_pipelines.bin"), b"STAMP123grown").unwrap();
        let again = seed_pipelines_json(
            base.join("root").to_str().unwrap(),
            base.join("out").to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(again["copied"], false);
        assert_eq!(
            fs::read(base.join("out/hookshot_pipelines.bin")).unwrap(),
            b"STAMP123grown"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn an_older_damaged_texture_check_runs_once_more() {
        let base = std::env::temp_dir().join(format!(
            "hookshot-rust-damaged-stamp-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let out = base.join("out");
        let romfs = out.join("romfs");
        fs::create_dir_all(&romfs).unwrap();
        let unpacked = "unpacked by this builder: game.nup 1";
        fs::write(out.join(".hookshot_romfs"), unpacked).unwrap();
        fs::write(
            out.join(".hookshot_romfs_checked"),
            format!("resolved-v1\n{unpacked}"),
        )
        .unwrap();
        fs::write(romfs.join("a_Nrm.zs"), vec![0u8; 16]).unwrap();
        fs::write(
            romfs.join("b_Nrm.zs"),
            zstd::bulk::compress(b"BNTX wall", 3).unwrap(),
        )
        .unwrap();
        let (damaged, unresolved) = stand_in_damaged_files(&out, &romfs).unwrap();
        assert_eq!(damaged, vec!["a_Nrm.zs"]);
        assert_eq!(unresolved, 0);
        assert!(
            read_stamp(&out.join(".hookshot_romfs_checked")).starts_with("resolved-v2\n"),
            "an older check is not trusted"
        );
        // (the new stamp is trusted: nothing is read again)
        assert_eq!(
            stand_in_damaged_files(&out, &romfs).unwrap(),
            (Vec::new(), 0)
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn prepares_a_folder_dump() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-dump-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let exefs = base.join("dump/exefs");
        let romfs = base.join("dump/romfs");
        fs::create_dir_all(&exefs).unwrap();
        fs::create_dir_all(romfs.join("data")).unwrap();
        let mut main = vec![0u8; 0x60];
        main[..4].copy_from_slice(b"NSO0");
        main[0x40..0x48].copy_from_slice(&[0xe0, 0xf3, 0xcc, 0xfa, 0, 0, 0, 0]);
        fs::write(exefs.join("main"), &main).unwrap();
        fs::write(exefs.join("rtld"), b"NSO0").unwrap();
        fs::write(romfs.join("data/file.bin"), b"game data").unwrap();
        let out = base.join("out");
        let work = base.join("work");
        let answer = prepare_dump_json(
            exefs.to_str().unwrap(),
            romfs.to_str().unwrap(),
            out.to_str().unwrap(),
            work.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(answer["modules"], serde_json::json!(["main", "rtld"]));
        assert_eq!(
            fs::read(out.join("romfs/data/file.bin")).unwrap(),
            b"game data"
        );
        assert!(work.join("exefs/main").is_file());
        assert!(out.join(".hookshot_exefs/main").is_file());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn names_another_build() {
        let folder = exefs_with([1, 2, 3, 4, 0, 0, 0, 0], "other");
        let answer = check_json(None, Some(folder.to_str().unwrap()), "/tmp").unwrap();
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["build_id"], "01020304");
        assert!(answer["message"].as_str().unwrap().contains("e0f3ccfa"));
        let _ = fs::remove_dir_all(folder);
    }
}
