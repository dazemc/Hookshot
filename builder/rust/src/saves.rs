//! The game's saves beside the emulators and ports that keep the same files:
//! find them in a folder, and copy one into a Hookshot profile, backing up
//! the side it overwrites first. Ported from scripts/oot_saves.py, which keeps
//! the command line; the builder's launcher uses these functions.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local};
use serde_json::{Value, json};

pub const TITLE: &str = "0400486024DEC000";
const NO_ACCOUNT: &str = "0000000000000000";

/// One part of a save folder's path under a data folder. `<id>` is one folder
/// per user profile; its name is the id shown.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Fixed(&'static str),
    Id,
}

const EDEN_LAYOUT: &[Part] = &[
    Part::Fixed("nand"),
    Part::Fixed("user"),
    Part::Fixed("save"),
    Part::Fixed(NO_ACCOUNT),
    Part::Id,
    Part::Fixed(TITLE),
];
// Eden's newer layout: used first when it exists.
const ACCOUNT_LAYOUT: &[Part] = &[
    Part::Fixed("nand"),
    Part::Fixed("user"),
    Part::Fixed("save"),
    Part::Fixed("account"),
    Part::Id,
    Part::Fixed(TITLE),
    Part::Fixed("0"),
];
const NX2NAVI_LAYOUT: &[Part] = &[Part::Fixed("profile"), Part::Fixed("save"), Part::Id];
const HOOKSHOT_GAME_LAYOUT: &[Part] = &[Part::Fixed("user"), Part::Fixed("save"), Part::Id];
const HOOKSHOT_DATA_LAYOUT: &[Part] = &[Part::Fixed("save"), Part::Id];

// The first layout that finds a profile wins.
const LAYOUTS: &[&[Part]] = &[
    ACCOUNT_LAYOUT,
    EDEN_LAYOUT,
    NX2NAVI_LAYOUT,
    HOOKSHOT_GAME_LAYOUT,
    HOOKSHOT_DATA_LAYOUT,
];

struct Program {
    name: &'static str,
    display: &'static str,
    linux: &'static [&'static str],
    windows: &'static [&'static str],
    processes: &'static [&'static str],
}

const EDEN_LINUX: &[&str] = &["{data}/eden"];
const EDEN_WINDOWS: &[&str] = &["{appdata}/eden"];
const LICHII_LINUX: &[&str] = &["{data}/lichii", "{data}/eden"];
const LICHII_WINDOWS: &[&str] = &["{appdata}/lichii", "{appdata}/eden"];

// Everything that runs the game and where it keeps its data, one entry each.
const EDEN: Program = Program {
    name: "eden",
    display: "Eden",
    linux: EDEN_LINUX,
    windows: EDEN_WINDOWS,
    processes: &["eden"],
};
const EMULATORS: &[Program] = &[
    Program {
        name: "lichii",
        display: "Lichii",
        linux: LICHII_LINUX,
        windows: LICHII_WINDOWS,
        processes: &["lichii"],
    },
    Program {
        name: "frizti",
        display: "Frizti (Fritchii)",
        linux: EDEN_LINUX,
        windows: EDEN_WINDOWS,
        processes: &["lichii", "fritchii"],
    },
    Program {
        name: "orchid",
        display: "Orchid",
        linux: EDEN_LINUX,
        windows: EDEN_WINDOWS,
        processes: &["orchid"],
    },
    Program {
        name: "pokedinner",
        display: "PokeDinner",
        linux: EDEN_LINUX,
        windows: EDEN_WINDOWS,
        processes: &["eglantine"],
    },
    EDEN,
    Program {
        name: "eglantine",
        display: "Eglantine",
        linux: &["{data}/eglantine"],
        windows: &["{appdata}/eglantine"],
        processes: &["eglantine"],
    },
    Program {
        name: "nx2navi",
        display: "NX2navi",
        linux: &["{home}/Games/Ocarina"],
        windows: &["{localappdata}/Programs/NX2navi/game", "C:/Games/Ocarina"],
        processes: &["nx2navi", "ocarina"],
    },
];
const HOOKSHOT: Program = Program {
    name: "hookshot",
    display: "Hookshot",
    linux: &["{data}/hookshot"],
    windows: &[],
    processes: &["hookshot"],
};

/// The import the launcher asks for: `from` is the emulator's side (a save
/// folder or a folder above it), `hookshot` the profile's save folder.
pub struct Import {
    pub from: String,
    pub hookshot: Option<String>,
    pub profile: Option<String>,
    pub backup_dir: Option<String>,
    pub force: bool,
    pub keep_settings: bool,
    pub dry_run: bool,
}

pub fn list_json(folder: Option<&str>) -> Result<Value, String> {
    let saves = match folder.filter(|folder| !folder.is_empty()) {
        Some(folder) => saves_in(folder)?,
        None => emulator_saves(),
    };
    Ok(json!({"saves": saves}))
}

pub fn import_json(request: &Value) -> Result<Value, String> {
    let job = Import {
        from: string(request, "from").unwrap_or_default(),
        hookshot: string(request, "hookshot").filter(|path| !path.is_empty()),
        profile: string(request, "profile").filter(|id| !id.is_empty()),
        backup_dir: string(request, "backup_dir").filter(|path| !path.is_empty()),
        force: request
            .get("force")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        keep_settings: request
            .get("keep_settings")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dry_run: request
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    import_job(&job)
}

fn string(request: &Value, key: &str) -> Option<String> {
    request.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn import_job(job: &Import) -> Result<Value, String> {
    let running = if job.force || job.dry_run {
        Vec::new()
    } else {
        running_games()
    };
    import_with(job, &running)
}

fn import_with(job: &Import, running: &[String]) -> Result<Value, String> {
    if job.from.is_empty() {
        return Err("import what? Pass the folder to import from (--from <folder>).".into());
    }
    let (_hookshot_root, hookshot) = hookshot_saves(job.hookshot.as_deref())?;
    let (_source_root, source) = locate_folder(&job.from, "--from")?;
    let (_hookshot_id, target) = pick(
        &hookshot,
        "Hookshot",
        "--hookshot <the user id folder>",
        None,
    )?;
    let (_source_id, from) = pick(
        &source,
        "the folder",
        "--profile <id>",
        job.profile.as_deref(),
    )?;
    if comparison(&from) == comparison(&target) {
        return Err(format!(
            "{} is both the source and the target",
            from.display()
        ));
    }
    if !job.force {
        if let Some(pending) = pending_commit(&from) {
            return Err(format!(
                "NX2navi still has a save waiting in {}, which it loads instead of {}: start NX2navi and quit it from the game, then copy (or pass --force)",
                pending.display(),
                from.display()
            ));
        }
    }
    if !job.dry_run && !job.force && !running.is_empty() {
        return Err(format!(
            "close {} first (or pass --force): a running game can write its save again and undo the copy",
            running.join(" and ")
        ));
    }
    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let base = job
        .backup_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(default_backup_dir)
        .join(format!("import-folder-{stamp}"));
    let mut backup = base.clone();
    let mut n = 1;
    while backup.symlink_metadata().is_ok() {
        n += 1;
        backup = PathBuf::from(format!("{}-{n}", base.display()));
    }
    let text = copy_saves(&from, &target, &backup, job.keep_settings, job.dry_run)?;
    Ok(json!({
        "text": text,
        "path": target.to_string_lossy(),
        "backup": backup.to_string_lossy(),
    }))
}

fn saves_in(folder: &str) -> Result<Vec<Value>, String> {
    let (root, found) = locate_folder(folder, "--from")?;
    Ok(found
        .iter()
        .map(|(id, path)| describe_save(id, path, &root, None))
        .collect())
}

/// The saves in every emulator's usual folder: the one import and export
/// would use (the first with saves), the programs sharing it named together.
fn emulator_saves() -> Vec<Value> {
    let mut groups: Vec<(PathBuf, Vec<&'static str>, BTreeMap<String, PathBuf>)> = Vec::new();
    for program in EMULATORS {
        for folder in folders(program) {
            if !folder.is_dir() {
                continue;
            }
            let found = find_saves(&folder);
            if found.is_empty() {
                continue;
            }
            match groups
                .iter()
                .position(|(other, _, _)| comparison(other) == comparison(&folder))
            {
                Some(at) => groups[at].1.push(program.display),
                None => groups.push((folder, vec![program.display], found)),
            }
            break;
        }
    }
    let mut out = Vec::new();
    for (folder, displays, found) in groups {
        let emulator = displays.join(" / ");
        for (id, path) in &found {
            out.push(describe_save(id, path, &folder, Some(&emulator)));
        }
    }
    out
}

/// Eden's user list (and yuzu's, which it comes from), under its nand folder:
/// 0x10 bytes, then 8 users of 0xC8 bytes each. A save's folder is named by
/// the id's second word, then its first, so the names are found by that name.
fn user_names(nand: &Path) -> BTreeMap<String, String> {
    let list = Path::new("system")
        .join("save")
        .join("8000000000000010")
        .join("su")
        .join("avators")
        .join("profiles.dat");
    let bytes = fs::read(nand.join(list)).unwrap_or_default();
    let data = &bytes[..bytes.len().min(0x650)];
    let mut names = BTreeMap::new();
    let mut at = 0x10;
    while at + 0xC7 < data.len() {
        let lo = u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
        let hi = u64::from_le_bytes(data[at + 8..at + 16].try_into().unwrap());
        let name = data[at + 40..at + 72]
            .split(|byte| *byte == 0)
            .next()
            .unwrap_or_default();
        let name = String::from_utf8_lossy(name).trim().to_owned();
        if (lo != 0 || hi != 0) && !name.is_empty() {
            names.insert(format!("{hi:016X}{lo:016X}"), name);
        }
        at += 0xC8;
    }
    names
}

/// The name the emulator gives the user whose save is in path (an
/// Eden-style nand folder above it), or None.
fn user_name(path: &Path) -> Option<String> {
    let norm = absolute(path);
    let mut up = norm.clone();
    loop {
        if basename(&up).eq_ignore_ascii_case("nand") {
            break;
        }
        match up.parent() {
            Some(parent) if parent != up => up = parent.to_path_buf(),
            _ => return None,
        }
    }
    let relative = norm.strip_prefix(&up).ok()?;
    let ids: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .filter(|name| is_id(name))
        .map(|name| name.to_ascii_uppercase())
        .collect();
    let names = user_names(&up);
    ids.into_iter().find_map(|id| names.get(&id).cloned())
}

fn describe_save(id: &str, path: &Path, folder: &Path, emulator: Option<&str>) -> Value {
    let names = slots(path);
    let times: Vec<SystemTime> = names
        .iter()
        .filter_map(|slot| {
            fs::metadata(path.join(slot).join("progress.sav"))
                .ok()
                .and_then(|meta| meta.modified().ok())
        })
        .collect();
    let when = times
        .iter()
        .max()
        .map(|time| {
            format!(
                ", last saved {}",
                DateTime::<Local>::from(*time).format("%Y-%m-%d %H:%M")
            )
        })
        .unwrap_or_default();
    let user = user_name(path);
    let who = match &user {
        Some(user) => format!("{user} ({}…)", &id[..id.len().min(8)]),
        None => id.to_owned(),
    };
    let count = names.len();
    json!({
        "id": id,
        "path": path.to_string_lossy(),
        "folder": folder.to_string_lossy(),
        "user": user,
        "emulator": emulator,
        "label": format!(
            "{who}: {count} slot{}{when}{}",
            if count == 1 { "" } else { "s" },
            emulator.map(|name| format!(" · {name}")).unwrap_or_default()
        ),
    })
}

fn hookshot_saves(hookshot: Option<&str>) -> Result<(PathBuf, BTreeMap<String, PathBuf>), String> {
    if let Some(path) = hookshot.filter(|path| !path.is_empty()) {
        if let Some(found) = new_profile(Path::new(path)) {
            return Ok((PathBuf::from(path), found));
        }
    }
    locate_program(&HOOKSHOT, hookshot, "--hookshot")
}

/// `{id: path}` when path is one profile's save folder of Hookshot's layout
/// (save/<32 hex digits>) holding no save yet, which may not be there either:
/// a profile the launcher added. An import fills it.
fn new_profile(path: &Path) -> Option<BTreeMap<String, PathBuf>> {
    let norm = absolute(path);
    let name = basename(&norm);
    if !basename(norm.parent()?).eq_ignore_ascii_case("save") || !is_id(&name) {
        return None;
    }
    if norm.is_dir() && !find_saves(&norm).is_empty() {
        return None;
    }
    let mut found = BTreeMap::new();
    found.insert(name, norm);
    Some(found)
}

fn locate_folder(path: &str, _flag: &str) -> Result<(PathBuf, BTreeMap<String, PathBuf>), String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(format!("no folder at {path}"));
    }
    let found = find_saves(&root);
    if found.is_empty() {
        return Err(format!(
            "no Ocarina of Time save under {path} (a folder with account.sav and slot_*, or {TITLE} under an emulator's nand\\user\\save): start the game there once and save"
        ));
    }
    Ok((root, found))
}

fn locate_program(
    program: &Program,
    override_path: Option<&str>,
    flag: &str,
) -> Result<(PathBuf, BTreeMap<String, PathBuf>), String> {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return locate_folder(path, flag);
    }
    let tried = folders(program);
    for folder in &tried {
        if !folder.is_dir() {
            continue;
        }
        let found = find_saves(folder);
        if !found.is_empty() {
            return Ok((folder.clone(), found));
        }
    }
    if program.name == HOOKSHOT.name && tried.is_empty() {
        return Err("Hookshot's saves are in the user folder of your game folder on Windows: pass the game folder with --hookshot (for example --hookshot C:\\Games\\Hookshot)".into());
    }
    let looked = if tried.is_empty() {
        "no default folder".to_owned()
    } else {
        tried
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(format!(
        "no {} save found (looked in {looked}): start the game in {} once and save, or pass {flag} <folder>",
        program.display, program.display
    ))
}

fn pick(
    found: &BTreeMap<String, PathBuf>,
    name: &str,
    flag: &str,
    wanted: Option<&str>,
) -> Result<(String, PathBuf), String> {
    if let Some(wanted) = wanted.filter(|id| !id.is_empty()) {
        for (id, path) in found {
            if id.eq_ignore_ascii_case(wanted) {
                return Ok((id.clone(), path.clone()));
            }
        }
        return Err(format!(
            "{name} has no profile {wanted}; it has: {}",
            found.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if found.len() > 1 {
        let lines: String = found
            .iter()
            .map(|(id, path)| format!("\n  {id}  {}", path.display()))
            .collect();
        return Err(format!(
            "{name} has Ocarina of Time saves in {} profiles, so pick one with {flag}:{lines}",
            found.len()
        ));
    }
    found
        .iter()
        .next()
        .map(|(id, path)| (id.clone(), path.clone()))
        .ok_or_else(|| format!("{name} has no Ocarina of Time save: nothing to copy"))
}

/// NX2navi writes a save to profile/save-tx first and moves it into
/// profile/save/<account> after: while something waits there, that is the
/// save the game loads next, not the account folder.
fn pending_commit(save: &Path) -> Option<PathBuf> {
    let save = absolute(save);
    let parent = save.parent()?;
    if !basename(parent).eq_ignore_ascii_case("save") {
        return None;
    }
    let tx = parent.parent()?.join("save-tx");
    if !tx.is_dir() {
        return None;
    }
    for entry in walkdir::WalkDir::new(&tx) {
        if let Ok(entry) = entry {
            if entry.file_type().is_file() {
                return Some(tx);
            }
        }
    }
    None
}

fn copy_saves(
    src: &Path,
    dst: &Path,
    backup: &Path,
    keep_settings: bool,
    dry_run: bool,
) -> Result<String, String> {
    if inside(backup, dst) {
        return Err(format!(
            "choose a --backup-dir outside the target save folder ({})",
            dst.display()
        ));
    }
    let names = slots(src);
    if names.is_empty() {
        return Err(format!(
            "{} has no slot_* folders: nothing to copy",
            src.display()
        ));
    }
    for slot in &names {
        refuse_links(&src.join(slot))?;
    }
    for file in ["account.sav", "option.sav"] {
        let path = src.join(file);
        if path.symlink_metadata().is_ok() {
            refuse_links(&path)?;
        }
    }
    let empty = empty_files(src);
    if let Some(first) = empty.first() {
        return Err(format!(
            "{} is empty: the save is not whole, so nothing was copied",
            first.display()
        ));
    }
    let mut files: Vec<&str> = ["account.sav", "option.sav"]
        .into_iter()
        .filter(|file| src.join(file).is_file())
        .collect();
    if keep_settings {
        files.retain(|file| *file != "option.sav");
    }
    let had = dst.is_dir();
    let mut out = String::new();
    if had {
        out.push_str(&format!(
            "{} {} -> {}\n",
            if dry_run {
                "Would back up"
            } else {
                "Backing up"
            },
            dst.display(),
            backup.display()
        ));
    } else {
        out.push_str(&format!(
            "{} holds no save yet: nothing to back up\n",
            dst.display()
        ));
    }
    out.push_str(&format!(
        "{} {} slot(s) ({}) and {} from {} to {}\n",
        if dry_run { "Would copy" } else { "Copying" },
        names.len(),
        names.join(", "),
        if files.is_empty() {
            "no loose files".to_owned()
        } else {
            files.join(", ")
        },
        src.display(),
        dst.display()
    ));
    if keep_settings {
        out.push_str("option.sav kept as it is on the target\n");
    }
    if dry_run {
        out.push_str("Dry run: nothing was changed\n");
        return Ok(out);
    }
    if had {
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(err)?;
        }
        copy_dir(dst, backup)?;
    } else {
        fs::create_dir_all(dst).map_err(err)?;
    }
    let copied = (|| -> Result<(), String> {
        for slot in slots(dst) {
            fs::remove_dir_all(dst.join(slot)).map_err(err)?;
        }
        for slot in &names {
            copy_dir(&src.join(slot), &dst.join(slot))?;
        }
        for file in &files {
            fs::copy(src.join(file), dst.join(file)).map_err(err)?;
        }
        Ok(())
    })();
    if let Err(error) = copied {
        restore(dst, if had { Some(backup) } else { None });
        return Err(format!(
            "the copy failed ({error}): {} is as it was before",
            dst.display()
        ));
    }
    if had {
        out.push_str(&format!(
            "Done. Backup of the old {} in {}\n",
            dst.display(),
            backup.display()
        ));
    } else {
        out.push_str("Done.\n");
    }
    Ok(out)
}

/// Put the target back as the backup has it (None: as it was before, when
/// there was no save: no folder, which the copy made).
fn restore(dst: &Path, backup: Option<&Path>) {
    for slot in slots(dst) {
        let _ = fs::remove_dir_all(dst.join(slot));
    }
    if let Some(backup) = backup {
        for slot in slots(backup) {
            let _ = copy_dir(&backup.join(&slot), &dst.join(&slot));
        }
    }
    for file in ["account.sav", "option.sav"] {
        let from = backup.map(|backup| backup.join(file));
        match from {
            Some(from) if from.is_file() => {
                let _ = fs::copy(&from, dst.join(file));
            }
            _ => {
                let _ = fs::remove_file(dst.join(file));
            }
        }
    }
    if backup.is_none() {
        let _ = fs::remove_dir(dst);
    }
}

/// A save from someone else is only files and folders: a link in it would
/// copy what it points to (any file or folder of this PC) into the save.
fn refuse_links(path: &Path) -> Result<(), String> {
    if is_link(path) {
        return Err(link_error(path));
    }
    for entry in walkdir::WalkDir::new(path) {
        let Ok(entry) = entry else {
            continue;
        };
        if is_link(entry.path()) {
            return Err(link_error(entry.path()));
        }
    }
    Ok(())
}

fn link_error(path: &Path) -> String {
    format!(
        "{} is a link: a save holds only files and folders, so nothing was copied",
        path.display()
    )
}

/// A Windows junction is a reparse point, not a symbolic link, and
/// copytree walks into one as into a folder.
pub(crate) fn is_link(path: &Path) -> bool {
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// The save's files of no bytes: a save cut short (a copy that failed, a full
/// disk) that would replace a good one.
fn empty_files(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for file in ["account.sav", "option.sav"] {
        let path = path.join(file);
        if path.is_file()
            && fs::metadata(&path)
                .map(|meta| meta.len() == 0)
                .unwrap_or(false)
        {
            out.push(path);
        }
    }
    for slot in slots(path) {
        for entry in walkdir::WalkDir::new(path.join(slot)) {
            let Ok(entry) = entry else {
                continue;
            };
            if entry.file_type().is_file()
                && entry
                    .metadata()
                    .map(|meta| meta.len() == 0)
                    .unwrap_or(false)
            {
                out.push(entry.path().to_path_buf());
            }
        }
    }
    out
}

fn copy_dir(source: &Path, target: &Path) -> Result<(), String> {
    // (a copy into a folder below its source reads the folder it just made and
    // recurses forever: refuse it even if a caller's own check missed it)
    if inside(target, source) {
        return Err(format!(
            "{} is inside {}: a copy would include itself",
            target.display(),
            source.display()
        ));
    }
    fs::create_dir_all(target).map_err(err)?;
    for entry in fs::read_dir(source).map_err(err)? {
        let entry = entry.map_err(err)?;
        let kind = entry.file_type().map_err(err)?;
        let to = target.join(entry.file_name());
        if kind.is_symlink() {
            if fs::metadata(entry.path()).map_err(err)?.is_dir() {
                copy_dir(&entry.path(), &to)?;
            } else {
                fs::copy(entry.path(), &to).map_err(err)?;
            }
        } else if kind.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &to).map_err(err)?;
        }
    }
    Ok(())
}

/// The game's save folders under root, as {profile id: path}; root may be the
/// save folder itself or any folder above it.
fn find_saves(root: &Path) -> BTreeMap<String, PathBuf> {
    let norm = absolute(root);
    let title_folder = basename(&norm).eq_ignore_ascii_case(TITLE);
    let mut found = BTreeMap::new();
    if is_save_folder(&norm) || title_folder && !subdirs(&norm).iter().any(|name| name == "0") {
        let mut key = norm.clone();
        loop {
            let name = basename(&key);
            let Some(parent) = key.parent().filter(|parent| *parent != key) else {
                break;
            };
            if !(name.eq_ignore_ascii_case(TITLE) || name == "0") {
                break;
            }
            key = parent.to_path_buf();
        }
        let name = basename(&key);
        found.insert(
            if name.is_empty() {
                norm.to_string_lossy().into_owned()
            } else {
                name
            },
            norm,
        );
        return found;
    }
    for layout in LAYOUTS {
        let idpos = layout
            .iter()
            .position(|part| matches!(part, Part::Id))
            .unwrap_or(0);
        let has_title = layout
            .iter()
            .any(|part| matches!(part, Part::Fixed(name) if *name == TITLE));
        let named_before = match layout.get(idpos.wrapping_sub(1)) {
            Some(Part::Fixed(name)) => *name,
            _ => "",
        };
        for start in 0..layout.len() {
            let below_has_title = layout[start..]
                .iter()
                .any(|part| matches!(part, Part::Fixed(name) if *name == TITLE));
            // (below the title id folder, only the title id folder itself is
            // a place to start)
            if has_title && !below_has_title && !title_folder {
                continue;
            }
            // (from a folder of users that is not named as the emulator names
            // it (copied user folders, say), only a folder named as a user is,
            // 32 hex digits, is one: any folder with a title id folder in it is
            // not, Eden's mods folder load/<title id> among them)
            let any_user = !(has_title
                && start == idpos
                && !basename(&norm).eq_ignore_ascii_case(named_before));
            let key = if start > idpos {
                // (root is at or below the <id> folder: the id is a folder above it)
                let mut up = norm.clone();
                for _ in 0..(start - idpos - 1) {
                    match up.parent() {
                        Some(parent) if parent != up => up = parent.to_path_buf(),
                        _ => break,
                    }
                }
                Some(basename(&up))
            } else {
                None
            };
            let mut rows = Vec::new();
            walk(&norm, &layout[start..], key.as_deref(), &mut rows);
            let last_is_id = matches!(layout.last(), Some(&Part::Id));
            for (id, path) in rows {
                // A folder named by the layout (the title id folder) counts as
                // soon as it is there: the game may not have saved yet. A
                // profile folder counts only when it holds a save (Eden's
                // profile folders do not).
                if !any_user && !is_id(&id) {
                    continue;
                }
                if (!last_is_id || is_save_folder(&path))
                    && !found.contains_key(&id)
                    && !found.values().any(|known| *known == path)
                {
                    found.insert(id, path);
                }
            }
        }
    }
    found
}

fn walk(root: &Path, parts: &[Part], key: Option<&str>, out: &mut Vec<(String, PathBuf)>) {
    match parts.split_first() {
        None => out.push((key.unwrap_or_default().to_owned(), root.to_path_buf())),
        Some((Part::Id, rest)) => {
            for name in subdirs(root) {
                walk(&root.join(&name), rest, Some(&name), out);
            }
        }
        Some((Part::Fixed(fixed), rest)) => {
            for name in subdirs(root) {
                if name.eq_ignore_ascii_case(fixed) {
                    walk(&root.join(&name), rest, key, out);
                }
            }
        }
    }
}

fn is_save_folder(path: &Path) -> bool {
    path.join("account.sav").is_file() || !slots(path).is_empty()
}

fn subdirs(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn slots(path: &Path) -> Vec<String> {
    subdirs(path)
        .into_iter()
        .filter(|name| name.starts_with("slot_"))
        .collect()
}

fn is_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn absolute(path: &Path) -> PathBuf {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for part in full.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `fs::canonicalize` for a path that may not be there yet: the longest
/// existing prefix is canonicalized and the missing parts are appended.
///
/// Both sides of a comparison must have the same form. Windows returns a
/// `\\?\C:\...` path from `fs::canonicalize` but a plain `C:\...` path from
/// `absolute`, and mixing them made `inside` miss a backup folder below a
/// target that existed (`C:\...` is not a prefix of `\\?\C:\...`), after which
/// `copy_dir` copied the target into its own backup until the stack ran out.
fn resolved(path: &Path) -> PathBuf {
    let absolute = absolute(path);
    let mut existing = absolute.as_path();
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(real) = fs::canonicalize(existing) {
            let mut out = real;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return out;
        }
        let Some(parent) = existing.parent() else {
            return absolute;
        };
        if parent == existing {
            return absolute;
        }
        if let Some(name) = existing.file_name() {
            missing.push(name.to_os_string());
        }
        existing = parent;
    }
}

fn comparison(path: &Path) -> String {
    let text = resolved(path).to_string_lossy().into_owned();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

pub(crate) fn inside(path: &Path, folder: &Path) -> bool {
    let path = comparison(path);
    let folder = comparison(folder);
    Path::new(&path).starts_with(Path::new(&folder))
}

fn home() -> PathBuf {
    // (Python's expanduser uses USERPROFILE on Windows, HOME elsewhere)
    let names: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME", "USERPROFILE"]
    };
    if let Some(path) = names.iter().find_map(|name| std::env::var_os(name)) {
        return PathBuf::from(path);
    }
    if cfg!(windows) {
        if let (Some(drive), Some(path)) =
            (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
        {
            return PathBuf::from(drive).join(path);
        }
    }
    PathBuf::from("~")
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local").join("share"))
}

fn default_backup_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData").join("Local"));
        base.join("Hookshot").join("oot-save-backups")
    } else {
        data_home().join("oot-save-backups")
    }
}

/// program's data folders on this OS, in the order they are tried.
fn folders(program: &Program) -> Vec<PathBuf> {
    let home = home().to_string_lossy().into_owned();
    let data = data_home().to_string_lossy().into_owned();
    let appdata = std::env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join("AppData").join("Roaming"))
        .to_string_lossy()
        .into_owned();
    let localappdata = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join("AppData").join("Local"))
        .to_string_lossy()
        .into_owned();
    folders_on(
        program,
        cfg!(windows),
        &home,
        &data,
        &appdata,
        &localappdata,
    )
}

fn folders_on(
    program: &Program,
    windows: bool,
    home: &str,
    data: &str,
    appdata: &str,
    localappdata: &str,
) -> Vec<PathBuf> {
    let templates = if windows {
        program.windows
    } else {
        program.linux
    };
    templates
        .iter()
        .map(|template| {
            PathBuf::from(
                template
                    .replace("{data}", data)
                    .replace("{home}", home)
                    .replace("{appdata}", appdata)
                    .replace("{localappdata}", localappdata),
            )
        })
        .collect()
}

/// {process name: what it belongs to} for every program in the tables.
fn process_names() -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    for program in std::iter::once(&HOOKSHOT).chain(EMULATORS.iter()) {
        for process in program.processes {
            match names.get(*process) {
                Some(existing) => {
                    let joined = format!("{existing} or {}", program.display);
                    names.insert((*process).to_owned(), joined);
                }
                None => {
                    names.insert((*process).to_owned(), program.display.to_owned());
                }
            }
        }
    }
    names
}

/// What is running among Hookshot and the emulators in EMULATORS.
fn running_games() -> Vec<String> {
    #[cfg(windows)]
    {
        windows_games()
    }
    #[cfg(not(windows))]
    {
        linux_games()
    }
}

#[cfg(windows)]
fn windows_games() -> Vec<String> {
    let Ok(output) = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    else {
        return Vec::new();
    };
    let known = process_names();
    let text = String::from_utf8_lossy(&output.stdout);
    let mut names = Vec::new();
    for line in text.lines() {
        let image = line
            .split("\",\"")
            .next()
            .unwrap_or("")
            .trim_matches('"')
            .to_lowercase();
        if let Some(process) = image.strip_suffix(".exe") {
            if let Some(display) = known.get(process) {
                names.push(display.clone());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

#[cfg(not(windows))]
fn linux_games() -> Vec<String> {
    use regex::Regex;

    let known = process_names();
    let mut names = Vec::new();
    // (the AppImage this runs from, when the Hookshot launcher in it runs this
    // code: its runtime is a process named after it, the launcher's own, not a
    // game)
    let own = std::env::var("APPIMAGE")
        .ok()
        .and_then(|path| {
            Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
        })
        .unwrap_or_default();
    let pattern = format!(
        "^({})[-_.].*\\.appimage$",
        known
            .keys()
            .map(|name| regex::escape(name))
            .collect::<Vec<_>>()
            .join("|")
    );
    let appimage = Regex::new(&pattern).ok();
    let Ok(entries) = fs::read_dir("/proc") else {
        return names;
    };
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().into_owned();
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let argv0 = fs::read(entry.path().join("cmdline"))
            .ok()
            .and_then(|bytes| {
                bytes
                    .split(|byte| *byte == 0)
                    .next()
                    .map(|first| String::from_utf8_lossy(first).into_owned())
            })
            .unwrap_or_default();
        let comm = fs::read_to_string(entry.path().join("comm"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let argv0 = Path::new(&argv0)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !own.is_empty() && argv0.to_lowercase() == own {
            continue;
        }
        for candidate in [comm.to_lowercase(), argv0.to_lowercase()] {
            if let Some(display) = known.get(&candidate) {
                names.push(display.clone());
            } else if let Some(display) = appimage
                .as_ref()
                .and_then(|pattern| pattern.captures(&candidate))
                .and_then(|captures| captures.get(1))
                .and_then(|name| known.get(name.as_str()))
            {
                names.push(display.clone());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn err(error: std::io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "FEDCBA9876543210FEDCBA9876543210";
    const OTHER: &str = "0123456789ABCDEF0123456789ABCDEF";
    const UID: &str = "0123456789abcdef0123456789abcdef";
    const ACCOUNT: &str = "00112233445566778899aabbccddeeff";

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hookshot-rust-saves-{name}-{}-{}",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn make_save(folder: &Path, tag: &str, slots: &[&str]) {
        fs::create_dir_all(folder).unwrap();
        fs::write(folder.join("account.sav"), format!("account-{tag}")).unwrap();
        fs::write(folder.join("option.sav"), format!("option-{tag}")).unwrap();
        for slot in slots {
            fs::create_dir_all(folder.join(slot)).unwrap();
            fs::write(
                folder.join(slot).join("progress.sav"),
                format!("progress-{tag}-{slot}"),
            )
            .unwrap();
            fs::write(
                folder.join(slot).join("caption.sav"),
                format!("caption-{tag}-{slot}"),
            )
            .unwrap();
        }
    }

    fn eden_save(data: &Path, profile: &str) -> PathBuf {
        data.join("nand")
            .join("user")
            .join("save")
            .join(NO_ACCOUNT)
            .join(profile)
            .join(TITLE)
    }

    fn tree(folder: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        for entry in walkdir::WalkDir::new(folder).sort_by_file_name() {
            let entry = entry.unwrap();
            if entry.file_type().is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(folder)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(relative, fs::read(entry.path()).unwrap());
            }
        }
        out
    }

    fn backups(folder: &Path) -> Vec<String> {
        subdirs(folder)
    }

    fn job(from: &Path, target: &Path, backups: &Path) -> Import {
        Import {
            from: from.to_string_lossy().into_owned(),
            hookshot: Some(target.to_string_lossy().into_owned()),
            profile: None,
            backup_dir: Some(backups.to_string_lossy().into_owned()),
            force: false,
            keep_settings: false,
            dry_run: false,
        }
    }

    #[test]
    fn windows_and_linux_folders_are_the_documented_ones() {
        let home = "/home/player";
        let data = "/home/player/.local/share";
        let appdata = "C:/Users/player/AppData/Roaming";
        let localappdata = "C:/Users/player/AppData/Local";
        let by_name = |name: &str| {
            EMULATORS
                .iter()
                .chain(std::iter::once(&HOOKSHOT))
                .find(|program| program.name == name)
                .unwrap()
        };
        assert_eq!(
            folders_on(&by_name("lichii"), true, home, data, appdata, localappdata),
            [
                "C:/Users/player/AppData/Roaming/lichii",
                "C:/Users/player/AppData/Roaming/eden"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            folders_on(&by_name("nx2navi"), true, home, data, appdata, localappdata),
            [
                "C:/Users/player/AppData/Local/Programs/NX2navi/game",
                "C:/Games/Ocarina"
            ]
            .map(PathBuf::from)
        );
        assert!(
            folders_on(
                &by_name("hookshot"),
                true,
                home,
                data,
                appdata,
                localappdata
            )
            .is_empty(),
            "on Windows Hookshot's game folder is asked for"
        );
        assert_eq!(
            folders_on(&by_name("lichii"), false, home, data, appdata, localappdata),
            [
                "/home/player/.local/share/lichii",
                "/home/player/.local/share/eden"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            folders_on(
                &by_name("nx2navi"),
                false,
                home,
                data,
                appdata,
                localappdata
            ),
            ["/home/player/Games/Ocarina"].map(PathBuf::from)
        );
        assert_eq!(
            folders_on(
                &by_name("hookshot"),
                false,
                home,
                data,
                appdata,
                localappdata
            ),
            ["/home/player/.local/share/hookshot"].map(PathBuf::from)
        );
    }

    #[test]
    fn finds_the_saves_at_every_level() {
        let root = temp("find");
        let save = eden_save(&root.join("eden"), PROFILE);
        make_save(&save, "SYNTH", &["slot_00"]);
        for start in [
            root.join("eden"),
            root.join("eden/nand"),
            root.join("eden/nand/user/save"),
            save.parent().unwrap().to_path_buf(),
            save.clone(),
        ] {
            assert_eq!(find_saves(&start).get(PROFILE), Some(&save), "{start:?}");
        }
        assert!(find_saves(&root.join("elsewhere")).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_the_saves_in_a_folder() {
        let root = temp("list");
        let data = root.join("eden");
        let save = eden_save(&data, PROFILE);
        make_save(&save, "SYNTH", &["slot_00", "slot_01"]);
        let rows = saves_in(&data.to_string_lossy()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], PROFILE);
        assert_eq!(rows[0]["path"], save.to_string_lossy().as_ref());
        let label = rows[0]["label"].as_str().unwrap();
        assert!(label.contains(&PROFILE[..8]), "{label}");
        assert!(label.contains("2 slots"), "{label}");
        let empty = root.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let error = saves_in(&empty.to_string_lossy()).unwrap_err();
        assert!(
            error.contains("no Ocarina of Time save under"),
            "wrong message: {error}"
        );
        assert!(error.contains(&empty.to_string_lossy().as_ref()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_the_user_names_from_the_emulators_list() {
        let root = temp("users");
        let data = root.join("eden");
        let save = eden_save(&data, PROFILE);
        make_save(&save, "SYNTH", &["slot_00"]);
        let nand = data.join("nand");
        let list = nand.join("system/save/8000000000000010/su/avators/profiles.dat");
        fs::create_dir_all(list.parent().unwrap()).unwrap();
        let mut bytes = vec![0u8; 0x10];
        let lo = u64::from_str_radix(&PROFILE[16..], 16).unwrap();
        let hi = u64::from_str_radix(&PROFILE[..16], 16).unwrap();
        bytes.extend_from_slice(&lo.to_le_bytes());
        bytes.extend_from_slice(&hi.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 24]);
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"Link");
        bytes.extend_from_slice(&name);
        bytes.extend_from_slice(&[0u8; 0x80]);
        fs::write(&list, bytes).unwrap();
        assert_eq!(user_name(&save).as_deref(), Some("Link"));
        let rows = saves_in(&data.to_string_lossy()).unwrap();
        assert_eq!(rows[0]["user"], "Link");
        assert!(
            rows[0]["label"]
                .as_str()
                .unwrap()
                .starts_with("Link (FEDCBA98…):")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imports_and_backs_up_what_it_overwrites() {
        let root = temp("import");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00", "slot_01"]);
        let before = tree(&target);
        let backups_dir = root.join("backups");
        let answer = import_with(&job(&data, &target, &backups_dir), &[]).unwrap();
        assert_eq!(tree(&target), tree(&source));
        assert!(!target.join("slot_01").exists());
        let made = backups(&backups_dir);
        assert_eq!(made.len(), 1);
        assert!(made[0].starts_with("import-folder-"), "{}", made[0]);
        assert_eq!(tree(&backups_dir.join(&made[0])), before);
        assert!(answer["text"].as_str().unwrap().contains("Done."));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_new_profile_is_filled_and_nothing_is_backed_up() {
        let root = temp("new");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_02"]);
        let target = root.join("hookshot/save").join(UID);
        let backups_dir = root.join("backups");
        import_with(&job(&data, &target, &backups_dir), &[]).unwrap();
        assert_eq!(tree(&target), tree(&source));
        assert!(!backups_dir.exists() || backups(&backups_dir).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_selection_picks_one_and_lists_them_all() {
        let root = temp("profiles");
        let data = root.join("eden");
        make_save(&eden_save(&data, PROFILE), "ONE", &["slot_00"]);
        make_save(&eden_save(&data, OTHER), "TWO", &["slot_01"]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let backups_dir = root.join("backups");
        let error =
            import_with(&job(&data, &target, &backups_dir), &[]).expect_err("ambiguous profiles");
        assert!(error.contains(PROFILE) && error.contains(OTHER), "{error}");
        assert!(error.contains("--profile"), "{error}");
        assert!(backups(&backups_dir).is_empty());

        let mut wanted = job(&data, &target, &backups_dir);
        wanted.profile = Some(OTHER.to_lowercase());
        import_with(&wanted, &[]).unwrap();
        assert_eq!(tree(&target), tree(&eden_save(&data, OTHER)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_waiting_nx2navi_save_needs_force() {
        let root = temp("force");
        let game = root.join("nx2navi");
        let source = game.join("profile/save").join(ACCOUNT);
        make_save(&source, "NX", &["slot_05"]);
        let waiting = game.join("profile/save-tx");
        fs::create_dir_all(&waiting).unwrap();
        fs::write(waiting.join("commit"), "pending").unwrap();
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let backups_dir = root.join("backups");
        let mut request = job(&game, &target, &backups_dir);
        let error = import_with(&request, &[]).expect_err("waiting save");
        assert!(error.contains("save-tx"), "{error}");
        assert!(backups(&backups_dir).is_empty());
        request.force = true;
        import_with(&request, &[]).unwrap();
        assert_eq!(tree(&target), tree(&source));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_running_game_needs_force() {
        let root = temp("running");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let backups_dir = root.join("backups");
        let mut request = job(&data, &target, &backups_dir);
        let error = import_with(&request, &["Hookshot".to_owned()]).expect_err("running game");
        assert!(error.contains("close Hookshot first"), "{error}");
        request.force = true;
        import_with(&request, &["Hookshot".to_owned()]).unwrap();
        assert_eq!(tree(&target), tree(&source));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let root = temp("dry");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00", "slot_01"]);
        let before = tree(&target);
        let backups_dir = root.join("backups");
        let mut request = job(&data, &target, &backups_dir);
        request.dry_run = true;
        let answer = import_with(&request, &["Hookshot".to_owned()]).unwrap();
        assert_eq!(tree(&target), before);
        assert!(backups(&backups_dir).is_empty());
        assert!(answer["text"].as_str().unwrap().contains("Would copy"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_backup_inside_the_target_is_refused() {
        let root = temp("inside");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let before = tree(&target);
        let backups_dir = target.join("backups");
        let error =
            import_with(&job(&data, &target, &backups_dir), &[]).expect_err("backup inside target");
        assert!(
            error.contains("--backup-dir outside the target save folder"),
            "{error}"
        );
        assert!(!backups_dir.exists());
        assert_eq!(tree(&target), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_path_below_the_target_is_inside_it() {
        let root = temp("inside-missing");
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let backup = target.join("backups").join("import-folder");
        assert!(
            inside(&backup, &target),
            "{} should be inside {}",
            backup.display(),
            target.display()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_save_without_slots_or_with_empty_files_is_refused() {
        let root = temp("broken");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &[]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let before = tree(&target);
        let backups_dir = root.join("backups");
        let error = import_with(&job(&data, &target, &backups_dir), &[]).unwrap_err();
        assert!(error.contains("has no slot_* folders"), "{error}");
        make_save(&source, "SOURCE", &["slot_00"]);
        fs::write(source.join("slot_00/progress.sav"), b"").unwrap();
        let error = import_with(&job(&data, &target, &backups_dir), &[]).unwrap_err();
        assert!(error.contains("is empty: the save is not whole"), "{error}");
        assert_eq!(tree(&target), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_title_folder_without_a_save_is_a_target() {
        let root = temp("empty-title");
        let save = eden_save(
            &root.join("portable/user"),
            "11111111111111111111111111111111",
        );
        fs::create_dir_all(&save).unwrap();
        assert_eq!(
            find_saves(&root.join("portable/user")).get("11111111111111111111111111111111"),
            Some(&save)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_engine_request_imports_a_save() {
        let root = temp("request");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let backups_dir = root.join("backups");
        let answer = import_json(&json!({
            "command": "saves.import",
            "from": data.to_string_lossy(),
            "hookshot": target.to_string_lossy(),
            "backup_dir": backups_dir.to_string_lossy(),
            "profile": PROFILE,
            "force": true,
        }))
        .unwrap();
        assert_eq!(tree(&target), tree(&source));
        // (a path, not its text: Windows writes the separator differently)
        assert_eq!(
            Path::new(answer["path"].as_str().unwrap()),
            target.as_path()
        );
        assert!(
            answer["text"]
                .as_str()
                .unwrap()
                .contains("Copying 1 slot(s)")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_source_is_refused() {
        let root = temp("link");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let secret = root.join("secret.txt");
        fs::write(&secret, b"not a save").unwrap();
        fs::remove_file(source.join("slot_00/progress.sav")).unwrap();
        std::os::unix::fs::symlink(&secret, source.join("slot_00/progress.sav")).unwrap();
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let before = tree(&target);
        let backups_dir = root.join("backups");
        let error = import_with(&job(&data, &target, &backups_dir), &[]).unwrap_err();
        assert!(error.contains("is a link"), "{error}");
        assert_eq!(tree(&target), before);
        assert_eq!(fs::read(&secret).unwrap(), b"not a save");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_as_a_loose_save_file_is_refused() {
        let root = temp("loose-link");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let secret = root.join("secret.txt");
        fs::write(&secret, b"not a save").unwrap();
        fs::remove_file(source.join("account.sav")).unwrap();
        std::os::unix::fs::symlink(&secret, source.join("account.sav")).unwrap();
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_00"]);
        let before = tree(&target);
        let backups_dir = root.join("backups");
        let error = import_with(&job(&data, &target, &backups_dir), &[]).unwrap_err();
        assert!(error.contains("is a link"), "{error}");
        assert_eq!(tree(&target), before);
        assert_eq!(fs::read(&secret).unwrap(), b"not a save");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn slots_are_copied_in_name_order_whatever_the_directory_order() {
        let root = temp("slot-order");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_02"]);
        make_save(&source, "SOURCE", &["slot_00"]);
        make_save(&source, "SOURCE", &["slot_01"]);
        assert_eq!(slots(&source), ["slot_00", "slot_01", "slot_02"]);
        let target = root.join("hookshot/save").join(UID);
        make_save(&target, "TARGET", &["slot_09"]);
        let backups_dir = root.join("backups");
        let answer = import_with(&job(&data, &target, &backups_dir), &[]).unwrap();
        let text = answer["text"].as_str().unwrap();
        assert!(
            text.contains("slot_00, slot_01, slot_02"),
            "the copy names the slots in order: {text}"
        );
        assert_eq!(tree(&target), tree(&source));
        assert!(!target.join("slot_09").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_copied_user_folder_needs_32_hex_digits_for_a_profile() {
        let root = temp("copied-user");
        let data = root.join("eden");
        // (a folder of users whose name is not the emulator's: only a folder
        // named as a user is one, not Eden's mods folder load/<title id>)
        let copied = data.join("save");
        let mods = copied.join("load").join(TITLE);
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("account.sav"), b"not a save").unwrap();
        let real = copied.join(PROFILE).join(TITLE);
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("account.sav"), b"save").unwrap();
        let found = find_saves(&copied);
        assert_eq!(found.get(PROFILE), Some(&real));
        assert!(!found.contains_key("load"), "{found:?}");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_target_that_is_the_source_through_a_link_is_refused() {
        let root = temp("same-link");
        let data = root.join("eden");
        let source = eden_save(&data, PROFILE);
        make_save(&source, "SOURCE", &["slot_00"]);
        let target = root.join("hookshot/save").join(UID);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&source, &target).unwrap();
        let backups_dir = root.join("backups");
        let error = import_with(&job(&data, &target, &backups_dir), &[]).unwrap_err();
        assert!(error.contains("both the source and the target"), "{error}");
        assert!(source.join("account.sav").is_file());
        let _ = fs::remove_dir_all(root);
    }
}
