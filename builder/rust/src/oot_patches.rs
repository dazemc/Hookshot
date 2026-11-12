//! Apply the fork's OoT C patches to a Rust lift. The patch table still lives
//! in nx2/oot_patches.py; this is the temporary bridge until it is ported.
//! For the supported build it is required, as in nx2/chunks.py.

use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

fn python() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("HOOKSHOT_PYTHON") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("HOOKSHOT_PYTHON is not a file: {}", path.display()));
    }
    for name in ["python3", "python"] {
        if Command::new(name)
            .arg("-c")
            .arg("pass")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(PathBuf::from(name));
        }
    }
    Err("the OoT patches need Python (set HOOKSHOT_PYTHON) until they are ported".into())
}

/// Apply `oot_patches.apply` and `apply_output_size` to `gen`.
/// `whole` marks a full supported-build lift: its output-size edits are required.
pub fn apply_json(folder: &str, build: &str, whole: bool) -> Result<Value, String> {
    if std::env::var_os("HOOKSHOT_NO_PATCHES").is_some() {
        return Ok(json!({"applied": false, "reason": "HOOKSHOT_NO_PATCHES"}));
    }
    if !build.starts_with(crate::nso::SUPPORTED_BUILD) {
        return Ok(json!({"applied": false, "reason": "unsupported build"}));
    }
    let root = crate::kit::root();
    let nx2 = root.join("nx2");
    if !nx2.join("oot_patches.py").is_file() {
        return Err(format!(
            "{} is missing",
            nx2.join("oot_patches.py").display()
        ));
    }
    let python = python()?;
    let gen_path = serde_json::to_string(folder).map_err(|e| e.to_string())?;
    let path =
        serde_json::to_string(&nx2.to_string_lossy().to_string()).map_err(|e| e.to_string())?;
    let script = format!(
        "import sys; sys.path.insert(0, {path}); import oot_patches; oot_patches.apply({gen_path}, protect_hearts=True, required={}); oot_patches.apply_output_size({gen_path}, required={})",
        if whole { "True" } else { "False" },
        if whole { "True" } else { "False" }
    );
    let output = Command::new(&python)
        .args(["-I", "-c", &script])
        .output()
        .map_err(|e| format!("{}: {e}", python.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(format!(
            "the OoT patches failed ({}):\n{}{}",
            python.display(),
            stdout
                .chars()
                .rev()
                .take(4000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>(),
            stderr
                .chars()
                .rev()
                .take(4000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
    }
    Ok(json!({
        "applied": true,
        "whole": whole,
        "python": python.to_string_lossy(),
        "output": stdout.lines().collect::<Vec<_>>(),
    }))
}
