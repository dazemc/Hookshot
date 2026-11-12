//! Per-function lifted C cache. Release updates usually leave most guest
//! functions at the same addresses; a content key lets their C come back
//! without translating and emitting them again.
//!
//! Layout (under the lift's `gen` folder):
//!   lift_cache/index.bin  count, then [key 20B][offset u64][len u32]
//!   lift_cache/data.bin   concatenated C texts, append-only
//!
//! A stale or corrupt index is ignored. Data grows by the changed functions
//! only; there is no compaction yet.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"HSLC";
const VERSION: u32 = 1;
const ENTRY_BYTES: usize = 20 + 8 + 4;

pub type Key = [u8; 20];

/// Fill `buf` from `at` without using or moving the file's cursor.
fn read_exact_at(file: &File, mut at: u64, mut buf: &mut [u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        while !buf.is_empty() {
            let read = file.read_at(buf, at)?;
            if read == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            at += read as u64;
            buf = &mut buf[read..];
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let read = file.seek_read(buf, at)?;
            if read == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            at += read as u64;
            buf = &mut buf[read..];
        }
    }
    Ok(())
}

pub struct Cache {
    dir: PathBuf,
    index: HashMap<Key, (u64, u32)>,
    data: File,
}

impl Cache {
    pub fn open(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let index_path = dir.join("index.bin");
        let data_path = dir.join("data.bin");
        let mut data = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&data_path)
            .map_err(|e| format!("{}: {e}", data_path.display()))?;
        let data_len = data.metadata().map(|m| m.len()).unwrap_or(0);
        let mut index = HashMap::new();
        if let Ok(mut file) = File::open(&index_path) {
            let mut magic = [0u8; 4];
            let mut version = [0u8; 4];
            let mut count = [0u8; 4];
            if file.read_exact(&mut magic).is_ok()
                && file.read_exact(&mut version).is_ok()
                && file.read_exact(&mut count).is_ok()
                && &magic == MAGIC
                && u32::from_le_bytes(version) == VERSION
            {
                let count = u32::from_le_bytes(count) as usize;
                for _ in 0..count {
                    let mut raw = [0u8; ENTRY_BYTES];
                    if file.read_exact(&mut raw).is_err() {
                        break;
                    }
                    let mut key = [0u8; 20];
                    key.copy_from_slice(&raw[..20]);
                    let offset = u64::from_le_bytes(raw[20..28].try_into().unwrap());
                    let len = u32::from_le_bytes(raw[28..32].try_into().unwrap());
                    if offset.saturating_add(len as u64) <= data_len {
                        index.insert(key, (offset, len));
                    }
                }
            } else {
                // (an old version: start from a clean data file too)
                let _ = data.set_len(0);
                data.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            index,
            data,
        })
    }

    /// Read a function's C without moving the file's cursor, so several
    /// lookups can run at once on the same open data file.
    pub fn get(&self, key: &Key) -> Option<String> {
        let (offset, len) = *self.index.get(key)?;
        let mut bytes = vec![0u8; len as usize];
        read_exact_at(&self.data, offset, &mut bytes).ok()?;
        Some(match String::from_utf8(bytes) {
            Ok(text) => text,
            // (a corrupt entry keeps the old lossy answer instead of becoming
            // a miss: the next lift rewrites it)
            Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
        })
    }

    pub fn put(&mut self, key: Key, text: &str) -> Result<(), String> {
        let offset = self
            .data
            .seek(SeekFrom::End(0))
            .map_err(|e| e.to_string())?;
        self.data
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        self.index.insert(key, (offset, text.len() as u32));
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        let index_path = self.dir.join("index.bin");
        let temporary = self.dir.join("index.bin.part");
        let mut keys: Vec<&Key> = self.index.keys().collect();
        keys.sort_unstable();
        let mut file =
            File::create(&temporary).map_err(|e| format!("{}: {e}", temporary.display()))?;
        file.write_all(MAGIC).map_err(|e| e.to_string())?;
        file.write_all(&VERSION.to_le_bytes())
            .map_err(|e| e.to_string())?;
        file.write_all(&(keys.len() as u32).to_le_bytes())
            .map_err(|e| e.to_string())?;
        for key in keys {
            let (offset, len) = self.index[key];
            file.write_all(key).map_err(|e| e.to_string())?;
            file.write_all(&offset.to_le_bytes())
                .map_err(|e| e.to_string())?;
            file.write_all(&len.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        file.flush().map_err(|e| e.to_string())?;
        fs::rename(&temporary, &index_path).map_err(|e| format!("{}: {e}", index_path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("hookshot-lift-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let key = [7u8; 20];
        {
            let mut cache = Cache::open(&dir).unwrap();
            cache.put(key, "void f_1(Ctx *c) { return; }\n").unwrap();
            cache.save().unwrap();
        }
        let cache = Cache::open(&dir).unwrap();
        assert_eq!(
            cache.get(&key).as_deref(),
            Some("void f_1(Ctx *c) { return; }\n")
        );
        assert_eq!(cache.get(&[8u8; 20]), None);
        let _ = fs::remove_dir_all(dir);
    }
}
