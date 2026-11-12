//! suyu's per-instruction AArch64 -> C translator, loaded from the vendored
//! a64-kit shared library. The Rust lifter owns functions and control flow;
//! this is only the instruction text, exactly as a64_lift.py calls it.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::sync::OnceLock;

use libloading::Library;
use serde_json::{Value, json};

type Config = unsafe extern "C" fn(c_int, c_int);
type Batch = unsafe extern "C" fn(
    *const u32,
    *const u64,
    c_int,
    *mut c_char,
    i64,
    *mut i64,
    *mut c_int,
) -> i64;
type RuntimeText = unsafe extern "C" fn(c_int) -> *const c_char;

const UNHANDLED: i32 = 1;
const TERMINATOR: i32 = 2;

fn library_name() -> &'static str {
    if cfg!(windows) {
        "a64emit.dll"
    } else if cfg!(target_os = "macos") {
        "liba64emit.dylib"
    } else {
        "liba64emit.so"
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("HOOKSHOT_A64EMIT_LIB") {
        paths.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        let folder = exe.parent().unwrap_or(&exe);
        paths.push(folder.join(library_name()));
        paths.push(folder.join("lib").join(library_name()));
    }
    let root = crate::kit::root();
    paths.push(root.join("third_party/a64-kit/lift").join(library_name()));
    for profile in ["release", "debug"] {
        paths.push(
            root.join("builder/rust/target")
                .join(profile)
                .join(library_name()),
        );
    }
    paths
}

struct Emitter {
    _library: Library,
    batch: Batch,
    runtime: RuntimeText,
}

fn emitter() -> Result<&'static Emitter, String> {
    static EMITTER: OnceLock<Result<Emitter, String>> = OnceLock::new();
    match EMITTER.get_or_init(|| {
        let mut last = String::new();
        for path in candidates() {
            let Ok(library) = (unsafe { Library::new(&path) }) else {
                last = path.display().to_string();
                continue;
            };
            let result = unsafe {
                let config: Config = *library.get(b"a64_config\0").map_err(|e| e.to_string())?;
                let batch: Batch = *library.get(b"a64_translate_batch\0").map_err(|e| e.to_string())?;
                let runtime: RuntimeText = *library.get(b"a64_runtime_text\0").map_err(|e| e.to_string())?;
                config(1, 1);
                Ok(Emitter { _library: library, batch, runtime })
            };
            return result;
        }
        Err(format!(
            "the a64emit translator is missing (last tried {last}); build it with python -I third_party/a64-kit/lift/build_emit.py"
        ))
    }) {
        Ok(emitter) => Ok(emitter),
        Err(error) => Err(error.clone()),
    }
}

/// Translate instructions in order. Returns each instruction's C and flags
/// (bit 0 unhandled, bit 1 terminator).
pub fn translate(items: &[(u64, u32)]) -> Result<(Vec<String>, Vec<i32>), String> {
    let emitter = emitter()?;
    if items.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let pcs: Vec<u64> = items.iter().map(|(pc, _)| *pc).collect();
    let words: Vec<u32> = items.iter().map(|(_, word)| *word).collect();
    let mut cap = 1usize << 24;
    loop {
        let mut output = vec![0u8; cap];
        let mut offsets = vec![0i64; items.len() + 1];
        let mut flags = vec![0i32; items.len()];
        let need = unsafe {
            (emitter.batch)(
                words.as_ptr(),
                pcs.as_ptr(),
                items.len() as c_int,
                output.as_mut_ptr().cast(),
                cap as i64,
                offsets.as_mut_ptr(),
                flags.as_mut_ptr(),
            )
        };
        if need < 0 {
            return Err("a64emit returned a negative size".into());
        }
        if need as usize > cap {
            cap = (need as f64 * 1.25) as usize;
            continue;
        }
        output.truncate(need as usize);
        let mut texts: Vec<String> = (0..items.len())
            .map(|i| {
                String::from_utf8_lossy(&output[offsets[i] as usize..offsets[i + 1] as usize])
                    .into_owned()
            })
            .collect();
        // (nx2/insn.py: forms the shared translator does not know)
        for (i, (_, word)) in items.iter().enumerate() {
            if let Some(text) = crate::insn::emit(*word) {
                texts[i] = format!("{text}\n");
                flags[i] = 0;
            }
        }
        return Ok((texts, flags));
    }
}

pub fn runtime_text(which: i32) -> Result<String, String> {
    let emitter = emitter()?;
    let pointer = unsafe { (emitter.runtime)(which) };
    if pointer.is_null() {
        return Ok(String::new());
    }
    Ok(unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned())
}

pub fn translate_json(request: &Value) -> Result<Value, String> {
    let items: Vec<(u64, u32)> = request
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let pair = item.as_array()?;
                    Some((pair.first()?.as_u64()?, pair.get(1)?.as_u64()? as u32))
                })
                .collect()
        })
        .unwrap_or_default();
    let (texts, flags) = translate(&items)?;
    Ok(json!({
        "texts": texts,
        "flags": flags,
        "unhandled": flags.iter().filter(|flag| *flag & UNHANDLED != 0).count(),
        "terminators": flags.iter().filter(|flag| *flag & TERMINATOR != 0).count(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_a_nop_and_an_add() {
        let Ok((texts, flags)) = translate(&[(0x1000, 0xD503_201F), (0x1004, 0x9100_0000)]) else {
            return; // the shared translator is not built
        };
        assert_eq!(texts.len(), 2);
        assert_eq!(flags.len(), 2);
        assert!(!texts[0].is_empty());
        assert!(!texts[1].is_empty());
    }
}
