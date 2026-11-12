//! The relocatable Builder home: its marker, the game's relative association,
//! and the path rules a portable build keeps. The Windows setup
//! (`builder/setup/portable_home.h`) writes the same marker and association;
//! `host/hos_update.c` reads the association through the game's launcher.
//!
//! A portable home is never inferred from the current directory. It is either
//! named by the setup (`HOOKSHOT_SETUP_BASE`) or found by walking up from the
//! running executable to a folder marked `.hookshot-portable`. Paths on the
//! home's volume are stored relative to it, so moving the folder (or changing
//! the drive letter) keeps settings, work files and the game's Builder link
//! working. Paths on another volume stay absolute, as upstream's WPF builder
//! does.
#![allow(dead_code)] // (the native setup and the tests exercise the same rules; the packager calls the rest)

use std::fs;
use std::path::{Path, PathBuf};

pub const HOME_MARKER: &str = ".hookshot-portable";
pub const HOME_MAGIC: &str = "Hookshot Builder portable home v1";
pub const GAME_HOME: &str = ".hookshot_builder_home";
pub const SETUP_BASE: &str = "HOOKSHOT_SETUP_BASE";
pub const PORTABLE: &str = "HOOKSHOT_BUILDER_PORTABLE";

/// A resolved Builder home. `portable` is true only for a marked folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub root: PathBuf,
    pub portable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Missing,
    Valid,
    Invalid,
}

/// Split an absolute path into its root and normal parts. Windows drive and
/// UNC roots are recognised from the text, so the rules can be tested on any
/// host; device namespace paths are refused.
fn split_root(path: &str) -> Option<(String, Vec<String>)> {
    if path.is_empty() || path.contains('\0') {
        return None;
    }
    let text = path.replace('\\', "/");
    if text.starts_with("//?/") || text.starts_with("//./") {
        return None;
    }
    if let Some(rest) = text.strip_prefix("//") {
        let mut parts = rest.split('/').filter(|part| !part.is_empty());
        let server = parts.next()?;
        let share = parts.next()?;
        if server.contains(':') || share.contains(':') {
            return None;
        }
        return Some((
            format!("//{server}/{share}"),
            parts.map(str::to_owned).collect(),
        ));
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        return Some((
            text[..2].to_ascii_uppercase(),
            text[3..]
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect(),
        ));
    }
    if text.starts_with('/') {
        return Some((
            "/".to_owned(),
            text[1..]
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect(),
        ));
    }
    None
}

fn looks_windows(path: &str) -> bool {
    let text = path.replace('\\', "/");
    let bytes = text.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/')
        || text.starts_with("//")
}

fn volume_root(path: &str) -> bool {
    split_root(path).is_some_and(|(_, parts)| parts.is_empty())
}

fn marker_state(root: &Path) -> Marker {
    let path = root.join(HOME_MARKER);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Marker::Missing,
        Err(_) => return Marker::Invalid,
    };
    let mut length = bytes.len();
    if length > HOME_MAGIC.len() + 2 {
        return Marker::Invalid;
    }
    while length > 0 && (bytes[length - 1] == b'\r' || bytes[length - 1] == b'\n') {
        length -= 1;
    }
    let root_text = root.to_string_lossy();
    if &bytes[..length] == HOME_MAGIC.as_bytes() && !volume_root(&root_text) {
        Marker::Valid
    } else {
        Marker::Invalid
    }
}

/// True when `root` has the exact portable marker and is a dedicated folder.
pub fn marked(root: &Path) -> bool {
    marker_state(root) == Marker::Valid
}

/// The relative text from `from` to `to`: forward slashes (the native setup
/// accepts either separator), `..` allowed for a sibling folder, never a root
/// or a colon. The game's association is `relative_between(game, home)`.
pub fn relative_between(from: &str, to: &str) -> Result<String, String> {
    let (from_root, from_parts) = split_root(from).ok_or_else(|| {
        "The portable Builder home must be a fully qualified drive or UNC folder.".to_owned()
    })?;
    let (to_root, to_parts) = split_root(to)
        .ok_or_else(|| "The game folder must be a fully qualified folder.".to_owned())?;
    if !from_root.eq_ignore_ascii_case(&to_root) {
        return Err("Keep a portable game and its Builder on the same volume.".to_owned());
    }
    if looks_windows(from) && from.len() >= 260 {
        return Err(
            "The portable Builder home is too long for the Windows setup. Use a shorter folder."
                .to_owned(),
        );
    }
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; from_parts.len() - common];
    parts.extend(to_parts[common..].iter().map(String::as_str));
    if parts.is_empty() {
        parts.push(".");
    }
    let relative = parts.join("/");
    if relative.len() >= 780 {
        return Err(
            "The portable Builder association is too long for the Windows setup.".to_owned(),
        );
    }
    if relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.contains(':')
        || relative.contains('\r')
        || relative.contains('\n')
        || relative.contains('\0')
    {
        return Err("The portable Builder association cannot be represented.".to_owned());
    }
    Ok(relative)
}

fn join_relative(base: &str, relative: &str) -> Result<String, String> {
    let (root, mut parts) = split_root(base)
        .ok_or_else(|| "The Builder home must be a fully qualified folder.".to_owned())?;
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.contains(':')
        || relative.contains('\r')
        || relative.contains('\n')
        || relative.contains('\0')
    {
        return Err("The portable Builder association is not a relative path.".to_owned());
    }
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("The portable Builder association leaves its volume.".to_owned());
                }
            }
            name => parts.push(name.to_owned()),
        }
    }
    if root == "/" {
        Ok(format!("/{}", parts.join("/")))
    } else {
        Ok(format!("{root}/{}", parts.join("/")))
    }
}

/// Resolve a game's relative association against the game folder and accept it
/// only when it reaches a marked Builder home.
pub fn resolve_association(game: &Path, relative: &str) -> Option<PathBuf> {
    let game_text = game.to_str()?;
    let resolved = join_relative(game_text, relative).ok()?;
    let path = PathBuf::from(resolved);
    marked(&path).then_some(path)
}

/// Find the marked home from the running executable (setup's
/// `versions/<tag>/HookshotBuilder`, a ZIP's `builders/<tag>/HookshotBuilder`,
/// or a plain portable folder). `requested` is the setup's explicit home.
pub fn discover(requested: Option<&str>, executable: &str, standard: &str) -> Result<Home, String> {
    if let Some(requested) = requested.filter(|path| !path.is_empty()) {
        let (root, _) = split_root(requested).ok_or_else(|| {
            "The portable Builder home must be a fully qualified drive or UNC folder.".to_owned()
        })?;
        let full = normalize_root(requested, &root);
        if volume_root(&full) {
            return Err(
                "A portable Builder home must be a dedicated folder, not a volume root.".to_owned(),
            );
        }
        return match marker_state(Path::new(&full)) {
            Marker::Valid => Ok(Home {
                root: PathBuf::from(full),
                portable: true,
            }),
            Marker::Missing => Ok(Home {
                root: PathBuf::from(full),
                portable: false,
            }),
            Marker::Invalid => Err(format!(
                "The portable Builder home marker is invalid: {full}"
            )),
        };
    }
    let (_, executable_parts) = split_root(executable)
        .ok_or_else(|| "The Builder executable must be in a fully qualified folder.".to_owned())?;
    let mut parts = executable_parts;
    parts.pop();
    for _ in 0..=3 {
        let candidate = build_root(executable, &parts);
        match marker_state(Path::new(&candidate)) {
            Marker::Valid => {
                return Ok(Home {
                    root: PathBuf::from(candidate),
                    portable: true,
                });
            }
            Marker::Invalid => {
                return Err(format!(
                    "The portable Builder home marker is invalid: {candidate}"
                ));
            }
            Marker::Missing => {}
        }
        if parts.is_empty() {
            break;
        }
        parts.pop();
    }
    let (standard_root, _) = split_root(standard)
        .ok_or_else(|| "The standard Builder home must be a fully qualified folder.".to_owned())?;
    Ok(Home {
        root: PathBuf::from(normalize_root(standard, &standard_root)),
        portable: false,
    })
}

fn root_path(root: &str, parts: &[String]) -> String {
    if parts.is_empty() {
        if root == "/" {
            "/".to_owned()
        } else {
            format!("{root}/")
        }
    } else {
        format!("{root}/{}", parts.join("/"))
    }
}

fn build_root(path: &str, parts: &[String]) -> String {
    let (root, _) = split_root(path).expect("absolute path");
    root_path(&root, parts)
}

fn normalize_root(path: &str, root: &str) -> String {
    let (_, parts) = split_root(path).expect("absolute path");
    root_path(root, &parts)
}

/// The portable home this process was given, or `None` for a standard build.
/// An explicitly portable process with a missing or invalid home is an error,
/// never a silent AppData fallback.
pub fn from_env() -> Result<Option<Home>, String> {
    let portable = std::env::var(PORTABLE).ok().as_deref() == Some("1");
    if !portable {
        return Ok(None);
    }
    let base = std::env::var(SETUP_BASE)
        .ok()
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "The portable Builder home was not passed to this process.".to_owned())?;
    let (root, _) = split_root(&base).ok_or_else(|| {
        "The portable Builder home must be a fully qualified drive or UNC folder.".to_owned()
    })?;
    let full = normalize_root(&base, &root);
    if volume_root(&full) {
        return Err(
            "A portable Builder home must be a dedicated folder, not a volume root.".to_owned(),
        );
    }
    match marker_state(Path::new(&full)) {
        Marker::Valid => Ok(Some(Home {
            root: PathBuf::from(full),
            portable: true,
        })),
        Marker::Missing | Marker::Invalid => Err(format!(
            "The portable Builder home is missing its marker or it is invalid: {full}"
        )),
    }
}

/// Validate the portable context before a long build changes output.
pub fn check_env() -> Result<(), String> {
    from_env().map(|_| ())
}

/// The game's association for this build, when the build is portable.
pub fn association_for(game: &Path) -> Result<Option<String>, String> {
    let Some(home) = from_env()? else {
        return Ok(None);
    };
    let game_text = game
        .to_str()
        .ok_or_else(|| "The game folder is not valid UTF-8.".to_owned())?;
    let home_text = home.root.to_string_lossy();
    relative_between(game_text, &home_text).map(Some)
}

/// Write or remove the game's `.hookshot_builder_home`. A standard rebuild
/// removes a former portable association before publishing its own metadata;
/// a failed removal is an error, so setup and updater never disagree.
pub fn write_game_home(game: &Path, association: Option<&str>) -> Result<(), String> {
    let destination = game.join(GAME_HOME);
    let Some(relative) = association else {
        return match fs::remove_file(&destination) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "The former portable Builder association could not be removed ({}): {error}",
                destination.display()
            )),
        };
    };
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.contains(':')
        || relative.contains('\r')
        || relative.contains('\n')
        || relative.contains('\0')
    {
        return Err("The portable Builder association cannot be represented.".to_owned());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let partial = game.join(format!(".hookshot-home-{}-{stamp}.tmp", std::process::id()));
    fs::write(&partial, relative).map_err(|error| {
        format!(
            "The portable Builder association could not be written ({}): {error}",
            partial.display()
        )
    })?;
    #[cfg(windows)]
    if destination.exists() {
        fs::remove_file(&destination).map_err(|error| {
            let _ = fs::remove_file(&partial);
            format!(
                "The former portable Builder association could not be replaced ({}): {error}",
                destination.display()
            )
        })?;
    }
    if let Err(error) = fs::rename(&partial, &destination) {
        let _ = fs::remove_file(&partial);
        return Err(format!(
            "The portable Builder association could not be published ({}): {error}",
            destination.display()
        ));
    }
    Ok(())
}

/// `hookshot_version.txt`: profiles, the release, and for a portable build the
/// game-relative `builder_local=1` (never an absolute Builder path).
pub fn version_text(release: &str, association: Option<&str>) -> String {
    let mut text = String::from("profiles=1\n");
    if !release.is_empty() && release != "unknown" {
        text.push_str(&format!("version={release}\n"));
    }
    if association.is_some() {
        text.push_str("builder_local=1\n");
    }
    text
}

/// Store a path relative to a portable home when it is on the home's volume;
/// another volume keeps its absolute path. Standard installs keep everything.
pub fn save_path(home: &Home, path: &str) -> Result<String, String> {
    if !home.portable || path.is_empty() {
        return Ok(path.to_owned());
    }
    let home_text = home.root.to_string_lossy();
    if split_root(path).is_none() {
        return Ok(path.to_owned());
    }
    let (path_root, _) = split_root(path).expect("absolute path");
    let (home_root, _) = split_root(&home_text).expect("absolute home");
    if !path_root.eq_ignore_ascii_case(&home_root) {
        return Ok(path.to_owned());
    }
    relative_between(&home_text, path)
}

/// Read a stored path back: a relative value is anchored to the current home,
/// so a moved folder resolves to its new location.
pub fn load_path(home: &Home, path: &str) -> Result<String, String> {
    if !home.portable || path.is_empty() || split_root(path).is_some() {
        return Ok(path.to_owned());
    }
    join_relative(&home.root.to_string_lossy(), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hookshot-portable-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn mark(folder: &Path) {
        fs::write(folder.join(HOME_MARKER), format!("{HOME_MAGIC}\n")).unwrap();
    }

    #[test]
    fn a_marker_is_exact_and_a_volume_root_is_never_a_home() {
        let folder = temp("marker");
        assert_eq!(marker_state(&folder), Marker::Missing);
        mark(&folder);
        assert!(marked(&folder));
        fs::write(folder.join(HOME_MARKER), HOME_MAGIC).unwrap();
        assert!(marked(&folder));
        fs::write(folder.join(HOME_MARKER), "wrong\n").unwrap();
        assert!(!marked(&folder));
        fs::write(folder.join(HOME_MARKER), format!("{HOME_MAGIC}\n\n\n")).unwrap();
        assert!(!marked(&folder));
        assert!(!marked(Path::new("/")));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn same_volume_paths_are_relative_and_other_volumes_stay_absolute() {
        let relative = relative_between(r"C:\SSD\HookshotBuilder", r"C:\SSD\Hookshot").unwrap();
        assert_eq!(relative, "../Hookshot");
        let inside = relative_between(
            r"C:\SSD\HookshotBuilder",
            r"C:\SSD\HookshotBuilder\versions\v1",
        )
        .unwrap();
        assert_eq!(inside, "versions/v1");
        let other = relative_between(r"C:\SSD\HookshotBuilder", r"D:\Games\Hookshot");
        assert!(other.unwrap_err().contains("same volume"));
        let posix = relative_between("/mnt/ssd/Builder", "/mnt/ssd/Game").unwrap();
        assert_eq!(posix, "../Game");
    }

    #[test]
    fn a_relative_association_resolves_after_the_tree_moves() {
        let root = temp("move");
        let home = root.join("Portable ü, tree").join("HookshotBuilder");
        let game = home.parent().unwrap().join("Hookshot");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&game).unwrap();
        mark(&home);
        let relative = relative_between(&game.to_string_lossy(), &home.to_string_lossy()).unwrap();
        assert_eq!(relative, "../HookshotBuilder");
        assert_eq!(resolve_association(&game, &relative), Some(home.clone()));
        let moved = root.join("Relocated ü, tree");
        fs::rename(root.join("Portable ü, tree"), &moved).unwrap();
        let moved_home = moved.join("HookshotBuilder");
        let moved_game = moved.join("Hookshot");
        assert_eq!(
            resolve_association(&moved_game, &relative),
            Some(moved_home)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_invalid_or_escaped_association_is_refused() {
        let root = temp("invalid");
        let game = root.join("Game");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(root.join("Builder")).unwrap();
        mark(&root.join("Builder"));
        assert_eq!(resolve_association(&game, "wrong"), None);
        assert_eq!(resolve_association(&game, "/absolute"), None);
        assert_eq!(resolve_association(&game, "C:\\absolute"), None);
        assert!(join_relative("/a/b", "../../../../x").is_err());
        assert!(relative_between("/a/b", "/a/b").is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_volume_root_is_not_a_home() {
        let standard = temp("volume-standard");
        assert!(discover(Some(r"C:\"), r"C:\setup.exe", &standard.to_string_lossy()).is_err());
        assert!(discover(Some("/"), "/setup", &standard.to_string_lossy()).is_err());
        let _ = fs::remove_dir_all(standard);
    }

    #[test]
    fn discovery_finds_the_home_from_the_installed_executable_and_after_a_move() {
        let root = temp("discover");
        let home = root.join("Builder");
        let exe = home.join("versions/v1/HookshotBuilder/HookshotBuilder.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        mark(&home);
        let found = discover(None, &exe.to_string_lossy(), "/standard").unwrap();
        assert_eq!(found.root, home);
        assert!(found.portable);
        let moved = root.join("Moved ü");
        fs::rename(&home, &moved).unwrap();
        let moved_exe = moved.join("versions/v1/HookshotBuilder/HookshotBuilder.exe");
        let found = discover(None, &moved_exe.to_string_lossy(), "/standard").unwrap();
        assert_eq!(found.root, moved);
        let standard = discover(None, "/opt/app/HookshotBuilder", "/standard").unwrap();
        assert_eq!(standard.root, PathBuf::from("/standard"));
        assert!(!standard.portable);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_update_state_names_the_game_relative_launcher_only() {
        let text = version_text("v2.2.0", Some("../HookshotBuilder"));
        assert_eq!(text, "profiles=1\nversion=v2.2.0\nbuilder_local=1\n");
        assert!(!text.contains("builder="));
        let standard = version_text("v2.2.0", None);
        assert_eq!(standard, "profiles=1\nversion=v2.2.0\n");
        assert!(!standard.contains("builder_local"));
    }

    #[test]
    fn a_game_home_is_written_relative_and_a_standard_build_removes_it() {
        let root = temp("game-home");
        let game = root.join("Hookshot");
        fs::create_dir_all(&game).unwrap();
        write_game_home(&game, Some("../HookshotBuilder")).unwrap();
        assert_eq!(
            fs::read_to_string(game.join(GAME_HOME)).unwrap(),
            "../HookshotBuilder"
        );
        write_game_home(&game, None).unwrap();
        assert!(!game.join(GAME_HOME).exists());
        write_game_home(&game, None).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn saved_paths_stay_relative_and_resolve_against_the_current_home() {
        let home = Home {
            root: PathBuf::from(r"C:\SSD\HookshotBuilder"),
            portable: true,
        };
        assert_eq!(
            save_path(&home, r"C:\SSD\HookshotBuilder\versions\v1").unwrap(),
            "versions/v1"
        );
        assert_eq!(
            save_path(&home, r"D:\Elsewhere\game").unwrap(),
            r"D:\Elsewhere\game"
        );
        assert_eq!(
            load_path(&home, "versions/v1").unwrap(),
            "C:/SSD/HookshotBuilder/versions/v1"
        );
        assert_eq!(
            load_path(&home, r"D:\Elsewhere\game").unwrap(),
            r"D:\Elsewhere\game"
        );
        let standard = Home {
            root: PathBuf::from("/data"),
            portable: false,
        };
        assert_eq!(save_path(&standard, "/data/game").unwrap(), "/data/game");
    }
}
