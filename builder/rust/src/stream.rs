//! The builder protocol on stdout: one JSON request in, the same `@@stage`,
//! `@@progress`, `@@done` and `@@error` lines make_game.py prints out.
//!
//! Flutter runs `hookshot_engine --stream`, writes the request the FFI takes
//! to stdin, and keeps its existing line parser.

use std::io::Write;

use serde_json::Value;

/// Where each stage starts on make_game.py's progress scale.
fn stage_floor(id: &str) -> f64 {
    match id {
        "read" => 0.0,
        "lift" => 0.05,
        "compile" => 0.25,
        "finish" => 0.95,
        _ => 0.0,
    }
}

/// One line of builder output, always a single line even when a compiler
/// reported several.
fn one_line(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 600 {
        format!("{}...", text.chars().take(600).collect::<String>())
    } else {
        text
    }
}

struct Protocol<'a> {
    out: &'a mut dyn Write,
    last: Option<(String, String)>,
}

impl Protocol<'_> {
    fn say(&mut self, text: &str) -> Result<(), String> {
        writeln!(self.out, "{text}")
            .map_err(|error| format!("could not write the build output: {error}"))
    }

    fn stage(&mut self, id: &str, text: &str) -> Result<(), String> {
        let now = (id.to_owned(), text.to_owned());
        if self.last.as_ref() == Some(&now) {
            return Ok(());
        }
        self.last = Some(now);
        self.say(&format!("@@stage {id} {text}"))?;
        self.say(&format!("@@progress {:.3}", stage_floor(id)))
    }

    fn progress(&mut self, fraction: f64) -> Result<(), String> {
        self.say(&format!("@@progress {fraction:.3}"))
    }

    fn done(&mut self, folder: &str) -> Result<(), String> {
        self.progress(1.0)?;
        // A folder path keeps its spacing: the builder opens it as written.
        let folder = folder.replace('\r', " ").replace('\n', " ");
        self.say(&format!("@@done {folder}"))
    }

    fn error(&mut self, message: &str) -> Result<(), String> {
        self.say(&format!("@@error {}", one_line(message)))
    }
}

/// Run one streaming request. Returns the process exit code: 0 built, 1
/// stopped, 2 a request this mode cannot take.
pub fn run(request: &str, out: &mut dyn Write) -> i32 {
    let request: Value = match serde_json::from_str(request.trim()) {
        Ok(value) => value,
        Err(error) => {
            let _ = Protocol { out, last: None }.error(&format!("invalid request: {error}"));
            return 2;
        }
    };
    let command = request.get("command").and_then(Value::as_str).unwrap_or("");
    if command != "build.all" {
        let what = if command.is_empty() {
            "no command"
        } else {
            command
        };
        let _ = Protocol { out, last: None }
            .error(&format!("--stream builds folders only, not {what}"));
        return 2;
    }
    let text = |key: &str| {
        request
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let exefs = text("exefs");
    let romfs = text("romfs");
    let folder = text("out");
    let work = text("work");
    let assets = text("assets");
    let vk = text("vk");
    let language = text("language");
    let mut protocol = Protocol { out, last: None };
    let result = crate::orchestrate::all_json_with(
        &exefs,
        &romfs,
        &folder,
        &work,
        &assets,
        &vk,
        &language,
        &mut |id, text| protocol.stage(id, text),
    );
    match result {
        Ok(value) if value["stage"] == "packaged" => {
            let _ = protocol.done(&folder);
            0
        }
        Ok(value) => {
            let _ = protocol.error(&failure_message(&value));
            1
        }
        Err(error) => {
            let _ = protocol.error(&error);
            1
        }
    }
}

/// A stopped build's sentence, from the stage response the build returned.
fn failure_message(value: &Value) -> String {
    if let Some(failed) = value["driver"]["compiled"]["failed"]
        .as_array()
        .filter(|failed| !failed.is_empty())
    {
        return compile_message(failed, "game");
    }
    if let Some(failed) = value["host"]["compiled"]["failed"]
        .as_array()
        .filter(|failed| !failed.is_empty())
    {
        return compile_message(failed, "host");
    }
    match value["stage"].as_str() {
        Some("host-link") => {
            let output = value["host"]["link"]["output"]
                .as_str()
                .unwrap_or("the linker said nothing");
            format!("The host did not link: {output}")
        }
        Some("lift") => {
            "The build stopped before translating the game to PC code; no game folder was made."
                .into()
        }
        Some(stage) => format!("The build stopped at the {stage} stage."),
        None => "The build stopped without saying why.".into(),
    }
}

fn compile_message(failed: &[Value], what: &str) -> String {
    let first = failed[0]["error"]
        .as_str()
        .unwrap_or("the compiler said nothing");
    if failed.len() == 1 {
        format!("The build did not compile the {what}: {first}")
    } else {
        format!(
            "The build did not compile {} {what} files; the first: {first}",
            failed.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_requests_this_mode_does_not_take() {
        let mut out = Vec::new();
        assert_eq!(run("{", &mut out), 2);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("@@error invalid request: "), "{text}");

        let mut out = Vec::new();
        assert_eq!(run(r#"{"command":"version.describe"}"#, &mut out), 2);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "@@error --stream builds folders only, not version.describe\n"
        );
    }

    #[test]
    fn a_build_error_is_one_line() {
        let mut out = Vec::new();
        assert_eq!(run(r#"{"command":"build.all"}"#, &mut out), 1);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "@@error build.all needs an exefs folder\n"
        );
    }

    #[test]
    fn stages_carry_the_builder_progress_and_are_not_repeated() {
        let mut out = Vec::new();
        let mut protocol = Protocol {
            out: &mut out,
            last: None,
        };
        protocol.stage("read", "Reading your game files").unwrap();
        protocol.stage("read", "Reading your game files").unwrap();
        protocol
            .stage("lift", "Translating the game to PC code")
            .unwrap();
        protocol.done("/games/My  Hookshot").unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "@@stage read Reading your game files\n\
             @@progress 0.000\n\
             @@stage lift Translating the game to PC code\n\
             @@progress 0.050\n\
             @@progress 1.000\n\
             @@done /games/My  Hookshot\n"
        );
    }

    #[test]
    fn failure_keeps_the_first_compiler_error() {
        let value = json!({
            "stage": "compile",
            "driver": {"compiled": {"failed": [
                {"error": "code_001.c:3: error: expected ';'\n  int x"},
                {"error": "code_002.c:9: error: nope"}
            ]}}
        });
        assert_eq!(
            failure_message(&value),
            "The build did not compile 2 game files; the first: code_001.c:3: error: expected ';'\n  int x"
        );
        let mut out = Vec::new();
        let mut protocol = Protocol {
            out: &mut out,
            last: None,
        };
        protocol.error(&failure_message(&value)).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "@@error The build did not compile 2 game files; the first: code_001.c:3: error: expected ';' int x\n"
        );
    }

    #[test]
    fn a_failed_host_link_says_the_linker_output() {
        let value = json!({
            "stage": "host-link",
            "host": {"link": {"ok": false, "output": "undefined symbol: hookshot_missing"}}
        });
        assert_eq!(
            failure_message(&value),
            "The host did not link: undefined symbol: hookshot_missing"
        );
    }
}
