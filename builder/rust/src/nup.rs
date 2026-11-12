//! Read and extract a .nup package: the container half of the build engine.
//! Ported from nx2/nup.py; the layout is documented there.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 4] = b"NUP\x01";
const TOC_MAGIC: &[u8; 4] = b"TOC\x01";
const ZSTD: &[u8; 4] = b"\x28\xb5\x2f\xfd";
/// The list of romfs .zs files whose head is not zstd, written beside the
/// extracted romfs so the build can tell a damaged dump from a game bug.
pub const HEADS_NAME: &str = "romfs_zstd_heads.txt";
pub const HEADS_MAGIC: &str = "hookshot romfs zstd heads 1";

#[derive(Clone, Copy, Debug)]
struct Dir {
    name_off: u32,
    name_len: u32,
    first: u32,
    count: u32,
}

#[derive(Clone, Debug)]
struct Record {
    name_off: u32,
    name_len: u32,
    size: u64,
    data_off: u64,
    stored: u64,
    hash: [u8; 32],
    codec: u8,
    flags: u8,
    reserved: [u8; 12],
}

#[derive(Clone, Copy, Debug)]
struct Title {
    index: u32,
    id: u64,
    root: u32,
    exefs: u32,
    romfs: u32,
    version: u32,
}

pub struct Nup {
    file: File,
    pub file_size: u64,
    physical: u64,
    toc_offset: u64,
    toc_size: u64,
    toc_sha256: [u8; 32],
    blob_offset: u64,
    titles: Vec<Title>,
    dirs: Vec<Dir>,
    files: Vec<Record>,
    kids: Vec<u32>,
    strings: Vec<u8>,
}

impl Nup {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let physical = file.metadata().map_err(|e| e.to_string())?.len();
        let mut head = [0u8; 0x80];
        file.read_exact(&mut head)
            .map_err(|_| "truncated NUP header".to_owned())?;
        if &head[..4] != MAGIC {
            return Err(format!("{}: not a NUP package", path.display()));
        }
        let file_size = u64::from_le_bytes(head[0x40..0x48].try_into().unwrap());
        let toc_offset = u64::from_le_bytes(head[8..16].try_into().unwrap());
        let toc_size = u64::from_le_bytes(head[16..24].try_into().unwrap());
        let mut toc_sha256 = [0u8; 32];
        toc_sha256.copy_from_slice(&head[24..56]);
        let blob_offset = u64::from_le_bytes(head[0x38..0x40].try_into().unwrap());
        if file_size != 0 && physical < file_size {
            return Err(format!(
                "the .nup file is incomplete: it is {} MB, its header says {} MB",
                physical >> 20,
                file_size >> 20
            ));
        }
        if toc_offset != 0x80 {
            return Err(format!("invalid TOC offset {toc_offset:#x}"));
        }
        file.seek(SeekFrom::Start(0x80))
            .map_err(|e| e.to_string())?;
        let mut toc = [0u8; 0x30];
        file.read_exact(&mut toc)
            .map_err(|_| "truncated TOC header".to_owned())?;
        if &toc[..4] != TOC_MAGIC {
            return Err("no TOC at 0x80".into());
        }
        let nt = u32::from_le_bytes(toc[4..8].try_into().unwrap()) as usize;
        let nd = u64::from_le_bytes(toc[8..16].try_into().unwrap()) as usize;
        let nf = u64::from_le_bytes(toc[16..24].try_into().unwrap()) as usize;
        let strings_off = u64::from_le_bytes(toc[24..32].try_into().unwrap()) as usize;
        let strings_size = u64::from_le_bytes(toc[32..40].try_into().unwrap()) as usize;
        let dirs_at = 0x80 + 0x30 + 0x40 * nt;
        let files_at = dirs_at + 24 * nd;
        let kids_at = files_at + 80 * nf;
        let strings_at = 0x80 + strings_off;
        if nt == 0 || strings_at + strings_size > physical as usize {
            return Err("invalid NUP table bounds".into());
        }
        file.seek(SeekFrom::Start(0x80))
            .map_err(|e| e.to_string())?;
        let mut meta = vec![0u8; strings_at + strings_size - 0x80];
        file.read_exact(&mut meta)
            .map_err(|_| "truncated NUP tables".to_owned())?;
        let mut titles = Vec::with_capacity(nt);
        for i in 0..nt {
            let at = 0x30 + 0x40 * i;
            titles.push(Title {
                index: i as u32,
                id: u64::from_le_bytes(meta[at..at + 8].try_into().unwrap()),
                root: u32::from_le_bytes(meta[at + 8..at + 12].try_into().unwrap()),
                exefs: u32::from_le_bytes(meta[at + 12..at + 16].try_into().unwrap()),
                romfs: u32::from_le_bytes(meta[at + 16..at + 20].try_into().unwrap()),
                version: u32::from_le_bytes(meta[at + 20..at + 24].try_into().unwrap()),
            });
        }
        let mut dirs = Vec::with_capacity(nd);
        for i in 0..nd {
            let at = dirs_at - 0x80 + 24 * i;
            dirs.push(Dir {
                name_off: u32::from_le_bytes(meta[at..at + 4].try_into().unwrap()),
                name_len: u32::from_le_bytes(meta[at + 4..at + 8].try_into().unwrap()),
                first: u32::from_le_bytes(meta[at + 8..at + 12].try_into().unwrap()),
                count: u32::from_le_bytes(meta[at + 12..at + 16].try_into().unwrap()),
            });
        }
        let mut files = Vec::with_capacity(nf);
        for i in 0..nf {
            let at = files_at - 0x80 + 80 * i;
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&meta[at + 32..at + 64]);
            let mut reserved = [0u8; 12];
            reserved.copy_from_slice(&meta[at + 66..at + 78]);
            files.push(Record {
                name_off: u32::from_le_bytes(meta[at..at + 4].try_into().unwrap()),
                name_len: u32::from_le_bytes(meta[at + 4..at + 8].try_into().unwrap()),
                size: u64::from_le_bytes(meta[at + 8..at + 16].try_into().unwrap()),
                data_off: u64::from_le_bytes(meta[at + 16..at + 24].try_into().unwrap()),
                stored: u64::from_le_bytes(meta[at + 24..at + 32].try_into().unwrap()),
                hash,
                codec: meta[at + 64],
                flags: meta[at + 65],
                reserved,
            });
        }
        let kids_at = kids_at - 0x80;
        let nk = (strings_at - (kids_at + 0x80)) / 4;
        let mut kids = Vec::with_capacity(nk);
        for i in 0..nk {
            let at = kids_at + 4 * i;
            kids.push(u32::from_le_bytes(meta[at..at + 4].try_into().unwrap()));
        }
        let strings = meta[strings_at - 0x80..].to_vec();
        Ok(Self {
            file,
            file_size,
            physical,
            toc_offset,
            toc_size,
            toc_sha256,
            blob_offset,
            titles,
            dirs,
            files,
            kids,
            strings,
        })
    }

    fn name(&self, off: u32, len: u32) -> Result<String, String> {
        let start = off as usize;
        let end = start + len as usize;
        if end > self.strings.len() {
            return Err("file/directory name outside the string table".into());
        }
        std::str::from_utf8(&self.strings[start..end])
            .map(str::to_owned)
            .map_err(|_| "invalid UTF-8 name".into())
    }

    pub fn titles_json(&self) -> Value {
        json!(
            self.titles
                .iter()
                .map(|t| json!({
                    "index": t.index,
                    "id": format!("{:016x}", t.id),
                    "version": t.version,
                    "exefs": t.exefs == u32::MAX,
                    "romfs": t.romfs == u32::MAX,
                }))
                .collect::<Vec<_>>()
        )
    }

    fn title(&self, want: Option<&str>) -> Result<Title, String> {
        if let Some(want) = want {
            if let Some(title) = self.titles.iter().find(|t| {
                t.index.to_string() == want
                    || t.version.to_string() == want
                    || format!("{:016x}", t.id) == want.to_ascii_lowercase()
            }) {
                return Ok(*title);
            }
            return Err(format!("no title {want}"));
        }
        self.titles
            .iter()
            .copied()
            .max_by_key(|t| t.version)
            .ok_or_else(|| "the package has no titles".into())
    }

    fn walk(&self, root: u32) -> Result<Vec<(String, Record)>, String> {
        if root as usize >= self.dirs.len() {
            return Err("title directory outside the table".into());
        }
        let mut out = Vec::new();
        let mut pending = vec![(root, String::new())];
        let mut seen = HashSet::new();
        while let Some((node, prefix)) = pending.pop() {
            if node < self.dirs.len() as u32 {
                if !seen.insert(node) {
                    return Err("directory cycle in package".into());
                }
                let dir = self.dirs[node as usize];
                if (dir.first + dir.count) as usize > self.kids.len() {
                    return Err("directory child range outside the table".into());
                }
                for child in self.kids[dir.first as usize..(dir.first + dir.count) as usize]
                    .iter()
                    .rev()
                {
                    if *child < self.dirs.len() as u32 {
                        let name = self.name(
                            self.dirs[*child as usize].name_off,
                            self.dirs[*child as usize].name_len,
                        )?;
                        pending.push((*child, format!("{prefix}{name}/")));
                    } else {
                        let record = self
                            .files
                            .get((*child as usize) - self.dirs.len())
                            .ok_or("file index outside the table")?;
                        let name = self.name(record.name_off, record.name_len)?;
                        out.push((format!("{prefix}{name}"), record.clone()));
                    }
                }
            }
        }
        Ok(out)
    }

    fn read_bytes(&mut self, record: &Record, name: &str) -> Result<Vec<u8>, String> {
        self.read_record(record, name, None)
    }

    /// The whole member. `packed` forces the codec: verification honors the
    /// record's explicit codec, extraction uses the legacy heuristics.
    fn read_record(
        &mut self,
        record: &Record,
        name: &str,
        packed: Option<bool>,
    ) -> Result<Vec<u8>, String> {
        if record.data_off > self.physical || record.stored > self.physical - record.data_off {
            return Err(format!("invalid extent for {name}"));
        }
        let packed = match packed {
            Some(packed) => packed,
            None => self.is_packed(record, name)?,
        };
        self.file
            .seek(SeekFrom::Start(record.data_off))
            .map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(record.size as usize);
        if !packed {
            let read = (&mut self.file)
                .take(record.size)
                .read_to_end(&mut out)
                .map_err(|e| format!("{name}: {e}"))?;
            if read as u64 != record.size {
                return Err(format!(
                    "the .nup file ends early (short read at {} MB of {} MB): it is incomplete. Download or copy it again",
                    record.data_off + read as u64 >> 20,
                    self.physical >> 20
                ));
            }
            return Ok(out);
        }
        // Explicit extents must not read the next member; legacy packed
        // records let the frame end the read.
        let limit = if record.codec == 1 {
            record.stored
        } else {
            record.size.saturating_add(1 << 16)
        };
        let limit = limit.min(self.physical - record.data_off);
        let mut decoder = zstd::stream::read::Decoder::new((&mut self.file).take(limit))
            .map_err(|e| format!("{name}: {e}"))?;
        decoder
            .read_to_end(&mut out)
            .map_err(|e| format!("{name}: {e}"))?;
        if out.len() as u64 > record.size {
            return Err(format!(
                "a compressed file expanded past its declared size of {} bytes",
                record.size
            ));
        }
        if out.len() as u64 != record.size {
            return Err(format!(
                "{name} came out as {} bytes, {} expected",
                out.len(),
                record.size
            ));
        }
        Ok(out)
    }

    /// A file the package keeps compressed. A frame that states no size is
    /// tried: it is the file when it unpacks to the listed size. A .zs file is
    /// zstd in the game too, and one that holds exactly as much as it takes is
    /// packed only if what it holds is zstd again.
    fn is_packed(&mut self, record: &Record, name: &str) -> Result<bool, String> {
        if record.codec == 1 {
            return Ok(true);
        }
        if record.data_off > self.physical {
            return Ok(false);
        }
        self.file
            .seek(SeekFrom::Start(record.data_off))
            .map_err(|e| e.to_string())?;
        let mut head = [0u8; 18];
        let mut got = 0;
        while got < head.len() {
            let n = self
                .file
                .read(&mut head[got..])
                .map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            got += n;
        }
        let head = &head[..got];
        if !head.starts_with(ZSTD) {
            return Ok(false);
        }
        let size = content_size(head);
        if name.ends_with(".zs") && (size.is_none() || size == Some(record.size)) {
            return Ok(size == Some(record.size) && self.inner_is_zstd(record)?);
        }
        if let Some(size) = size {
            return Ok(size == record.size);
        }
        // No content size in the frame header: decompress until it either
        // reaches the listed size or passes it.
        self.file
            .seek(SeekFrom::Start(record.data_off))
            .map_err(|e| e.to_string())?;
        let limit = record
            .size
            .saturating_add(1 << 16)
            .min(self.physical - record.data_off);
        let mut decoder = zstd::stream::read::Decoder::new((&mut self.file).take(limit))
            .map_err(|e| format!("{name}: {e}"))?;
        let mut buffer = [0u8; 64 << 10];
        let mut total = 0u64;
        loop {
            match decoder.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    total += n as u64;
                    if total > record.size {
                        return Ok(false);
                    }
                }
                Err(_) => return Ok(false),
            }
        }
        Ok(total == record.size)
    }

    /// Whether what this frame holds is itself a zstd frame.
    fn inner_is_zstd(&mut self, record: &Record) -> Result<bool, String> {
        self.file
            .seek(SeekFrom::Start(record.data_off))
            .map_err(|e| e.to_string())?;
        let limit = record
            .size
            .min(1 << 20)
            .min(self.physical - record.data_off);
        let mut decoder = zstd::stream::read::Decoder::new((&mut self.file).take(limit))
            .map_err(|e| e.to_string())?;
        let mut magic = [0u8; 4];
        match decoder.read_exact(&mut magic) {
            Ok(()) => Ok(magic == *ZSTD),
            Err(_) => Ok(false),
        }
    }

    /// Read-only container checks plus expected .zs headers, not full inner
    /// decoding or authenticity. Ported from nup.py's verify().
    fn verify_toc(&mut self) -> Result<(), String> {
        if self.file_size != self.physical
            || self.toc_offset != 0x80
            || self.toc_size < 0x30
            || self.toc_offset + self.toc_size > self.blob_offset
            || self.blob_offset > self.physical
            || self.blob_offset % 16 != 0
        {
            return Err("invalid declared file/TOC/blob bounds".into());
        }
        self.file
            .seek(SeekFrom::Start(self.toc_offset))
            .map_err(|e| e.to_string())?;
        let mut toc = [0u8; 0x30];
        self.file
            .read_exact(&mut toc)
            .map_err(|_| "truncated TOC header".to_owned())?;
        if &toc[..4] != TOC_MAGIC {
            return Err("invalid TOC header".into());
        }
        let nt = u32::from_le_bytes(toc[4..8].try_into().unwrap()) as u64;
        let nd = u64::from_le_bytes(toc[8..16].try_into().unwrap());
        let nf = u64::from_le_bytes(toc[16..24].try_into().unwrap());
        let soff = u64::from_le_bytes(toc[24..32].try_into().unwrap());
        let ssz = u64::from_le_bytes(toc[32..40].try_into().unwrap());
        let tables_end = 0x30u64
            .saturating_add(64 * nt)
            .saturating_add(24 * nd)
            .saturating_add(80 * nf);
        if nt == 0
            || tables_end > soff
            || (soff - tables_end) % 4 != 0
            || soff.saturating_add(ssz) > self.toc_size
        {
            return Err(
                "parsed title/directory/file/child/string tables exceed declared TOC".into(),
            );
        }
        self.file
            .seek(SeekFrom::Start(self.toc_offset))
            .map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut left = self.toc_size;
        let mut buffer = vec![0u8; 8 << 20];
        while left > 0 {
            let want = left.min(buffer.len() as u64) as usize;
            let n = self
                .file
                .read(&mut buffer[..want])
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("truncated TOC".into());
            }
            digest.update(&buffer[..n]);
            left -= n as u64;
        }
        if digest.finalize().as_slice() != self.toc_sha256 {
            return Err("TOC SHA-256 mismatch".into());
        }
        Ok(())
    }

    fn verify_tree(&self) -> Result<(), String> {
        for dir in &self.dirs {
            if (dir.name_off as usize).saturating_add(dir.name_len as usize) > self.strings.len() {
                return Err("directory name range outside table".into());
            }
            self.name(dir.name_off, dir.name_len)?;
            if (dir.first as usize).saturating_add(dir.count as usize) > self.kids.len() {
                return Err("directory child range outside table".into());
            }
        }
        for record in &self.files {
            if (record.name_off as usize).saturating_add(record.name_len as usize)
                > self.strings.len()
            {
                return Err("file name outside string table".into());
            }
            self.name(record.name_off, record.name_len)?;
        }
        if self
            .kids
            .iter()
            .any(|child| *child as usize >= self.dirs.len() + self.files.len())
        {
            return Err("child index outside directory/file tables".into());
        }
        let mut reached_dirs = HashSet::new();
        let mut reached_files = HashSet::new();
        for title in &self.titles {
            if title.root as usize >= self.dirs.len()
                || [title.exefs, title.romfs]
                    .iter()
                    .any(|dir| *dir != u32::MAX && *dir as usize >= self.dirs.len())
            {
                return Err(format!(
                    "title {} has an invalid directory index",
                    title.index
                ));
            }
            let mut seen = HashSet::new();
            let mut active = HashSet::new();
            let mut pending = vec![(title.root, false)];
            while let Some((directory, leaving)) = pending.pop() {
                if leaving {
                    active.remove(&directory);
                    seen.insert(directory);
                    continue;
                }
                if active.contains(&directory) {
                    return Err(format!("directory cycle in title {}", title.index));
                }
                if seen.contains(&directory) {
                    continue;
                }
                active.insert(directory);
                pending.push((directory, true));
                let dir = self.dirs[directory as usize];
                for child in &self.kids[dir.first as usize..(dir.first + dir.count) as usize] {
                    if (*child as usize) < self.dirs.len() {
                        pending.push((*child, false));
                    } else {
                        reached_files.insert(*child as usize - self.dirs.len());
                    }
                }
            }
            for part in [title.exefs, title.romfs] {
                if part != u32::MAX && !seen.contains(&part) {
                    return Err(format!("title subtree is not reachable from its root"));
                }
            }
            reached_dirs.extend(seen);
        }
        if reached_dirs.len() != self.dirs.len() || reached_files.len() != self.files.len() {
            return Err("orphan directory/file records are not reachable from title roots".into());
        }
        Ok(())
    }

    pub fn verify_json(
        path: &str,
        want: Option<&str>,
        only: Option<&str>,
    ) -> Result<Value, String> {
        let mut nup = Nup::open(path)?;
        nup.verify_toc()?;
        nup.verify_tree()?;
        let titles: Vec<Title> = match want {
            Some(want) => vec![nup.title(Some(want))?],
            None => nup.titles.clone(),
        };
        let mut checked = 0usize;
        let mut bad = 0usize;
        let mut problems = Vec::new();
        let mut cache: HashMap<(u64, u64, u8, u64), Result<(Vec<u8>, [u8; 32]), String>> =
            HashMap::new();
        for title in &titles {
            let parts: Vec<(String, u32)> = match only {
                Some(part) => vec![(
                    part.to_owned(),
                    if part == "exefs" {
                        title.exefs
                    } else {
                        title.romfs
                    },
                )],
                None => vec![(String::new(), title.root)],
            };
            for (part, directory) in parts {
                if directory == u32::MAX {
                    continue;
                }
                for (name, record) in nup.walk(directory)? {
                    checked += 1;
                    let mut reasons = Vec::new();
                    if record.codec > 1 || record.flags != 0 || record.reserved != [0u8; 12] {
                        reasons.push("unsupported codec/flags/reserved bytes".to_owned());
                    }
                    if (record.size == 0
                        && (record.data_off != 0 || record.stored != 0 || record.codec != 0))
                        || (record.size != 0
                            && (record.stored == 0
                                || record.data_off < nup.blob_offset
                                || record.data_off % 16 != 0
                                || record.data_off > nup.physical
                                || record.stored > nup.physical - record.data_off))
                    {
                        reasons.push("invalid extent bounds".to_owned());
                    }
                    if record.codec == 0 && record.stored != record.size {
                        reasons.push("raw extent length differs from member length".to_owned());
                    }
                    if reasons.is_empty() {
                        let key = (record.data_off, record.stored, record.codec, record.size);
                        if !cache.contains_key(&key) {
                            let decoded = nup
                                .read_record(&record, &name, Some(record.codec == 1))
                                .map(|data| {
                                    let digest: [u8; 32] = Sha256::digest(&data).into();
                                    (data, digest)
                                });
                            cache.insert(key, decoded);
                        }
                        match &cache[&key] {
                            Err(error) => reasons.push(format!("member decode failed: {error}")),
                            Ok((data, digest)) => {
                                if data.len() as u64 != record.size {
                                    reasons.push(
                                        "decoded length differs from member length".to_owned(),
                                    );
                                }
                                if digest != &record.hash {
                                    reasons.push("member SHA-256 mismatch".to_owned());
                                }
                                if name.ends_with(".zs") && !frame_ok(data) {
                                    reasons.push(
                                        ".zs member lacks the expected game Zstd header".to_owned(),
                                    );
                                }
                            }
                        }
                    }
                    if !reasons.is_empty() {
                        bad += 1;
                        problems.push(json!({
                            "title": title.index,
                            "id": format!("{:016x}", title.id),
                            "version": title.version,
                            "part": part,
                            "name": name,
                            "reasons": reasons,
                        }));
                    }
                }
            }
        }
        Ok(json!({
            "checked_records": checked,
            "decoded_extents": cache.len(),
            "bad": bad,
            "problems": problems,
        }))
    }

    /// Extract the package, saying how many members of all of them are done.
    /// The heads file of a romfs is written once the whole romfs was walked.
    pub fn extract_with_progress(
        &mut self,
        out: &Path,
        only: Option<&str>,
        want: Option<&str>,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<(usize, u64), String> {
        let title = self.title(want)?;
        let mut parts = Vec::new();
        let mut total = 0usize;
        for (part, dir) in [("exefs", title.exefs), ("romfs", title.romfs)] {
            if only.is_some_and(|only| only != part) || dir == u32::MAX {
                continue;
            }
            let entries = self.walk(dir)?;
            total += entries.len();
            parts.push((part, entries));
        }
        let mut count = 0usize;
        let mut bytes = 0u64;
        let mut done = 0usize;
        let mut where_to = Destination::new(out);
        let mut stale = true;
        for (part, entries) in parts {
            let mut damaged = Vec::new();
            for (rel, record) in entries {
                let path = where_to.path(part, &rel)?;
                let checked = part == "romfs" && rel.ends_with(".zs");
                let expected = if checked && record.hash != [0u8; 32] {
                    Some(record.hash)
                } else {
                    None
                };
                let info = match fs::symlink_metadata(&path) {
                    Ok(meta) => Some(meta),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(format!("{}: {error}", path.display())),
                };
                if info.as_ref().is_some_and(is_link) {
                    return Err(format!(
                        "extraction path is a link or reparse point: {}",
                        path.display()
                    ));
                }
                // (a member cut short by an interrupted run is shorter than its size: written again)
                if info
                    .as_ref()
                    .is_some_and(|meta| meta.is_file() && meta.len() == record.size)
                {
                    if checked {
                        let previous =
                            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                        if !zstd_start(&previous)
                            || expected
                                .is_some_and(|want| Sha256::digest(&previous).as_slice() != want)
                        {
                            damaged.push(rel.clone());
                        }
                    }
                    done += 1;
                    progress(done, total);
                    continue;
                }
                let data = self.read_bytes(&record, &rel)?;
                if checked
                    && (!zstd_start(&data)
                        || expected.is_some_and(|want| Sha256::digest(&data).as_slice() != want))
                {
                    damaged.push(rel.clone());
                }
                // Written in place: a temporary and a rename per member took
                // most of an extraction's time on Windows. An old file is
                // unlinked, not truncated, so another hard link keeps its data.
                if info.is_some() {
                    fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                }
                if stale {
                    // (an interrupted builder that wrote through a temporary left it beside the first member still to write)
                    stale = false;
                    let mut temporary = path.as_os_str().to_owned();
                    temporary.push(".part");
                    let _ = fs::remove_file(PathBuf::from(temporary));
                }
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                if let Err(error) = file.write_all(&data) {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(format!("{}: {error}", path.display()));
                }
                drop(file);
                count += 1;
                bytes += data.len() as u64;
                done += 1;
                progress(done, total);
            }
            if part == "romfs" {
                let heads = out.join(HEADS_NAME);
                let mut temporary = heads.as_os_str().to_owned();
                temporary.push(".part");
                let temporary = PathBuf::from(temporary);
                let mut text = String::from(HEADS_MAGIC);
                text.push('\n');
                for rel in &damaged {
                    text.push_str(rel);
                    text.push('\n');
                }
                fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
                fs::rename(&temporary, &heads).map_err(|e| format!("{}: {e}", heads.display()))?;
            }
        }
        if only != Some("romfs") {
            self.heal(title.index, out)?;
        }
        Ok((count, bytes))
    }

    /// An update's exefs need not hold every module whole: what is not an NSO
    /// there is taken from the package's other titles. Then ZBIC modules are
    /// rewritten as plain NSOs for the tools that follow.
    fn heal(&mut self, title_index: u32, out: &Path) -> Result<(), String> {
        for name in crate::nso::NAMES {
            let path = out.join("exefs").join(name);
            let have = fs::read(&path).unwrap_or_default();
            if have.len() >= 4 && &have[..4] == b"NSO0" {
                continue;
            }
            let mut found = false;
            for other in self.titles.iter().rev().copied().collect::<Vec<_>>() {
                if other.index == title_index || other.exefs == u32::MAX {
                    continue;
                }
                for (rel, record) in self.walk(other.exefs)? {
                    if rel != name {
                        continue;
                    }
                    let data = self.read_bytes(&record, &name)?;
                    if data.starts_with(b"NSO0") {
                        let mut temporary = path.as_os_str().to_owned();
                        temporary.push(".part");
                        let temporary = PathBuf::from(temporary);
                        fs::write(&temporary, &data)
                            .map_err(|e| format!("{}: {e}", temporary.display()))?;
                        fs::rename(&temporary, &path)
                            .map_err(|e| format!("{}: {e}", path.display()))?;
                        found = true;
                    }
                    break;
                }
                if found {
                    break;
                }
            }
        }
        for name in crate::nso::NAMES {
            let path = out.join("exefs").join(name);
            if path.is_file() {
                crate::nso::unpack_nso(&path)?;
            }
        }
        Ok(())
    }
}

/// Whether a file starts as zstd data: a frame, or a skippable frame
/// (magic 0x184D2A50 to 0x184D2A5F).
fn zstd_start(data: &[u8]) -> bool {
    data.len() >= 4
        && (data[..4] == *ZSTD || (data[1..4] == [0x2a, 0x4d, 0x18] && data[0] & 0xf0 == 0x50))
}

/// The game .zs files are ordinary zstd frames: magic plus a plausible frame
/// header. Full decoding is deliberately left to the game.
fn frame_ok(data: &[u8]) -> bool {
    if data.len() < 6 || !data.starts_with(ZSTD) {
        return false;
    }
    let fhd = data[4];
    if fhd & 0x18 != 0 || fhd & 3 == 3 {
        return false;
    }
    let mut at = 5;
    if fhd & 0x20 == 0 {
        at += 1; // window descriptor
    }
    at += [0usize, 1, 2, 4][(fhd & 3) as usize];
    let content = (fhd >> 6) as usize;
    at += if content == 0 {
        usize::from(fhd & 0x20 != 0)
    } else {
        [0usize, 2, 4, 8][content]
    };
    if fhd & 0x04 != 0 {
        at += 1; // frame header checksum
    }
    at <= data.len()
}

fn content_size(head: &[u8]) -> Option<u64> {
    if head.len() < 18 || !head.starts_with(ZSTD) {
        return None;
    }
    let fhd = head[4];
    let single = (fhd >> 5) & 1;
    let at = 5 + usize::from(single == 0) + [0usize, 1, 2, 4][(fhd & 3) as usize];
    let n = [usize::from(single == 0), 2, 4, 8][(fhd >> 6) as usize];
    if n == 0 || at + n > head.len() {
        return None;
    }
    let raw = match n {
        2 => u16::from_le_bytes(head[at..at + 2].try_into().ok()?) as u64 + 256,
        4 => u32::from_le_bytes(head[at..at + 4].try_into().ok()?) as u64,
        8 => u64::from_le_bytes(head[at..at + 8].try_into().ok()?),
        _ => return None,
    };
    Some(raw)
}

/// Whether a folder or member is a link or a Windows reparse point: the
/// extraction must not write through one.
fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0 // (FILE_ATTRIBUTE_REPARSE_POINT)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// The extraction's output, resolved once, with each folder checked or made
/// once: checking every folder on every member's path was most of a Windows
/// extraction's time. A link where a folder should be is refused.
struct Destination {
    root: PathBuf,
    folders: HashSet<PathBuf>,
}

impl Destination {
    fn new(out: &Path) -> Self {
        let root = fs::canonicalize(out).unwrap_or_else(|_| {
            if out.is_absolute() {
                out.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(out))
                    .unwrap_or_else(|_| out.to_path_buf())
            }
        });
        Self {
            root,
            folders: HashSet::new(),
        }
    }

    fn folder(&mut self, path: &Path) -> Result<(), String> {
        if self.folders.contains(path) {
            return Ok(());
        }
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                if is_link(&meta) {
                    return Err(format!(
                        "extraction path is a link or reparse point: {}",
                        path.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
        self.folders.insert(path.to_path_buf());
        Ok(())
    }

    fn path(&mut self, part: &str, rel: &str) -> Result<PathBuf, String> {
        let relative = safe_relative(rel)?;
        let mut at = self.root.clone();
        self.folder(&at)?;
        at.push(part);
        self.folder(&at)?;
        if let Some(parent) = relative.parent() {
            for name in parent.components() {
                at.push(name);
                self.folder(&at)?;
            }
        }
        Ok(at.join(relative.file_name().unwrap_or_default()))
    }

    /// How many folders were checked or made: a check of "each folder once"
    /// (the tests read it).
    #[cfg(test)]
    fn checked(&self) -> usize {
        self.folders.len()
    }
}

fn safe_relative(rel: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') {
        return Err(format!("unsafe path in the package: {rel:?}"));
    }
    for part in rel.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(' ')
            || part.ends_with('.')
        {
            return Err(format!("unsafe path in the package: {rel:?}"));
        }
        if part
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?') || (c as u32) < 32)
        {
            return Err(format!("unsafe path in the package: {rel:?}"));
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "CONIN$" | "CONOUT$" | "PRN" | "AUX" | "NUL"
        ) || (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem[3..].chars().all(|c| c.is_ascii_digit())
        {
            return Err(format!("unsafe path in the package: {rel:?}"));
        }
        path.push(part);
    }
    Ok(path)
}

pub fn list_json(path: &str) -> Result<Value, String> {
    let nup = Nup::open(path)?;
    Ok(json!({"file": path, "size": nup.file_size, "titles": nup.titles_json()}))
}

/// The romfs .zs files that do not start as zstd, with "/" between folders.
/// Whether a .zs file that starts as zstd (`head`: its first 18 bytes) is the
/// .nup's own frame round a damaged file, as builders before #456 wrote them
/// (#2361): a frame whose content is as large as the file, and that starts
/// with zeros or does not decompress. Only those are decompressed, and only
/// their start: a game .zs file's content is larger than the file, and most of
/// them need the game's dictionary to decompress at all.
fn still_in_nup_frame(file: &mut File, head: &[u8], size: u64) -> bool {
    if content_size(head) != Some(size) {
        return false;
    }
    if file.seek(SeekFrom::Start(0)).is_err() {
        return true;
    }
    let Ok(mut decoder) = zstd::stream::read::Decoder::new(&mut *file) else {
        return true;
    };
    let mut start = [0u8; 16];
    match decoder.read(&mut start) {
        Ok(0) => true, // (nothing: the frame says it holds the file, and holds nothing)
        Ok(n) => !start[..n].iter().any(|byte| *byte != 0), // (zeros: what a damaged .bntx looked like)
        Err(_) => true,                                     // (does not decompress)
    }
}

pub fn damaged_files(romfs: &Path) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(romfs).sort_by_file_name() {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "zs") {
            continue;
        }
        let size = entry.metadata().map_err(|e| e.to_string())?.len();
        let mut head = [0u8; 18];
        let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let got = file
            .read(&mut head)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !zstd_start(&head[..got]) || still_in_nup_frame(&mut file, &head[..got], size) {
            let rel = path.strip_prefix(romfs).map_err(|e| e.to_string())?;
            found.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(found)
}

/// The suffix that says a stand-in is the same kind of file: the last
/// extension with an optional `_Nrm`-style part just before it.
fn stand_in_suffix(name: &str) -> String {
    let Some(dot) = name.rfind('.') else {
        return String::new();
    };
    let before = &name[..dot];
    if let Some(underscore) = before.rfind('_') {
        let tag = &before[underscore + 1..];
        if !tag.is_empty()
            && tag.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && tag.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return name[underscore..].to_owned();
        }
    }
    name[dot..].to_owned()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

fn stand_in_for(path: &Path, damaged: &HashSet<PathBuf>) -> Result<Option<PathBuf>, String> {
    let folder = path.parent().ok_or("damaged file has no folder")?;
    let name = path
        .file_name()
        .ok_or("damaged file has no name")?
        .to_string_lossy();
    let suffix = stand_in_suffix(&name);
    let size = fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    let mut best: Option<(usize, u64, PathBuf)> = None;
    for entry in fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let other = entry.file_name().to_string_lossy().into_owned();
        if other == name || !other.ends_with(&suffix) {
            continue;
        }
        let candidate = entry.path();
        if damaged.contains(&candidate) {
            continue;
        }
        let Ok(mut file) = fs::File::open(&candidate) else {
            continue;
        };
        let mut head = [0u8; 4];
        let got = file.read(&mut head).unwrap_or(0);
        if !zstd_start(&head[..got]) {
            continue;
        }
        let other_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let score = (
            common_prefix(&name, &other),
            u64::MAX - size.abs_diff(other_size),
        );
        if best
            .as_ref()
            .is_none_or(|(prefix, closeness, _)| score > (*prefix, *closeness))
        {
            best = Some((score.0, score.1, candidate));
        }
    }
    Ok(best.map(|(_, _, path)| path))
}

/// Replace each damaged romfs .zs file with a whole one of the same kind.
/// `known` lists the paths relative to romfs when the extraction already
/// found them (its heads file), so the ~100,000 files are not opened again.
pub fn stand_in_damaged(
    romfs: &Path,
    known: Option<Vec<String>>,
) -> Result<Vec<(String, Option<String>)>, String> {
    let damaged = match known {
        Some(known) => known,
        None => damaged_files(romfs)?,
    };
    let paths: Vec<PathBuf> = damaged.iter().map(|rel| romfs.join(rel)).collect();
    let excluded: HashSet<PathBuf> = paths.iter().cloned().collect();
    let mut done = Vec::new();
    for (rel, path) in damaged.into_iter().zip(paths) {
        let other = stand_in_for(&path, &excluded)?;
        if let Some(other) = &other {
            let mut temporary = path.as_os_str().to_owned();
            temporary.push(".part");
            let temporary = PathBuf::from(temporary);
            fs::copy(other, &temporary).map_err(|e| format!("{}: {e}", temporary.display()))?;
            fs::rename(&temporary, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let replacement = other.and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        done.push((rel, replacement));
    }
    Ok(done)
}

pub fn stand_in_json(romfs: &str, heads: Option<&str>) -> Result<Value, String> {
    let known = match heads {
        Some(heads) => {
            let text = fs::read_to_string(heads).map_err(|e| format!("{}: {e}", heads))?;
            let mut lines = text.split('\n');
            if lines.next() != Some(HEADS_MAGIC) {
                return Err(format!("{} is not a Hookshot heads list", heads));
            }
            Some(
                lines
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect(),
            )
        }
        None => None,
    };
    let done = stand_in_damaged(Path::new(romfs), known)?;
    let unresolved = done.iter().filter(|(_, other)| other.is_none()).count();
    Ok(json!({
        "damaged": done.len(),
        "unresolved": unresolved,
        "replaced": done.iter().map(|(path, other)| json!({"path": path, "replacement": other})).collect::<Vec<_>>(),
    }))
}

pub fn verify_json(path: &str, want: Option<&str>, only: Option<&str>) -> Result<Value, String> {
    Nup::verify_json(path, want, only)
}

pub fn extract_json(
    path: &str,
    out: &str,
    only: Option<&str>,
    title: Option<&str>,
) -> Result<Value, String> {
    extract_json_with_progress(path, out, only, title, &mut |_, _| {})
}

pub fn extract_json_with_progress(
    path: &str,
    out: &str,
    only: Option<&str>,
    title: Option<&str>,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Value, String> {
    let mut nup = Nup::open(path)?;
    let (files, bytes) = nup.extract_with_progress(Path::new(out), only, title, progress)?;
    Ok(json!({"files": files, "bytes": bytes, "titles": nup.titles_json()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        // (one path a test: the tests run at once)
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        // One title, one directory, one raw file and one zstd file.
        let raw = b"raw file contents";
        let packed = zstd::stream::encode_all(&b"packed file contents"[..], 0).unwrap();
        let names = b"rootmember.binpacked.bin";
        let nt = 1usize;
        let nd = 1usize;
        let nf = 2usize;
        let dirs_at = 0x80 + 0x30 + 0x40 * nt;
        let files_at = dirs_at + 24 * nd;
        let kids_at = files_at + 80 * nf;
        let strings_off = kids_at + 8 - 0x80;
        let strings_size = names.len();
        let data_at = (0x80 + strings_off + strings_size + 15) & !15;
        let packed_at = (data_at + raw.len() + 15) & !15;
        let mut bytes = vec![0u8; data_at];
        bytes[..4].copy_from_slice(MAGIC);
        bytes[8..16].copy_from_slice(&0x80u64.to_le_bytes());
        bytes[16..24].copy_from_slice(&(strings_off as u64).to_le_bytes());
        bytes[0x38..0x40].copy_from_slice(&(data_at as u64).to_le_bytes());
        bytes[0x40..0x48].copy_from_slice(&((packed_at + packed.len()) as u64).to_le_bytes());
        let toc = &mut bytes[0x80..0xb0];
        toc[..4].copy_from_slice(TOC_MAGIC);
        toc[4..8].copy_from_slice(&(nt as u32).to_le_bytes());
        toc[8..16].copy_from_slice(&(nd as u64).to_le_bytes());
        toc[16..24].copy_from_slice(&(nf as u64).to_le_bytes());
        toc[24..32].copy_from_slice(&(strings_off as u64).to_le_bytes());
        toc[32..40].copy_from_slice(&(strings_size as u64).to_le_bytes());
        let title = &mut bytes[0xb0..0xf0];
        title[0..8].copy_from_slice(&0x0400_4860_24de_c000u64.to_le_bytes());
        title[8..12].copy_from_slice(&0u32.to_le_bytes());
        title[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        title[16..20].copy_from_slice(&0u32.to_le_bytes());
        title[20..24].copy_from_slice(&1u32.to_le_bytes());
        // root dir node 0: name len 4, first child 0, count 2
        let dir = &mut bytes[dirs_at..dirs_at + 24];
        dir[4..8].copy_from_slice(&4u32.to_le_bytes());
        dir[8..12].copy_from_slice(&0u32.to_le_bytes());
        dir[12..16].copy_from_slice(&2u32.to_le_bytes());
        let file = &mut bytes[files_at..files_at + 80];
        file[0..4].copy_from_slice(&4u32.to_le_bytes());
        file[4..8].copy_from_slice(&10u32.to_le_bytes());
        file[8..16].copy_from_slice(&(raw.len() as u64).to_le_bytes());
        file[16..24].copy_from_slice(&(data_at as u64).to_le_bytes());
        file[24..32].copy_from_slice(&(raw.len() as u64).to_le_bytes());
        file[32..64].copy_from_slice(&Sha256::digest(raw));
        file[64] = 0;
        let second = &mut bytes[files_at + 80..files_at + 160];
        second[0..4].copy_from_slice(&(4u32 + 10).to_le_bytes());
        second[4..8].copy_from_slice(&10u32.to_le_bytes());
        second[8..16].copy_from_slice(&(b"packed file contents".len() as u64).to_le_bytes());
        second[16..24].copy_from_slice(&(packed_at as u64).to_le_bytes());
        second[24..32].copy_from_slice(&(packed.len() as u64).to_le_bytes());
        second[32..64].copy_from_slice(&Sha256::digest(b"packed file contents"));
        second[64] = 1;
        // child indices: node 0 is the root directory, then file 0 and file 1
        bytes[kids_at..kids_at + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[kids_at + 4..kids_at + 8].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x80 + strings_off..0x80 + strings_off + names.len()].copy_from_slice(names);
        bytes.extend_from_slice(raw);
        bytes.resize(packed_at, 0);
        bytes.extend_from_slice(&packed);
        let toc_size = strings_off + strings_size;
        bytes[16..24].copy_from_slice(&(toc_size as u64).to_le_bytes());
        let digest: [u8; 32] = Sha256::digest(&bytes[0x80..0x80 + toc_size]).into();
        bytes[24..56].copy_from_slice(&digest);
        let path =
            std::env::temp_dir().join(format!("hookshot-rust-nup-{}-{n}.nup", std::process::id()));
        let mut f = File::create(&path).unwrap();
        f.write_all(&bytes).unwrap();
        path
    }

    #[test]
    fn lists_and_extracts_raw_and_zstd_members() {
        let path = fixture();
        let list = list_json(path.to_str().unwrap()).unwrap();
        assert_eq!(list["titles"][0]["version"], 1);
        let out =
            std::env::temp_dir().join(format!("hookshot-rust-nup-out-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        let mut progress = Vec::new();
        let result = extract_json_with_progress(
            path.to_str().unwrap(),
            out.to_str().unwrap(),
            Some("romfs"),
            None,
            &mut |done, total| progress.push((done, total)),
        )
        .unwrap();
        assert_eq!(result["files"], 2);
        assert_eq!(progress.last(), Some(&(2, 2)));
        assert_eq!(
            fs::read(out.join("romfs/member.bin")).unwrap(),
            b"raw file contents"
        );
        assert_eq!(
            fs::read(out.join("romfs/packed.bin")).unwrap(),
            b"packed file contents"
        );
        let verify = verify_json(path.to_str().unwrap(), None, Some("romfs")).unwrap();
        assert_eq!(verify["checked_records"], 2);
        assert_eq!(verify["bad"], 0);
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(out);
    }

    fn extract_into(out: &Path, path: &Path) {
        let mut nup = Nup::open(path).unwrap();
        nup.extract_with_progress(out, Some("romfs"), None, &mut |_, _| {})
            .unwrap();
    }

    fn extract(out: &Path, path: &Path) {
        let _ = fs::remove_dir_all(out);
        extract_into(out, path);
    }

    #[test]
    fn folders_are_checked_once_per_extraction() {
        let root =
            std::env::temp_dir().join(format!("hookshot-rust-nup-folders-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut destination = Destination::new(&root);
        for i in 0..30 {
            let path = destination
                .path("romfs", &format!("Map/{}/file{i}.bin", i % 3))
                .unwrap();
            assert!(path.starts_with(&destination.root));
        }
        // (the output, romfs, Map, then Map/0 to Map/2: each looked at once)
        assert_eq!(destination.checked(), 1 + 1 + 1 + 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_members_in_place_without_a_temporary() {
        let path = fixture();
        let out =
            std::env::temp_dir().join(format!("hookshot-rust-nup-inplace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        extract(&out, &path);
        assert_eq!(
            fs::read(out.join("romfs/member.bin")).unwrap(),
            b"raw file contents"
        );
        assert_eq!(
            fs::read(out.join("romfs/packed.bin")).unwrap(),
            b"packed file contents"
        );
        let leftovers: Vec<String> = walkdir::WalkDir::new(&out)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".part"))
            .map(|entry| entry.path().display().to_string())
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(out);
    }

    #[test]
    fn a_member_cut_short_is_written_again_and_its_stale_part_removed() {
        let path = fixture();
        let out =
            std::env::temp_dir().join(format!("hookshot-rust-nup-short-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        extract(&out, &path);
        let romfs = out.join("romfs");
        fs::write(romfs.join("member.bin"), b"cut").unwrap();
        fs::remove_file(romfs.join("packed.bin")).unwrap();
        fs::write(
            romfs.join("packed.bin.part"),
            b"an older builder's temporary",
        )
        .unwrap();
        extract_into(&out, &path);
        assert_eq!(
            fs::read(romfs.join("member.bin")).unwrap(),
            b"raw file contents"
        );
        assert_eq!(
            fs::read(romfs.join("packed.bin")).unwrap(),
            b"packed file contents"
        );
        assert!(!romfs.join("packed.bin.part").exists());
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(out);
    }

    #[test]
    fn a_member_hard_link_is_replaced_without_truncating_it() {
        let path = fixture();
        let out =
            std::env::temp_dir().join(format!("hookshot-rust-nup-hard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        extract(&out, &path);
        let romfs = out.join("romfs");
        let sentinel = out.join("keep.bin");
        fs::write(&sentinel, b"keep outside").unwrap();
        fs::remove_file(romfs.join("member.bin")).unwrap();
        fs::hard_link(&sentinel, romfs.join("member.bin")).unwrap();
        extract_into(&out, &path);
        assert_eq!(fs::read(&sentinel).unwrap(), b"keep outside");
        assert_eq!(
            fs::read(romfs.join("member.bin")).unwrap(),
            b"raw file contents"
        );
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(out);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_member_or_folder_is_refused() {
        let path = fixture();
        let out =
            std::env::temp_dir().join(format!("hookshot-rust-nup-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        fs::create_dir_all(&out).unwrap();
        let sentinel = out.join("keep.bin");
        fs::write(&sentinel, b"keep outside data").unwrap();
        std::os::unix::fs::symlink(&sentinel, out.join("romfs")).unwrap();
        let mut nup = Nup::open(&path).unwrap();
        let error = nup
            .extract_with_progress(&out, Some("romfs"), None, &mut |_, _| {})
            .unwrap_err();
        assert!(error.contains("link or reparse point"), "{error}");
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(out);
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_temporary_link_cannot_truncate_an_outside_file() {
        let path = fixture();
        let out = std::env::temp_dir().join(format!(
            "hookshot-rust-nup-stale-link-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&out);
        let romfs = out.join("romfs");
        fs::create_dir_all(&romfs).unwrap();
        let sentinel = out.join("keep.bin");
        fs::write(&sentinel, b"keep outside data").unwrap();
        std::os::unix::fs::symlink(&sentinel, romfs.join("packed.bin.part")).unwrap();
        extract_into(&out, &path);
        assert_eq!(fs::read(&sentinel).unwrap(), b"keep outside data");
        assert!(!romfs.join("packed.bin.part").exists());
        assert_eq!(
            fs::read(romfs.join("member.bin")).unwrap(),
            b"raw file contents"
        );
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(out);
    }

    #[test]
    fn finds_and_replaces_damaged_zs_files() {
        let root =
            std::env::temp_dir().join(format!("hookshot-rust-stand-in-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let damaged = zstd::stream::encode_all(&b"good texture"[..], 0).unwrap();
        fs::write(root.join("a_Nrm.zs"), vec![0u8; damaged.len()]).unwrap();
        fs::write(root.join("b_Nrm.zs"), &damaged).unwrap();
        fs::write(root.join("unrelated.bin"), b"not a texture").unwrap();
        // (#2361: still in the .nup's frame, as builders before #456 wrote them: content as large as the file)
        fn wrapped(content: &[u8]) -> Vec<u8> {
            let mut frame = zstd::bulk::compress(content, 3).unwrap();
            assert!(content.len() >= frame.len());
            frame.resize(content.len(), b'x');
            frame
        }
        let zeros = wrapped(&vec![0u8; 1000]);
        let healthy = zstd::bulk::compress(b"BNTX wall", 3).unwrap();
        let mut broken = zeros[..8].to_vec(); // (the header says 1000 bytes; the rest does not decompress)
        broken.resize(1000, 0xff);
        let nonzero = wrapped(&b"BNTX".repeat(250));
        fs::write(root.join("Wall_Gate_M_01_Nrm.zs"), &zeros).unwrap();
        fs::write(root.join("Wall_Gate_M_02_Nrm.zs"), &broken).unwrap();
        fs::write(root.join("Wall_Gate_P_01_Nrm.zs"), &healthy).unwrap();
        fs::write(root.join("Wall_Gate_W_01_Alb.zs"), &nonzero).unwrap();
        assert_eq!(
            damaged_files(&root).unwrap(),
            vec!["Wall_Gate_M_01_Nrm.zs", "Wall_Gate_M_02_Nrm.zs", "a_Nrm.zs"]
        );
        let done = stand_in_damaged(&root, None).unwrap();
        assert_eq!(done.len(), 3);
        assert_eq!(done[0].0, "Wall_Gate_M_01_Nrm.zs");
        assert_eq!(done[0].1.as_deref(), Some("Wall_Gate_P_01_Nrm.zs"));
        assert_eq!(done[1].0, "Wall_Gate_M_02_Nrm.zs");
        assert_eq!(done[1].1.as_deref(), Some("Wall_Gate_P_01_Nrm.zs"));
        assert_eq!(done[2].0, "a_Nrm.zs");
        assert_eq!(done[2].1.as_deref(), Some("b_Nrm.zs"));
        assert_eq!(fs::read(root.join("a_Nrm.zs")).unwrap(), damaged);
        assert_eq!(
            fs::read(root.join("Wall_Gate_M_01_Nrm.zs")).unwrap(),
            healthy
        );
        assert_eq!(
            fs::read(root.join("Wall_Gate_W_01_Alb.zs")).unwrap(),
            nonzero
        );
        let _ = fs::remove_dir_all(root);
    }
}
