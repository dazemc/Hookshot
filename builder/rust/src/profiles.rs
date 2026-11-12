//! The launcher's profiles: one game user per profile, save/<32 hex id>.
//! Ported from nx2/profiles.py; the same profiles.txt is read by the game.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde_json::{Value, json};

pub const DEFAULT: &str = "5a3c9e1f0b7d24680000000000000001";
const PREFIX: &str = "5a3c9e1f0b7d2468";
const LIST: &str = "profiles.txt";
const NAME_MAX: usize = 32;

pub fn user_dir(folder: &str) -> PathBuf {
    let beside = Path::new(folder).join("user");
    if let Ok(entries) = fs::read_dir(&beside) {
        if entries.into_iter().next().is_some() {
            return beside;
        }
    }
    if cfg!(windows) {
        return beside;
    }
    if let Some(xdg) =
        std::env::var_os("XDG_DATA_HOME").filter(|v| v.to_string_lossy().starts_with('/'))
    {
        return PathBuf::from(xdg).join("hookshot");
    }
    if let Some(home) = std::env::var_os("HOME").filter(|v| v.to_string_lossy().starts_with('/')) {
        return PathBuf::from(home).join(".local/share/hookshot");
    }
    beside
}

pub fn supported(folder: &str) -> bool {
    fs::read_to_string(Path::new(folder).join("hookshot_version.txt"))
        .map(|text| text.lines().any(|line| line.trim() == "profiles=1"))
        .unwrap_or(false)
}

pub fn clean_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(NAME_MAX)
        .collect()
}

pub fn load(user: &Path) -> Vec<(String, String)> {
    let mut profiles = Vec::new();
    if let Ok(bytes) = fs::read(user.join(LIST)) {
        for line in String::from_utf8_lossy(&bytes).lines() {
            let line = line.trim();
            let mut split = line.splitn(2, ' ');
            let uid = split.next().unwrap_or("").to_ascii_lowercase();
            let name = clean_name(split.next().unwrap_or(""));
            if uid.len() == 32
                && uid.chars().all(|c| c.is_ascii_hexdigit())
                && uid != "0".repeat(32)
                && !profiles.iter().any(|(id, _)| id == &uid)
            {
                profiles.push((
                    uid.clone(),
                    if name.is_empty() {
                        uid[28..].to_owned()
                    } else {
                        name
                    },
                ));
            }
        }
    }
    if !profiles.iter().any(|(id, _)| id == DEFAULT) {
        profiles.insert(0, (DEFAULT.to_owned(), "Player".to_owned()));
    } else {
        profiles.sort_by_key(|(id, _)| id != DEFAULT);
    }
    profiles
}

pub fn store(user: &Path, profiles: &[(String, String)]) -> Result<(), String> {
    fs::create_dir_all(user).map_err(err)?;
    let path = user.join(LIST);
    let mut body = String::from(
        "# Hookshot's profiles: the save folder (save/<id>) and the name the launcher shows\n",
    );
    for (id, name) in profiles {
        body.push_str(&format!("{id} {name}\n"));
    }
    let temp = user.join(format!("{LIST}.tmp"));
    fs::write(&temp, body).map_err(err)?;
    fs::rename(&temp, path).map_err(err)
}

pub fn save_dir(user: &Path, uid: &str) -> PathBuf {
    user.join("save").join(uid)
}

/// A profile id names one folder below `save/`: 32 hex digits, as
/// host/nn_fs.c and profiles.txt use. Anything else (an empty id, `..`, a
/// separator) would name a folder outside the profile's own, so it is
/// refused before it reaches a path.
fn profile_id(uid: &str) -> Result<&str, String> {
    if uid.len() == 32 && uid.bytes().all(|byte| byte.is_ascii_hexdigit()) && uid != "0".repeat(32)
    {
        Ok(uid)
    } else {
        Err(format!(
            "\"{uid}\" is not a profile id (32 hex digits): no save folder was used"
        ))
    }
}

pub fn folder_to_show(user: &Path, uid: &str) -> Option<PathBuf> {
    profile_id(uid).ok()?;
    for path in [save_dir(user, uid), user.join("save"), user.to_path_buf()] {
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

pub fn state(folder: &str, selected: Option<&str>) -> Result<Value, String> {
    let user = user_dir(folder);
    let list = load(&user);
    let selected = selected
        .filter(|id| list.iter().any(|(uid, _)| uid == id))
        .map(str::to_owned)
        .unwrap_or_else(|| list.first().map(|(id, _)| id.clone()).unwrap_or_default());
    Ok(json!({
        "user": user.to_string_lossy(),
        "supported": supported(folder),
        "profiles": list.iter().map(|(id, name)| json!({
            "id": id,
            "name": name,
            "default": id == DEFAULT,
        })).collect::<Vec<_>>(),
        "selected": selected,
    }))
}

pub fn add(folder: &str, name: &str) -> Result<Value, String> {
    let user = user_dir(folder);
    let name = clean_name(name);
    if name.is_empty() {
        return Err("Give the profile a name.".into());
    }
    let mut list = load(&user);
    if list
        .iter()
        .any(|(_, n)| n.to_lowercase() == name.to_lowercase())
    {
        return Err(format!("There is a profile called \"{name}\" already."));
    }
    let mut taken: Vec<String> = list.iter().map(|(id, _)| id.clone()).collect();
    for dir in ["save", "save-backups"] {
        if let Ok(entries) = fs::read_dir(user.join(dir)) {
            for entry in entries.flatten() {
                let text = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if text.len() >= 32 {
                    taken.push(text[..32].to_owned());
                }
            }
        }
    }
    let mut n = 2u64;
    let uid = loop {
        let candidate = format!("{PREFIX}{n:016x}");
        if !taken.contains(&candidate) {
            break candidate;
        }
        n += 1;
    };
    list.push((uid.clone(), name));
    store(&user, &list)?;
    state(folder, Some(&uid))
}

pub fn rename(folder: &str, uid: &str, name: &str) -> Result<Value, String> {
    let user = user_dir(folder);
    let name = clean_name(name);
    if name.is_empty() {
        return Err("Give the profile a name.".into());
    }
    let list = load(&user);
    if list
        .iter()
        .any(|(id, n)| id != uid && n.to_lowercase() == name.to_lowercase())
    {
        return Err(format!("There is a profile called \"{name}\" already."));
    }
    let updated: Vec<_> = list
        .into_iter()
        .map(|(id, n)| (id.clone(), if id == uid { name.clone() } else { n }))
        .collect();
    store(&user, &updated)?;
    state(folder, Some(uid))
}

fn backup(user: &Path, uid: &str) -> Result<Option<PathBuf>, String> {
    let saves = save_dir(user, uid);
    match fs::read_dir(&saves) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                let _ = fs::remove_dir(&saves);
                return Ok(None);
            }
        }
        Err(_) => return Ok(None),
    }
    let root = user.join("save-backups");
    fs::create_dir_all(&root).map_err(err)?;
    let stamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let mut dest = root.join(format!("{uid}-{stamp}"));
    let mut k = 2;
    while dest.exists() {
        dest = root.join(format!("{uid}-{stamp}-{k}"));
        k += 1;
    }
    fs::rename(&saves, &dest).map_err(err)?;
    Ok(Some(dest))
}

pub fn remove(folder: &str, uid: &str) -> Result<Value, String> {
    let uid = profile_id(uid)?;
    if uid == DEFAULT {
        return Err("The default profile cannot be removed: the game plays it when it is started without the launcher.".into());
    }
    let user = user_dir(folder);
    let moved = backup(&user, uid)?;
    let list: Vec<_> = load(&user)
        .into_iter()
        .filter(|(id, _)| id != uid)
        .collect();
    store(&user, &list)?;
    let mut value = state(folder, None)?;
    value["moved"] = moved
        .map(|p| json!(p.to_string_lossy()))
        .unwrap_or(Value::Null);
    Ok(value)
}

pub fn export(folder: &str, uid: &str, parent: &str) -> Result<Value, String> {
    let uid = profile_id(uid)?;
    let user = user_dir(folder);
    let list = load(&user);
    let name = list
        .iter()
        .find(|(id, _)| id == uid)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| uid.to_owned());
    let saves = save_dir(&user, uid);
    let mut slots = Vec::new();
    if let Ok(entries) = fs::read_dir(&saves) {
        for entry in entries.flatten() {
            let item = entry.file_name().to_string_lossy().into_owned();
            if !item.starts_with("slot_") {
                continue;
            }
            // (a linked slot would copy what it points to: any folder of
            // this PC, not the save)
            if crate::saves::is_link(&entry.path()) {
                return Err(format!(
                    "A linked save file cannot be exported safely: {}",
                    entry.path().display()
                ));
            }
            if entry.file_type().map_err(err)?.is_dir() {
                slots.push(item);
            }
        }
    }
    slots.sort();
    if slots.is_empty() {
        return Err(format!("{name} has no saves yet: nothing to export."));
    }
    let parent = Path::new(parent);
    if !parent.is_dir() {
        return Err(format!("There is no folder at {}.", parent.display()));
    }
    let safe: String = name
        .chars()
        .map(|c| {
            if "\\/:*?\"<>|".contains(c) || (c as u32) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let safe = safe.trim_matches(|c: char| c == ' ' || c == '.').to_owned();
    let safe = if safe.is_empty() {
        uid[28..].to_owned()
    } else {
        safe
    };
    let base = parent.join(format!(
        "Hookshot save - {safe} - {}",
        Local::now().format("%Y-%m-%d %H-%M")
    ));
    let mut dest = base.clone();
    let mut k = 2;
    while dest.symlink_metadata().is_ok() {
        dest = PathBuf::from(format!("{} ({k})", base.display()));
        k += 1;
    }
    // (a parent inside the profile's save folder would copy the export into
    // the save, and a parent inside a slot would copy it into itself)
    if crate::saves::inside(&dest, &saves) {
        return Err(format!(
            "Choose a folder outside the profile's save folder ({}), not {}.",
            saves.display(),
            parent.display()
        ));
    }
    fs::create_dir(&dest).map_err(err)?;
    let copied = (|| -> Result<(), String> {
        for slot in &slots {
            copy_tree(&saves.join(slot), &dest.join(slot))?;
        }
        for file in ["account.sav", "option.sav"] {
            let source = saves.join(file);
            if source.symlink_metadata().is_err() {
                continue;
            }
            if crate::saves::is_link(&source) {
                return Err(format!(
                    "A linked save file cannot be exported safely: {}",
                    source.display()
                ));
            }
            if source.is_file() {
                fs::copy(&source, dest.join(file)).map_err(err)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = copied {
        // (no half a save left behind to be imported later)
        let _ = fs::remove_dir_all(&dest);
        return Err(error);
    }
    Ok(json!({"path": dest.to_string_lossy()}))
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(err)?;
    for entry in fs::read_dir(source).map_err(err)?.flatten() {
        let kind = entry.file_type().map_err(err)?;
        let to = target.join(entry.file_name());
        // (a Windows junction is a reparse point, not a symbolic link, and
        // file_type() reports it as a folder: is_link sees both)
        if crate::saves::is_link(&entry.path()) {
            return Err(format!(
                "A linked save file cannot be exported safely: {}",
                entry.path().display()
            ));
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &to).map_err(err)?;
        }
    }
    Ok(())
}

fn err(error: io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hookshot-rust-profiles-{}-{}",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn list_add_rename_remove_round_trip() {
        let folder = temp();
        fs::create_dir_all(folder.join("user/save")).unwrap();
        let folder = folder.to_string_lossy().to_string();
        let value = add(&folder, "  Link  ").unwrap();
        assert_eq!(value["profiles"][1]["name"], "Link");
        let uid = value["selected"].as_str().unwrap().to_owned();
        assert!(uid.starts_with(PREFIX));
        assert!(rename(&folder, &uid, "Zelda").is_ok());
        let state = state(&folder, Some(&uid)).unwrap();
        assert_eq!(state["profiles"][1]["name"], "Zelda");
        let removed = remove(&folder, &uid).unwrap();
        assert!(removed["profiles"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn a_removed_profile_keeps_its_saves() {
        let folder = temp();
        let user = folder.join("user");
        fs::create_dir_all(user.join("save")).unwrap();
        let folder = folder.to_string_lossy().to_string();
        let uid = add(&folder, "Link").unwrap()["selected"]
            .as_str()
            .unwrap()
            .to_owned();
        fs::create_dir_all(save_dir(&user, &uid).join("slot_00")).unwrap();
        fs::write(save_dir(&user, &uid).join("slot_00/progress.sav"), b"save").unwrap();
        let removed = remove(&folder, &uid).unwrap();
        let moved = removed["moved"].as_str().unwrap();
        assert!(Path::new(moved).join("slot_00/progress.sav").is_file());
        assert!(!save_dir(&user, &uid).exists());
    }

    fn make_save(folder: &Path, tag: &str) {
        fs::create_dir_all(folder.join("slot_00")).unwrap();
        fs::write(folder.join("account.sav"), format!("account-{tag}")).unwrap();
        fs::write(folder.join("option.sav"), format!("option-{tag}")).unwrap();
        fs::write(
            folder.join("slot_00/progress.sav"),
            format!("progress-{tag}"),
        )
        .unwrap();
        fs::write(folder.join("slot_00/caption.sav"), format!("caption-{tag}")).unwrap();
    }

    fn exports(parent: &Path) -> Vec<String> {
        fs::read_dir(parent)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn export_copies_the_save_into_a_new_folder() {
        let folder = temp();
        let user = folder.join("user");
        fs::create_dir_all(user.join("save")).unwrap();
        let folder = folder.to_string_lossy().to_string();
        let uid = add(&folder, "Link").unwrap()["selected"]
            .as_str()
            .unwrap()
            .to_owned();
        let saves = save_dir(&user_dir(&folder), &uid);
        make_save(&saves, "ONE");
        let parent = Path::new(&folder).join("exports");
        fs::create_dir_all(&parent).unwrap();

        let value = export(&folder, &uid, &parent.to_string_lossy()).unwrap();
        let dest = PathBuf::from(value["path"].as_str().unwrap());
        assert!(dest.starts_with(&parent), "{}", dest.display());
        assert_eq!(
            fs::read(dest.join("slot_00/progress.sav")).unwrap(),
            b"progress-ONE"
        );
        assert_eq!(fs::read(dest.join("account.sav")).unwrap(), b"account-ONE");
        assert_eq!(fs::read(dest.join("option.sav")).unwrap(), b"option-ONE");
        assert!(!dest.join("slot_01").exists());

        // (a second export in the same minute gets its own folder)
        let again = export(&folder, &uid, &parent.to_string_lossy()).unwrap();
        let second = PathBuf::from(again["path"].as_str().unwrap());
        assert_ne!(dest, second);
        assert!(dest.is_dir());
        assert!(second.is_dir());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_profile_id_must_name_one_folder_below_save() {
        let folder = temp();
        let user = folder.join("user");
        fs::create_dir_all(user.join("save")).unwrap();
        fs::write(user.join("save/keep.txt"), b"keep").unwrap();
        let victim = user.join("victim");
        fs::create_dir_all(&victim).unwrap();
        fs::write(victim.join("keep.txt"), b"keep").unwrap();
        let parent = folder.join("exports");
        fs::create_dir_all(&parent).unwrap();
        let folder = folder.to_string_lossy().to_string();
        let zeros = "0".repeat(32);

        for bad in ["", "..", "../victim", "not-an-id", zeros.as_str()] {
            let error = remove(&folder, bad).unwrap_err();
            assert!(error.contains("not a profile id"), "{bad:?}: {error}");
            assert!(user.join("save/keep.txt").is_file(), "{bad:?}");
            assert!(victim.join("keep.txt").is_file(), "{bad:?}");
            let error = export(&folder, bad, &parent.to_string_lossy()).unwrap_err();
            assert!(error.contains("not a profile id"), "{bad:?}: {error}");
            assert!(victim.join("keep.txt").is_file(), "{bad:?}");
            assert!(folder_to_show(&user, bad).is_none(), "{bad:?}");
        }
        assert!(exports(&parent).is_empty());
        let _ = fs::remove_dir_all(folder);
    }

    #[cfg(unix)]
    #[test]
    fn export_refuses_a_linked_save_and_leaves_no_half_folder() {
        let folder = temp();
        let user = folder.join("user");
        fs::create_dir_all(user.join("save")).unwrap();
        let folder = folder.to_string_lossy().to_string();
        let uid = add(&folder, "Link").unwrap()["selected"]
            .as_str()
            .unwrap()
            .to_owned();
        let saves = save_dir(&user_dir(&folder), &uid);
        make_save(&saves, "ONE");
        let parent = Path::new(&folder).join("exports");
        fs::create_dir_all(&parent).unwrap();
        let secret = Path::new(&folder).join("secret.txt");
        fs::write(&secret, b"not a save").unwrap();

        // (a loose file that is a link would copy what it names)
        fs::remove_file(saves.join("account.sav")).unwrap();
        std::os::unix::fs::symlink(&secret, saves.join("account.sav")).unwrap();
        let error = export(&folder, &uid, &parent.to_string_lossy()).unwrap_err();
        assert!(error.contains("linked save file"), "{error}");
        assert!(exports(&parent).is_empty(), "{:?}", exports(&parent));

        // (a slot that is a link would copy the folder it names)
        fs::remove_file(saves.join("account.sav")).unwrap();
        fs::write(saves.join("account.sav"), b"account-ONE").unwrap();
        let external = Path::new(&folder).join("external");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("progress.sav"), b"outside").unwrap();
        fs::remove_dir_all(saves.join("slot_00")).unwrap();
        std::os::unix::fs::symlink(&external, saves.join("slot_00")).unwrap();
        let error = export(&folder, &uid, &parent.to_string_lossy()).unwrap_err();
        assert!(error.contains("linked save file"), "{error}");
        assert!(exports(&parent).is_empty(), "{:?}", exports(&parent));
        assert_eq!(fs::read(&secret).unwrap(), b"not a save");
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn export_refuses_a_folder_inside_the_save() {
        let folder = temp();
        let user = folder.join("user");
        fs::create_dir_all(user.join("save")).unwrap();
        let folder = folder.to_string_lossy().to_string();
        let uid = add(&folder, "Link").unwrap()["selected"]
            .as_str()
            .unwrap()
            .to_owned();
        let saves = save_dir(&user_dir(&folder), &uid);
        make_save(&saves, "ONE");
        let parent = saves.join("slot_00");
        let before = exports(&parent);
        let error = export(&folder, &uid, &parent.to_string_lossy()).unwrap_err();
        assert!(
            error.contains("outside the profile's save folder"),
            "{error}"
        );
        assert_eq!(exports(&parent), before);
        let _ = fs::remove_dir_all(folder);
    }

    #[cfg(unix)]
    #[test]
    fn export_refuses_a_linked_folder_inside_the_save() {
        let folder = temp();
        let user = folder.join("user");
        fs::create_dir_all(user.join("save")).unwrap();
        let folder = folder.to_string_lossy().to_string();
        let uid = add(&folder, "Link").unwrap()["selected"]
            .as_str()
            .unwrap()
            .to_owned();
        let saves = save_dir(&user_dir(&folder), &uid);
        make_save(&saves, "ONE");
        let parent = Path::new(&folder).join("into-save");
        std::os::unix::fs::symlink(saves.join("slot_00"), &parent).unwrap();
        let before = exports(&saves.join("slot_00"));
        let error = export(&folder, &uid, &parent.to_string_lossy()).unwrap_err();
        assert!(
            error.contains("outside the profile's save folder"),
            "{error}"
        );
        assert_eq!(exports(&saves.join("slot_00")), before);
        let _ = fs::remove_dir_all(folder);
    }
}
