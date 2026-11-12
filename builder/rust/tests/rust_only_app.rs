// The app's path is Rust and Flutter: the Flutter builder starts the Rust engine (and the game), never Python and
// never the legacy make_game.py. Game-free: this reads the app's source; it runs no build and needs no dump.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sources(folder: &str, extension: &str) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    walk(&root().join(folder), extension, &mut files);
    files
}

fn walk(dir: &Path, extension: &str, files: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, extension, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some(extension) {
            let text =
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            files.push((path, text));
        }
    }
}

fn says_python(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("python") || text.contains("make_game") || text.contains("nx2/")
}

#[test]
fn the_flutter_app_starts_the_engine_not_python() {
    let files = sources("builder/flutter/lib", "dart");
    assert!(
        !files.is_empty(),
        "builder/flutter/lib holds no Dart source"
    );
    for (path, text) in files {
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            let starts_process = ["Process.start(", "Process.run(", "Process.runSync("]
                .iter()
                .any(|call| code.contains(call));
            if starts_process {
                assert!(
                    !says_python(code),
                    "{}:{} starts Python from the app: {}",
                    path.display(),
                    number + 1,
                    line.trim()
                );
            }
            // (a literal command, not the error strings that say the fallback is gone)
            assert!(
                !code.contains("'python")
                    && !code.contains("\"python")
                    && !code.contains("'make_game")
                    && !code.contains("\"make_game"),
                "{}:{} names a Python command in the app: {}",
                path.display(),
                number + 1,
                line.trim()
            );
        }
    }
}

#[test]
fn the_rust_engine_does_not_start_python() {
    let files = sources("builder/rust/src", "rs");
    assert!(!files.is_empty(), "builder/rust/src holds no Rust source");
    for (path, text) in files {
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            if code.contains("Command::new(") {
                assert!(
                    !says_python(code),
                    "{}:{} starts Python from the engine: {}",
                    path.display(),
                    number + 1,
                    line.trim()
                );
            }
            assert!(
                !code.contains("Command::new(\"python") && !code.contains("Command::new('python"),
                "{}:{} names Python as a command: {}",
                path.display(),
                number + 1,
                line.trim()
            );
        }
    }
}
