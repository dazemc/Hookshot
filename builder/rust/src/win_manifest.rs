//! The game's Windows manifest as a .res the exe links in: UTF-8 as the
//! process's code page. Ported from nx2/win_manifest.py.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

pub const MANIFEST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="Hookshot.Game" version="1.0.0.0"/>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
</assembly>
"#;

const RT_MANIFEST: u16 = 24;
const CREATEPROCESS_MANIFEST_RESOURCE_ID: u16 = 1;

fn res_entry(kind: u16, name: u16, data: &[u8], flags: u32, language: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + data.len() + 3);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(flags as u16).to_le_bytes());
    out.extend_from_slice(&language.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    out.resize(out.len() + (-(data.len() as isize) & 3) as usize, 0);
    out
}

pub fn res_bytes() -> Vec<u8> {
    let mut out = res_entry(0, 0, b"", 0, 0);
    out.extend(res_entry(
        RT_MANIFEST,
        CREATEPROCESS_MANIFEST_RESOURCE_ID,
        MANIFEST,
        0x1030,
        0x0409,
    ));
    out
}

pub fn write(path: &Path) -> Result<(), String> {
    let data = res_bytes();
    if fs::read(path).map(|old| old == data).unwrap_or(false) {
        return Ok(());
    }
    fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write_json(path: &str) -> Result<Value, String> {
    write(Path::new(path))?;
    Ok(json!({"out": path, "bytes": res_bytes().len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_utf8_manifest() {
        let path =
            std::env::temp_dir().join(format!("hookshot-rust-manifest-{}.res", std::process::id()));
        let _ = fs::remove_file(&path);
        write(&path).unwrap();
        let data = fs::read(&path).unwrap();
        assert_eq!(data, res_bytes());
        assert!(data.windows(5).any(|window| window == b"UTF-8"));
        write(&path).unwrap();
        let _ = fs::remove_file(path);
    }
}
