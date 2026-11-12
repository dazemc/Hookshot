// The `--stream` command the Flutter builder runs: one JSON request on stdin,
// builder-protocol lines on stdout.

use std::io::Write;
use std::process::{Command, Stdio};

fn stream(request: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hookshot_engine"))
        .arg("--stream")
        .env_remove("HOOKSHOT_NGX_SDK")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start hookshot_engine --stream");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request.as_bytes())
        .expect("write request");
    let output = child.wait_with_output().expect("wait for the engine");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn a_build_request_that_cannot_run_becomes_an_error_line() {
    let (code, text) = stream(r#"{"command":"build.all"}"#);
    assert_eq!(code, 1);
    assert_eq!(text, "@@error build.all needs an exefs folder\n");
}

#[test]
fn a_dlss_build_without_an_sdk_folder_stops_before_the_game_is_read() {
    let (code, text) = stream(r#"{"command":"build.all","exefs":"/no/such/exefs","dlss":true}"#);
    assert_eq!(code, 1);
    assert!(
        text.starts_with("@@error NVIDIA DLSS needs NVIDIA's SDK folder: "),
        "{text}"
    );
    assert!(!text.contains("@@stage read"), "{text}");
}

#[test]
fn other_commands_are_refused_with_a_message() {
    let (code, text) = stream(r#"{"command":"version.describe"}"#);
    assert_eq!(code, 2);
    assert_eq!(
        text,
        "@@error --stream builds folders and reads .nup packages, not version.describe\n"
    );
}

#[test]
fn a_nup_read_starts_with_the_read_stage_and_stops_without_the_file() {
    let (code, text) = stream(
        r#"{"command":"build.prepare","nup":"/no/such/game.nup","out":"/tmp/out","work":"/tmp/work"}"#,
    );
    assert_eq!(code, 1);
    assert!(
        text.starts_with("@@stage read Reading your game files\n@@progress 0.000\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "@@error Could not access your .nup file /no/such/game.nup: it is not there."
        ),
        "{text}"
    );
    assert!(!text.contains("@@done"), "{text}");
}
