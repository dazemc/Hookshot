// The `--stream` command the Flutter builder runs: one JSON request on stdin,
// builder-protocol lines on stdout.

use std::io::Write;
use std::process::{Command, Stdio};

fn stream(request: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hookshot_engine"))
        .arg("--stream")
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
fn other_commands_are_refused_with_a_message() {
    let (code, text) = stream(r#"{"command":"version.describe"}"#);
    assert_eq!(code, 2);
    assert_eq!(
        text,
        "@@error --stream builds folders only, not version.describe\n"
    );
}
