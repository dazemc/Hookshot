//! ZBIC decompression through the small `hookshot_zbic` shared library.
//! ZBIC is the Switch firmware 22.0.0+ zstd variant used by game modules.
//! It is loaded at runtime because its frame magic replaces standard zstd's:
//! the engine's normal zstd crate must keep decoding packages.

use std::path::PathBuf;
use std::sync::OnceLock;

use libloading::{Library, Symbol};

type Decompress = unsafe extern "C" fn(
    *mut std::os::raw::c_void,
    usize,
    *const std::os::raw::c_void,
    usize,
) -> i64;

fn library_name() -> &'static str {
    if cfg!(windows) {
        "hookshot_zbic.dll"
    } else if cfg!(target_os = "macos") {
        "libhookshot_zbic.dylib"
    } else {
        "libhookshot_zbic.so"
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("HOOKSHOT_ZBIC_LIB") {
        paths.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        let folder = exe.parent().unwrap_or(&exe);
        paths.push(folder.join(library_name()));
        paths.push(folder.join("lib").join(library_name()));
        if let Some(parent) = folder.parent() {
            paths.push(parent.join(library_name()));
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for profile in ["release", "debug"] {
        paths.push(root.join("target").join(profile).join(library_name()));
    }
    paths
}

fn library() -> Result<&'static Library, String> {
    static LIB: OnceLock<Result<Library, String>> = OnceLock::new();
    match LIB.get_or_init(|| {
        let mut last = String::new();
        for path in candidates() {
            match unsafe { Library::new(&path) } {
                Ok(lib) => return Ok(lib),
                Err(error) => last = format!("{}: {error}", path.display()),
            }
        }
        Err(format!(
            "the ZBIC decompressor is missing ({last}); build it with cargo build --release in builder/rust"
        ))
    }) {
        Ok(lib) => Ok(lib),
        Err(error) => Err(error.clone()),
    }
}

/// Decompress one ZBIC frame. `size` is the segment's uncompressed size.
pub fn decompress(data: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let lib = library()?;
    let decompress: Symbol<Decompress> = unsafe { lib.get(b"hookshot_zbic_decompress\0") }
        .map_err(|e| format!("the ZBIC decompressor is not usable: {e}"))?;
    let mut out = vec![0u8; size];
    let written = unsafe {
        decompress(
            out.as_mut_ptr().cast(),
            size,
            data.as_ptr().cast(),
            data.len(),
        )
    };
    if written != size as i64 {
        return Err(format!(
            "ZBIC: decompression failed ({} of {size} bytes)",
            written
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_data_that_is_not_a_frame() {
        assert!(decompress(b"not a zstd frame", 16).is_err());
    }
}
