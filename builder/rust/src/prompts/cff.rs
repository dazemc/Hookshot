//! Just enough CFF 1 to read the game's `External` font and write it back
//! with extra Type 2 charstrings: the INDEXes, the DICT operands the font
//! uses, the charset's SIDs, the Private DICT and its local subroutines.
//! Existing charstrings are copied byte for byte; new ones are appended.

#[derive(Debug, Clone)]
pub struct DictEntry {
    pub op: u16,
    pub operands: Vec<f64>,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TopDict {
    pub entries: Vec<DictEntry>,
    pub charset: usize,
    pub charstrings: usize,
    pub private: Option<(usize, usize)>,
    pub font_bbox: [i32; 4],
}

#[derive(Debug, Clone)]
pub struct Index {
    pub objects: Vec<Vec<u8>>,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Cff {
    pub header: [u8; 4],
    pub name_index: Vec<u8>,
    pub top: TopDict,
    pub strings: Index,
    pub global_subrs: Vec<u8>,
    /// One SID per glyph; glyph 0 is not in the charset and has SID 0.
    pub charset: Vec<u16>,
    pub charstrings: Vec<Vec<u8>>,
    pub private: Vec<u8>,
    pub local_subrs: Vec<u8>,
    /// The Subrs operator's value: the local subroutines' offset from the
    /// Private DICT's start.
    pub local_subrs_at: Option<usize>,
}

fn u16_at(data: &[u8], at: usize) -> Result<u16, String> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "CFF: read past the table".to_string())
}

/// An INDEX's header and objects. An empty INDEX is just `count == 0`.
pub fn read_index(data: &[u8], at: usize) -> Result<Index, String> {
    let count = u16_at(data, at)? as usize;
    if count == 0 {
        return Ok(Index {
            objects: Vec::new(),
            raw: data[at..at + 2].to_vec(),
        });
    }
    let off_size = *data
        .get(at + 2)
        .ok_or_else(|| "CFF: read past the table".to_string())? as usize;
    if !(1..=4).contains(&off_size) {
        return Err(format!("CFF: an INDEX offset size of {off_size}"));
    }
    let offsets_at = at + 3;
    let mut offsets = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let p = offsets_at + i * off_size;
        let bytes = data
            .get(p..p + off_size)
            .ok_or_else(|| "CFF: read past an INDEX's offsets".to_string())?;
        let mut value = 0usize;
        for byte in bytes {
            value = (value << 8) | *byte as usize;
        }
        offsets.push(value);
    }
    if offsets[0] != 1 || offsets.windows(2).any(|pair| pair[1] < pair[0]) {
        return Err("CFF: an INDEX's offsets are not ascending from 1".into());
    }
    let data_at = offsets_at + (count + 1) * off_size;
    let end = data_at + offsets[count] - 1;
    if end > data.len() {
        return Err("CFF: an INDEX's data runs past the table".into());
    }
    let objects = (0..count)
        .map(|i| data[data_at + offsets[i] - 1..data_at + offsets[i + 1] - 1].to_vec())
        .collect();
    Ok(Index {
        objects,
        raw: data[at..end].to_vec(),
    })
}

pub fn write_index(objects: &[Vec<u8>]) -> Vec<u8> {
    if objects.is_empty() {
        return vec![0, 0];
    }
    let total = objects.iter().map(Vec::len).sum::<usize>() + 1;
    let off_size = if total <= 0xFF {
        1
    } else if total <= 0xFFFF {
        2
    } else if total <= 0xFF_FFFF {
        3
    } else {
        4
    };
    let mut out = Vec::new();
    out.extend_from_slice(&(objects.len() as u16).to_be_bytes());
    out.push(off_size as u8);
    let mut at = 1u32;
    for object in objects {
        let bytes = at.to_be_bytes();
        out.extend_from_slice(&bytes[4 - off_size..]);
        at += object.len() as u32;
    }
    out.extend_from_slice(&at.to_be_bytes()[4 - off_size..]);
    for object in objects {
        out.extend_from_slice(object);
    }
    out
}

/// The operands and operator of a DICT, with the operand bytes kept so the
/// entries this module does not change are written back as they were.
pub fn parse_dict(data: &[u8]) -> Result<Vec<DictEntry>, String> {
    let mut entries = Vec::new();
    let mut operands = Vec::new();
    let mut raw_start = 0usize;
    let mut at = 0usize;
    while at < data.len() {
        let b0 = data[at];
        if b0 <= 21 {
            let (op, size) = if b0 == 12 {
                let b1 = *data
                    .get(at + 1)
                    .ok_or_else(|| "CFF: a DICT ends in 12".to_string())?;
                (1200 + b1 as u16, 2)
            } else {
                (b0 as u16, 1)
            };
            entries.push(DictEntry {
                op,
                operands: std::mem::take(&mut operands),
                raw: data[raw_start..at].to_vec(),
            });
            at += size;
            raw_start = at;
            continue;
        }
        let (value, size) = number(data, at)?;
        operands.push(value);
        at += size;
    }
    if raw_start != data.len() {
        return Err("CFF: a DICT ends with operands and no operator".into());
    }
    Ok(entries)
}

fn number(data: &[u8], at: usize) -> Result<(f64, usize), String> {
    let b0 = *data
        .get(at)
        .ok_or_else(|| "CFF: read past a DICT".to_string())?;
    match b0 {
        32..=246 => Ok(((b0 as i32 - 139) as f64, 1)),
        247..=250 => {
            let b1 = *data.get(at + 1).ok_or("CFF: a short DICT number")?;
            Ok((((b0 as i32 - 247) * 256 + b1 as i32 + 108) as f64, 2))
        }
        251..=254 => {
            let b1 = *data.get(at + 1).ok_or("CFF: a short DICT number")?;
            Ok(((-(b0 as i32 - 251) * 256 - b1 as i32 - 108) as f64, 2))
        }
        28 => {
            let bytes = data.get(at + 1..at + 3).ok_or("CFF: a shortint")?;
            Ok((i16::from_be_bytes(bytes.try_into().unwrap()) as f64, 3))
        }
        29 => {
            let bytes = data.get(at + 1..at + 5).ok_or("CFF: a longint")?;
            Ok((i32::from_be_bytes(bytes.try_into().unwrap()) as f64, 5))
        }
        30 => {
            let mut value = String::new();
            let mut i = at + 1;
            'digits: loop {
                let byte = *data.get(i).ok_or("CFF: a real that does not end")?;
                i += 1;
                for nibble in [byte >> 4, byte & 0xF] {
                    match nibble {
                        0..=9 => value.push(char::from(b'0' + nibble)),
                        0xA => value.push('.'),
                        0xB => value.push('E'),
                        0xC => value.push_str("E-"),
                        0xD => {}
                        0xE => value.push('-'),
                        0xF => break 'digits,
                        _ => return Err("CFF: a bad real nibble".into()),
                    }
                }
            }
            Ok((value.parse().map_err(|_| "CFF: a bad real")?, i - at))
        }
        255 => {
            let bytes = data.get(at + 1..at + 5).ok_or("CFF: a fixed")?;
            Ok((
                i32::from_be_bytes(bytes.try_into().unwrap()) as f64 / 65536.0,
                5,
            ))
        }
        _ => Err(format!("CFF: reserved DICT byte {b0}")),
    }
}

fn entries_values(entries: &[DictEntry], op: u16) -> Option<Vec<f64>> {
    entries
        .iter()
        .find(|entry| entry.op == op)
        .map(|entry| entry.operands.clone())
}

fn int(value: f64) -> i32 {
    value as i32
}

/// Parse the CFF table of an OpenType font (one font, not a collection).
pub fn parse(data: &[u8]) -> Result<Cff, String> {
    if data.len() < 4 || data[0] != 1 {
        return Err("CFF: not a version 1 CFF table".into());
    }
    let header = data[0..4].try_into().unwrap();
    let header_size = data[2] as usize;
    let name_index = read_index(data, header_size)?;
    let top_index = read_index(data, header_size + name_index.raw.len())?;
    let raw_top = top_index
        .objects
        .first()
        .ok_or_else(|| "CFF: no Top DICT".to_string())?;
    let entries = parse_dict(raw_top)?;
    if entries_values(&entries, 1230).is_some() {
        return Err("CFF: a CID-keyed font is not the game's".into());
    }
    if let Some(values) = entries_values(&entries, 16)
        && values.first().is_some_and(|value| *value > 1.0)
    {
        return Err("CFF: a custom Encoding is not supported".into());
    }
    let charset = entries_values(&entries, 15)
        .and_then(|values| values.first().map(|v| int(*v)))
        .ok_or_else(|| "CFF: no charset offset".to_string())? as usize;
    if charset <= 2 {
        return Err("CFF: a predefined charset is not supported".into());
    }
    let charstrings = entries_values(&entries, 17)
        .and_then(|values| values.first().map(|v| int(*v)))
        .ok_or_else(|| "CFF: no CharStrings offset".to_string())? as usize;
    let private = entries_values(&entries, 18).and_then(|values| {
        (values.len() >= 2).then(|| (int(values[0]) as usize, int(values[1]) as usize))
    });
    let font_bbox = entries_values(&entries, 5)
        .filter(|values| values.len() == 4)
        .map(|values| {
            [
                int(values[0]),
                int(values[1]),
                int(values[2]),
                int(values[3]),
            ]
        })
        .unwrap_or([0, 0, 0, 0]);
    let top = TopDict {
        entries,
        charset,
        charstrings,
        private,
        font_bbox,
    };
    let strings = read_index(
        data,
        header_size + name_index.raw.len() + top_index.raw.len(),
    )?;
    let strings_end = header_size + name_index.raw.len() + top_index.raw.len() + strings.raw.len();
    let global_subrs = read_index(data, strings_end)?;
    let charset = read_charset(
        data,
        top.charset,
        read_index(data, charstrings)?.objects.len(),
    )?;
    let charstrings = read_index(data, top.charstrings)?.objects;
    let (private_bytes, local_subrs, local_subrs_at) = match top.private {
        Some((size, offset)) => {
            let bytes = data
                .get(offset..offset + size)
                .ok_or_else(|| "CFF: the Private DICT runs past the table".to_string())?;
            let dict = parse_dict(bytes).unwrap_or_default();
            let subrs = entries_values(&dict, 19).and_then(|v| v.first().map(|v| int(*v)));
            let local = match subrs {
                Some(at) => {
                    let index = read_index(data, offset + at as usize)?;
                    (index.raw, Some(at as usize))
                }
                None => (Vec::new(), None),
            };
            (bytes.to_vec(), local.0, local.1)
        }
        None => (Vec::new(), Vec::new(), None),
    };
    Ok(Cff {
        header,
        name_index: name_index.raw,
        top,
        strings,
        global_subrs: global_subrs.raw,
        charset,
        charstrings,
        private: private_bytes,
        local_subrs,
        local_subrs_at,
    })
}

fn read_charset(data: &[u8], at: usize, glyphs: usize) -> Result<Vec<u16>, String> {
    let format = *data
        .get(at)
        .ok_or_else(|| "CFF: the charset is past the table".to_string())?;
    if glyphs == 0 {
        return Ok(Vec::new());
    }
    let mut sids = vec![0u16];
    let mut p = at + 1;
    match format {
        0 => {
            for _ in 1..glyphs {
                sids.push(u16_at(data, p)?);
                p += 2;
            }
        }
        1 | 2 => {
            while sids.len() < glyphs {
                let first = u16_at(data, p)?;
                p += 2;
                let left = if format == 1 {
                    let value = *data
                        .get(p)
                        .ok_or_else(|| "CFF: the charset is past the table".to_string())?;
                    p += 1;
                    value as usize
                } else {
                    let value = u16_at(data, p)?;
                    p += 2;
                    value as usize
                };
                for i in 0..=left {
                    if sids.len() >= glyphs {
                        break;
                    }
                    sids.push(first.wrapping_add(i as u16));
                }
            }
        }
        other => return Err(format!("CFF: charset format {other}")),
    }
    Ok(sids)
}

impl Cff {
    /// Append a glyph: its charstring and its name (as a new String INDEX
    /// entry). The SID is returned, as the new charset needs it.
    pub fn add_glyph(&mut self, name: &str, charstring: Vec<u8>) -> u16 {
        let sid = match self
            .strings
            .objects
            .iter()
            .position(|string| string == name.as_bytes())
        {
            Some(index) => 391 + index as u16,
            None => {
                self.strings.objects.push(name.as_bytes().to_vec());
                391 + (self.strings.objects.len() - 1) as u16
            }
        };
        self.charset.push(sid);
        self.charstrings.push(charstring);
        sid
    }

    /// Write the CFF table back: the original structures in a canonical
    /// layout, the appended charstrings and charset entries included.
    pub fn write(&self) -> Result<Vec<u8>, String> {
        let charset = {
            let mut bytes = vec![0u8];
            for sid in self.charset.iter().skip(1) {
                bytes.extend_from_slice(&sid.to_be_bytes());
            }
            bytes
        };
        let charstrings = write_index(&self.charstrings);
        let strings = write_index(&self.strings.objects);
        let subrs_at = self.local_subrs_at.unwrap_or(self.private.len());
        if subrs_at < self.private.len() {
            return Err("CFF: the local subroutines overlap the Private DICT".into());
        }
        let local_subrs_at = self.private.len().max(subrs_at);

        // The Top DICT's offsets are written as longint operands, so the
        // dictionary's length does not change when they do.
        let mut top_entries = self.top.entries.clone();
        for entry in &mut top_entries {
            match entry.op {
                5 => entry.raw = longints(&[0, 0, 0, 0]),
                15 | 17 => entry.raw = longints(&[0]),
                18 => entry.raw = longints(&[0, 0]),
                _ => {}
            }
        }
        let top_len = dict_len(&top_entries);
        // (the Top DICT INDEX: count, offSize, two offsets, object)
        let top_index_len = 2 + 1 + 2 * 4 + top_len;
        let data_at =
            4 + self.name_index.len() + top_index_len + strings.len() + self.global_subrs.len();
        let charset_at = data_at;
        let charstrings_at = charset_at + charset.len();
        let private_at = charstrings_at + charstrings.len();
        let subrs_at = private_at + local_subrs_at;

        let set = |entries: &mut Vec<DictEntry>, op: u16, raw: Vec<u8>| {
            if let Some(entry) = entries.iter_mut().find(|entry| entry.op == op) {
                entry.raw = raw;
            } else {
                entries.push(DictEntry {
                    op,
                    operands: Vec::new(),
                    raw,
                });
            }
        };
        set(&mut top_entries, 15, longints(&[charset_at as i32]));
        set(&mut top_entries, 17, longints(&[charstrings_at as i32]));
        if !self.private.is_empty() {
            set(
                &mut top_entries,
                18,
                longints(&[self.private.len() as i32, private_at as i32]),
            );
        }
        set(&mut top_entries, 5, longints(&self.top.font_bbox));
        let top_dict = write_dict(&top_entries);
        if top_dict.len() != top_len {
            return Err("CFF: the Top DICT changed length while its offsets were set".into());
        }
        let mut top_index = Vec::new();
        top_index.extend_from_slice(&1u16.to_be_bytes());
        top_index.push(4);
        top_index.extend_from_slice(&1u32.to_be_bytes());
        top_index.extend_from_slice(&(top_dict.len() as u32 + 1).to_be_bytes());
        top_index.extend_from_slice(&top_dict);
        if top_index.len() != top_index_len {
            return Err("CFF: the Top DICT INDEX changed length".into());
        }

        let mut out = Vec::with_capacity(subrs_at + self.local_subrs.len());
        out.extend_from_slice(&self.header);
        out.extend_from_slice(&self.name_index);
        out.extend_from_slice(&top_index);
        out.extend_from_slice(&strings);
        out.extend_from_slice(&self.global_subrs);
        out.extend_from_slice(&charset);
        out.extend_from_slice(&charstrings);
        out.extend_from_slice(&self.private);
        out.resize(private_at + local_subrs_at, 0);
        out.extend_from_slice(&self.local_subrs);
        Ok(out)
    }
}

fn longints(values: &[i32]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(values.len() * 5);
    for value in values {
        raw.push(29);
        raw.extend_from_slice(&value.to_be_bytes());
    }
    raw
}

fn write_dict(entries: &[DictEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        out.extend_from_slice(&entry.raw);
        if entry.op >= 1200 {
            out.push(12);
            out.push((entry.op - 1200) as u8);
        } else {
            out.push(entry.op as u8);
        }
    }
    out
}

fn dict_len(entries: &[DictEntry]) -> usize {
    entries
        .iter()
        .map(|entry| entry.raw.len() + if entry.op >= 1200 { 2 } else { 1 })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indexed(objects: &[&[u8]]) -> Vec<u8> {
        write_index(&objects.iter().map(|o| o.to_vec()).collect::<Vec<_>>())
    }

    #[test]
    fn an_index_reads_what_it_writes() {
        for objects in [
            vec![b"one".as_slice()],
            vec![b"".as_slice(), b"two".as_slice(), b"three".as_slice()],
        ] {
            let bytes = indexed(&objects);
            let index = read_index(&bytes, 0).unwrap();
            assert_eq!(index.objects, objects);
            assert_eq!(index.raw, bytes);
        }
    }

    #[test]
    fn an_empty_index_is_two_zero_bytes() {
        assert_eq!(write_index(&[]), [0, 0]);
        assert!(read_index(&[0, 0], 0).unwrap().objects.is_empty());
    }

    #[test]
    fn a_dict_keeps_its_operand_bytes() {
        let dict = [139u8, 29, 0, 0, 0, 5, 12, 30];
        let entries = parse_dict(&dict).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, 1230);
        assert_eq!(entries[0].operands, [0.0, 5.0]);
        assert_eq!(write_dict(&entries), dict);
    }

    #[test]
    fn offsets_start_at_one() {
        let index = read_index(&indexed(&[b"ab", b"c"]), 0).unwrap();
        assert_eq!(index.objects[0], b"ab");
        assert_eq!(index.objects[1], b"c");
    }
}

#[cfg(test)]
mod large_index_test {
    use super::*;
    #[test]
    fn a_large_index_keeps_its_objects() {
        let objects = (0..334).map(|i| vec![b'a'; 3 + i % 5]).collect::<Vec<_>>();
        let bytes = write_index(&objects);
        let index = read_index(&bytes, 0).unwrap();
        assert_eq!(index.objects, objects);
    }
}
