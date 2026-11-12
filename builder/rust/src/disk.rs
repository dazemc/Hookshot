//! Is a folder on a spinning disk? A build reads and writes tens of
//! gigabytes, and the answer is only ever a warning. Ported from nx2/disk.py.
//!
//! A Btrfs folder has a device number no disk has (one of Btrfs's own making,
//! a subvolume), so the sysfs walk has nowhere to start: its disks are found
//! through the mount instead (`/proc/self/mountinfo`, and every disk of the
//! file system in `/sys/fs/btrfs/<id>/devices`).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The nearest folder above `path` that exists (the game folder may be made
/// later, as it often is).
pub fn existing_parent(path: &Path) -> PathBuf {
    let expanded = expand_home(path);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir().unwrap_or_default().join(expanded)
    };
    let mut folder = absolute;
    loop {
        if folder.exists() {
            return folder;
        }
        match folder.parent() {
            Some(parent) => folder = parent.to_path_buf(),
            None => return folder,
        }
    }
}

fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("~") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        if !home.is_empty() {
            return PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']));
        }
    }
    PathBuf::from(text.into_owned())
}

/// True on a hard drive, false on an SSD, None when it cannot be told.
pub fn spinning(path: &Path) -> Option<bool> {
    let folder = existing_parent(path);
    #[cfg(windows)]
    {
        windows::spinning(&folder)
    }
    #[cfg(target_os = "linux")]
    {
        linux::spinning(&folder)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = folder;
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::path::{Path, PathBuf};

    pub fn spinning(path: &Path) -> Option<bool> {
        spinning_in(
            path,
            Path::new("/proc/self/mountinfo"),
            Path::new("/sys"),
            &block_node,
        )
    }

    fn device_number(device: u64) -> (u64, u64) {
        let major = ((device >> 8) & 0xfff) | ((device >> 32) & !0xfff);
        let minor = (device & 0xff) | ((device >> 12) & !0xff);
        (major, minor)
    }

    fn spinning_in(
        path: &Path,
        mountinfo: &Path,
        sysfs: &Path,
        node_of: &dyn Fn(&Path, &Path) -> Option<PathBuf>,
    ) -> Option<bool> {
        let device = fs::metadata(path).ok()?.dev();
        let (major, minor) = device_number(device);
        let node = sysfs
            .join("dev")
            .join("block")
            .join(format!("{major}:{minor}"));
        if node.exists() {
            return rotational(&node, 0);
        }
        // (Btrfs numbers its files' device itself, a number a subvolume and none of
        // them a disk's (0:38): sysfs has no such node, and the walk below ended at
        // None, so a hard drive with Btrfs on it, as Fedora, Bazzite and CachyOS
        // format one, never warned)
        let answers: Vec<bool> = mounted_from_with(path, mountinfo, sysfs, node_of)
            .iter()
            .filter_map(|node| rotational(node, 0))
            .collect();
        if answers.is_empty() {
            None
        } else {
            Some(answers.into_iter().any(|spinning| spinning))
        }
    }

    /// A mountinfo field as the path it stands for: a space, tab, newline and
    /// backslash in it are written `\040`, `\011`, `\012`, `\134`.
    fn unescaped(field: &str) -> String {
        let bytes = field.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'\\'
                && at + 4 <= bytes.len()
                && bytes[at + 1..at + 4].iter().all(u8::is_ascii_digit)
            {
                if let Ok(value) = u8::from_str_radix(&field[at + 1..at + 4], 8) {
                    out.push(value);
                    at += 4;
                    continue;
                }
            }
            out.push(bytes[at]);
            at += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// sysfs's node of a block device file (`/dev/sda2`); None for anything else
    /// (a network share's or ZFS's name).
    fn block_node(device: &Path, sysfs: &Path) -> Option<PathBuf> {
        let meta = fs::metadata(device).ok()?;
        if !meta.file_type().is_block_device() {
            return None;
        }
        let (major, minor) = device_number(meta.rdev());
        Some(
            sysfs
                .join("dev")
                .join("block")
                .join(format!("{major}:{minor}")),
        )
    }

    /// The path a symlink resolves to, or the path itself when it cannot be
    /// resolved (as Python's realpath: sysfs's nodes are links a disk of the
    /// file system, and a test's are not there at all).
    fn real(path: &Path) -> PathBuf {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// sysfs's nodes of the block devices `path`'s file system was mounted from:
    /// the one its mount names, and for a Btrfs over several disks every one of
    /// them (the mount names one; `/sys/fs/btrfs/<id>/devices` has them all).
    fn mounted_from_with(
        path: &Path,
        mountinfo: &Path,
        sysfs: &Path,
        node_of: &dyn Fn(&Path, &Path) -> Option<PathBuf>,
    ) -> Vec<PathBuf> {
        let Ok(path) = fs::canonicalize(path) else {
            return Vec::new();
        };
        let path = path.to_string_lossy();
        let text = fs::read(mountinfo).unwrap_or_default();
        let mut found: Option<(String, String, String)> = None;
        for line in String::from_utf8_lossy(&text).lines() {
            // (id, parent, device, root, mount point, options... - type, source, options)
            let Some((mount, source)) = line.split_once(" - ") else {
                continue;
            };
            let mount: Vec<&str> = mount.split(' ').collect();
            let source: Vec<&str> = source.split(' ').collect();
            if mount.len() < 5 || source.len() < 2 {
                continue;
            }
            let point = unescaped(mount[4]);
            let inside =
                *path == point || path.starts_with(&format!("{}/", point.trim_end_matches('/')));
            // (the deepest; of two at one point, the later is on top)
            if inside
                && found
                    .as_ref()
                    .is_none_or(|(deepest, _, _)| point.len() >= deepest.len())
            {
                found = Some((point, source[0].to_owned(), unescaped(source[1])));
            }
        }
        let Some((_, kind, source)) = found else {
            return Vec::new();
        };
        let Some(node) = node_of(Path::new(&source), sysfs) else {
            return Vec::new();
        };
        if kind == "btrfs" {
            if let Ok(entries) = fs::read_dir(sysfs.join("fs").join("btrfs")) {
                for entry in entries.flatten() {
                    let devices = entry.path().join("devices");
                    let Ok(names) = fs::read_dir(&devices) else {
                        continue;
                    };
                    let mut nodes: Vec<PathBuf> = names.flatten().map(|e| e.path()).collect();
                    nodes.sort();
                    let node = real(&node);
                    if nodes.iter().any(|other| real(other) == node) {
                        return nodes;
                    }
                }
            }
        }
        vec![node]
    }

    fn rotational(node: &Path, depth: usize) -> Option<bool> {
        let folder = fs::canonicalize(node).ok()?;
        if depth < 4 {
            if let Ok(entries) = fs::read_dir(folder.join("slaves")) {
                let mut names: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
                names.sort();
                let answers: Vec<bool> = names
                    .iter()
                    .filter_map(|name| rotational(name, depth + 1))
                    .collect();
                if !answers.is_empty() {
                    return Some(answers.into_iter().any(|spinning| spinning));
                }
            }
        }
        let mut at = folder;
        loop {
            if let Ok(text) = fs::read_to_string(at.join("queue/rotational")) {
                return Some(text.trim() == "1");
            }
            match at.parent() {
                Some(parent) => at = parent.to_path_buf(),
                None => return None,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::collections::HashMap;

        /// A PC whose system is an SSD (ext4) and whose games are on a Btrfs
        /// mounted at "My Games", as `/proc/self/mountinfo` and sysfs show them.
        struct Pc {
            base: PathBuf,
            sysfs: PathBuf,
            games: PathBuf,
            nodes: HashMap<PathBuf, PathBuf>,
        }

        impl Pc {
            fn new(name: &str) -> Self {
                let base = std::env::temp_dir()
                    .join(format!("hookshot-rust-disk-{}-{name}", std::process::id()));
                let _ = fs::remove_dir_all(&base);
                let sysfs = base.join("sys");
                let games = base.join("My Games");
                fs::create_dir_all(games.join("Hookshot")).unwrap();
                // (no node in it for the folders' own device number)
                fs::create_dir_all(sysfs.join("dev").join("block")).unwrap();
                Self {
                    base,
                    sysfs,
                    games,
                    nodes: HashMap::new(),
                }
            }

            /// A disk and one partition of it in the copy of sysfs;
            /// `/dev/<part>` is its device file.
            fn disk(&mut self, name: &str, rotational: bool, part: &str) {
                let folder = self
                    .sysfs
                    .join("devices")
                    .join("pci")
                    .join("block")
                    .join(name);
                fs::create_dir_all(folder.join(part)).unwrap();
                fs::create_dir_all(folder.join("queue")).unwrap();
                fs::write(
                    folder.join("queue").join("rotational"),
                    if rotational { "1\n" } else { "0\n" },
                )
                .unwrap();
                self.nodes
                    .insert(PathBuf::from(format!("/dev/{part}")), folder.join(part));
            }

            fn node(&self, part: &str) -> PathBuf {
                self.nodes[&PathBuf::from(format!("/dev/{part}"))].clone()
            }

            /// mountinfo with the system's own mount first; a line is
            /// (mount point, type, source).
            fn mounts(&self, lines: &[(&Path, &str, &str)]) -> PathBuf {
                let path = self.base.join("mountinfo");
                let mut text = format!(
                    "30 1 0:30 / {} rw,relatime shared:30 - ext4 /dev/nvme0n1p2 rw\n",
                    self.base.to_string_lossy().replace(' ', "\\040")
                );
                for (point, kind, source) in lines {
                    text.push_str(&format!(
                        "31 1 0:31 / {} rw,relatime shared:31 - {kind} {source} rw\n",
                        point.to_string_lossy().replace(' ', "\\040")
                    ));
                }
                fs::write(&path, text).unwrap();
                path
            }

            fn spinning(&self, folder: &Path, mountinfo: &Path) -> Option<bool> {
                let nodes = &self.nodes;
                // (sysfs has a link a disk of the file system; this PC has no
                // block device files, so the test says what each one maps to)
                let node_of = |device: &Path, _sysfs: &Path| nodes.get(device).cloned();
                spinning_in(folder, mountinfo, &self.sysfs, &node_of)
            }
        }

        impl Drop for Pc {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.base);
            }
        }

        #[test]
        fn a_btrfs_hard_drive_is_found_through_its_mount() {
            let mut pc = Pc::new("btrfs-hard-drive");
            pc.disk("nvme0n1", false, "nvme0n1p2");
            pc.disk("sdb", true, "sdb1");
            let games = pc.games.clone();
            let mountinfo = pc.mounts(&[(&games, "btrfs", "/dev/sdb1")]);
            assert_eq!(pc.spinning(&games.join("Hookshot"), &mountinfo), Some(true));
            // (the mount point itself, its space written \040)
            assert_eq!(pc.spinning(&games, &mountinfo), Some(true));
            // (beside it: the system's SSD)
            assert_eq!(pc.spinning(&pc.base, &mountinfo), Some(false));
        }

        #[test]
        fn the_mount_on_top_is_the_one_read() {
            let mut pc = Pc::new("btrfs-on-top");
            pc.disk("nvme0n1", false, "nvme0n1p2");
            pc.disk("sdb", true, "sdb1");
            let games = pc.games.clone();
            let mountinfo = pc.mounts(&[
                (&games, "btrfs", "/dev/sdb1"),
                (&games, "btrfs", "/dev/nvme0n1p2"),
            ]);
            assert_eq!(
                pc.spinning(&games.join("Hookshot"), &mountinfo),
                Some(false)
            );
        }

        #[test]
        fn a_btrfs_over_two_disks_reads_both() {
            let mut pc = Pc::new("btrfs-two-disks");
            pc.disk("nvme0n1", false, "nvme0n1p2");
            pc.disk("nvme1n1", false, "nvme1n1p1");
            pc.disk("sdb", true, "sdb1");
            let devices = pc
                .sysfs
                .join("fs")
                .join("btrfs")
                .join("0f5c2a1e-6d0b-4c57-9d57-1f6f3a0c9b11")
                .join("devices");
            fs::create_dir_all(&devices).unwrap();
            for part in ["nvme1n1p1", "sdb1"] {
                // (as sysfs has it: a link a disk of the file system, to the partition's folder)
                if std::os::unix::fs::symlink(pc.node(part), devices.join(part)).is_err() {
                    return; // (this PC does not let a test make the symlink sysfs uses)
                }
            }
            let games = pc.games.clone();
            // (the mount names the first disk only)
            let mountinfo = pc.mounts(&[(&games, "btrfs", "/dev/nvme1n1p1")]);
            assert_eq!(pc.spinning(&games.join("Hookshot"), &mountinfo), Some(true));
        }

        #[test]
        fn a_mount_from_no_disk_is_not_guessed() {
            let mut pc = Pc::new("zfs");
            pc.disk("nvme0n1", false, "nvme0n1p2");
            let games = pc.games.clone();
            let mountinfo = pc.mounts(&[(&games, "zfs", "tank/games")]);
            assert_eq!(pc.spinning(&games.join("Hookshot"), &mountinfo), None);
        }

        #[test]
        fn only_a_block_device_file_has_a_node() {
            let pc = Pc::new("block-node");
            let name = pc.base.join("tmpfs");
            fs::write(&name, "").unwrap();
            // (a mount's source is a name as often as a file: "tmpfs", "tank/games")
            assert_eq!(block_node(&name, &pc.sysfs), None);
            assert_eq!(block_node(&pc.base.join("nowhere"), &pc.sysfs), None);
        }

        #[test]
        fn mountinfo_escapes_are_decoded() {
            assert_eq!(unescaped(r"My\040Games"), "My Games");
            assert_eq!(unescaped(r"a\134b"), "a\\b");
            assert_eq!(unescaped("plain"), "plain");
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const OPEN_EXISTING: u32 = 3;
    const FILE_SHARE_READ_WRITE: u32 = 1 | 2;
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D1400;

    #[repr(C)]
    struct Query {
        property_id: u32,
        query_type: u32,
        additional: u32,
    }

    #[repr(C)]
    struct SeekPenalty {
        version: u32,
        size: u32,
        incurs: u8,
    }

    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn DeviceIoControl(
            device: Handle,
            code: u32,
            input: *mut c_void,
            input_size: u32,
            output: *mut c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub fn spinning(path: &Path) -> Option<bool> {
        let root = path
            .to_string_lossy()
            .chars()
            .take_while(|c| *c != '\\' && *c != '/')
            .collect::<String>();
        if root.is_empty() {
            return None;
        }
        let name: Vec<u16> = format!("\\\\.\\{root}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                0,
                FILE_SHARE_READ_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let query = Query {
            property_id: 7,
            query_type: 0,
            additional: 0,
        };
        let mut answer = SeekPenalty {
            version: 0,
            size: 0,
            incurs: 0,
        };
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                (&query as *const Query).cast_mut().cast(),
                std::mem::size_of::<Query>() as u32,
                (&mut answer as *mut SeekPenalty).cast(),
                std::mem::size_of::<SeekPenalty>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        unsafe { CloseHandle(handle) };
        if ok == 0 || returned < std::mem::size_of::<SeekPenalty>() as u32 {
            return None;
        }
        Some(answer.incurs != 0)
    }
}

pub fn spinning_json(path: &str) -> Result<Value, String> {
    let folder = existing_parent(Path::new(path));
    Ok(json!({"path": folder.to_string_lossy(), "spinning": spinning(Path::new(path))}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_an_existing_parent() {
        let folder = existing_parent(Path::new("/definitely/not/here/hookshot"));
        assert!(folder.exists());
        assert!(folder.is_dir());
    }

    #[test]
    fn answers_without_error() {
        let value = spinning_json(".").unwrap();
        assert!(value["spinning"].is_boolean() || value["spinning"].is_null());
    }
}
