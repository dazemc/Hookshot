//! The finish work after packaging: the game's shader list (in the driver) and
//! a run of the built game with --prepare-shaders so its first start is not
//! behind a black window. No Python is called from here.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Run the built game with --prepare-shaders, in its folder and without the
/// builder's own environment (as make_game.py's game_env). Exit 3 (no usable
/// Vulkan) and 4 (the driver stopped responding) are reported, not fatal; a
/// run that prints nothing for 15 minutes is stopped. The first start then
/// does the work, exactly as when Python skips it.
pub fn prepare_shaders(exe: &Path, out: &Path) -> Value {
    if !exe.is_file() {
        return json!({"prepared": false, "reason": "the game executable is missing"});
    }
    let mut command = Command::new(exe);
    command
        .arg("--prepare-shaders")
        .current_dir(out)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, _) in std::env::vars() {
        if key.starts_with("HOOKSHOT_")
            || key.starts_with("PYTHON")
            || matches!(
                key.as_str(),
                "APPDIR" | "APPIMAGE" | "ARGV0" | "OWD" | "LD_LIBRARY_PATH"
            )
        {
            command.env_remove(key);
        }
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return json!({"prepared": false, "reason": error.to_string()}),
    };
    let (sender, receiver) = mpsc::channel::<Instant>();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    std::thread::scope(|scope| {
        if let Some(stream) = stdout {
            let sender = sender.clone();
            scope.spawn(move || forward(stream, sender));
        }
        if let Some(stream) = stderr {
            let sender = sender.clone();
            scope.spawn(move || forward(stream, sender));
        }
    });
    let limit = Duration::from_secs(15 * 60);
    let mut last = Instant::now();
    let status = loop {
        if let Some(done) = child.try_wait().ok().flatten() {
            break Some(done);
        }
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(at) => last = at,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last.elapsed() > limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    return json!({"prepared": false, "stopped": true, "reason": "the game printed nothing for 15 minutes"});
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break child.wait().ok(),
        }
    };
    let code = status.and_then(|status| status.code()).unwrap_or(1);
    let reason = match code {
        0 => String::new(),
        3 => "no usable Vulkan here".into(),
        4 => "the GPU driver stopped responding".into(),
        other => format!("the game stopped with exit status {other}"),
    };
    json!({"prepared": code == 0, "exit": code, "reason": reason})
}

fn forward(stream: impl std::io::Read, sender: mpsc::Sender<Instant>) {
    let reader = BufReader::new(stream);
    for line in reader.lines().map_while(Result::ok) {
        // (the game's own @@done/@@error would end or fail the builder's run:
        // Python's show_prepared prints those as plain text)
        // (stderr: the JSON entry's stdout is its response; the stream CLI
        // reads its protocol from stdout and shows this as the build log)
        if line.starts_with("@@done") || line.starts_with("@@error") {
            eprintln!("{}", &line[2..]);
        } else {
            eprintln!("{line}");
        }
        let _ = sender.send(Instant::now());
    }
}

/// The finish side after packaging: the shader preparation run.
pub fn finish(_root: &Path, _romfs: &Path, out: &Path, exe: &Path) -> Value {
    prepare_shaders(exe, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_executable_is_not_fatal() {
        let value = prepare_shaders(Path::new("/nonexistent/hookshot"), Path::new("/tmp"));
        assert_eq!(value["prepared"], false);
        assert!(
            value["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("missing"))
        );
    }
}
