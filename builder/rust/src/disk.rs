//! Is a folder on a spinning disk? A build reads and writes tens of
//! gigabytes, and the answer is only ever a warning. Ported from nx2/disk.py.

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
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    pub fn spinning(path: &Path) -> Option<bool> {
        let device = fs::metadata(path).ok()?.dev();
        let major = ((device >> 8) & 0xfff) | ((device >> 32) & !0xfff);
        let minor = (device & 0xff) | ((device >> 12) & !0xff);
        rotational(&PathBuf::from(format!("/sys/dev/block/{major}:{minor}")), 0)
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
