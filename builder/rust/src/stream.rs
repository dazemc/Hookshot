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
        "vulkan" => 0.90,
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

/// Run one streaming request. Returns the process exit code: 0 built (or
/// read), 1 stopped, 2 a request this mode cannot take.
pub fn run(request: &str, out: &mut dyn Write) -> i32 {
    let request: Value = match serde_json::from_str(request.trim()) {
        Ok(value) => value,
        Err(error) => {
            let _ = Protocol { out, last: None }.error(&format!("invalid request: {error}"));
            return 2;
        }
    };
    let command = request.get("command").and_then(Value::as_str).unwrap_or("");
    match command {
        "build.all" => run_build(&request, out),
        "build.prepare" => run_prepare(&request, out),
        _ => {
            let what = if command.is_empty() {
                "no command"
            } else {
                command
            };
            let _ = Protocol { out, last: None }.error(&format!(
                "--stream builds folders and reads .nup packages, not {what}"
            ));
            2
        }
    }
}

fn text(request: &Value, key: &str) -> String {
    request
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// `build.all`: lift, compile, link and package an unpacked exefs/romfs pair.
fn run_build(request: &Value, out: &mut dyn Write) -> i32 {
    let exefs = text(request, "exefs");
    let romfs = text(request, "romfs");
    let folder = text(request, "out");
    let work = text(request, "work");
    let assets = text(request, "assets");
    let vk = text(request, "vk");
    let language = text(request, "language");
    let prepare = request
        .get("prepare")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut protocol = Protocol { out, last: None };
    let result = crate::orchestrate::all_json_with(
        &exefs,
        &romfs,
        &folder,
        &work,
        &assets,
        &vk,
        &language,
        prepare,
        &mut |id, text| protocol.stage(id, text),
    );
    match result {
        Ok(value) if value["stage"] == "packaged" => {
            if let Some(message) = value["vk"]["message"].as_str() {
                let _ = protocol.say(message);
            }
            if let Some(prompts) = value["package"]["prompts"].as_object() {
                let text = match prompts.get("reason").and_then(Value::as_str) {
                    Some("unchanged, kept") => "button prompts: unchanged, kept".to_owned(),
                    Some(reason) => format!("button prompts: regenerated ({reason})"),
                    None => String::new(),
                };
                if !text.is_empty() {
                    let _ = protocol.say(&text);
                }
            }
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

/// `build.prepare`: the read stage only, so the Flutter builder can hand the
/// extracted work/exefs and out/romfs to make_game.py's remaining stages.
/// There is no `@@done`: the caller starts the next process.
fn run_prepare(request: &Value, out: &mut dyn Write) -> i32 {
    let nup = text(request, "nup");
    let folder = text(request, "out");
    let work = text(request, "work");
    let mut protocol = Protocol { out, last: None };
    let result = (|| -> Result<(), String> {
        protocol.stage("read", "Reading your game files")?;
        let path = std::path::Path::new(&nup);
        if !path.is_file() {
            return Err(crate::stage::missing_package(path));
        }
        let listed = crate::nup::list_json(&nup)?;
        let titles = listed["titles"].as_array().map(Vec::len).unwrap_or(0);
        if titles == 0 {
            return Err("The package has no titles.".into());
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        protocol.say(&format!(
            "reading {name} ({titles} title{})",
            if titles == 1 { "" } else { "s" }
        ))?;
        // The game's code is small and must hash cleanly before the long
        // extraction; the romfs is checked member by member as it is written.
        let verified = crate::nup::verify_json(&nup, None, Some("exefs"))?;
        let bad = verified["bad"].as_u64().unwrap_or(0);
        if bad > 0 {
            return Err(format!(
                "The package's game code did not verify: {bad} file{} in it {} damaged. Dump the game again and select the complete dump in Hookshot Builder.",
                if bad == 1 { "" } else { "s" },
                if bad == 1 { "is" } else { "are" }
            ));
        }
        let mut last = 0usize;
        let answer =
            crate::stage::prepare_json_with_progress(&nup, &folder, &work, &mut |done, total| {
                if done == total || done.saturating_sub(last) >= 200 {
                    last = done;
                    let _ = protocol.progress(done as f64 / total.max(1) as f64);
                }
            })?;
        if answer["reused"].as_bool() == Some(true) {
            protocol.say("the game was read from this .nup before: reading it again is skipped")?;
        }
        protocol.progress(1.0)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
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
    if let Some(failed) = value["vk"]["compiled"]["failed"]
        .as_array()
        .filter(|failed| !failed.is_empty())
    {
        return compile_message(failed, "Vulkan back end");
    }
    match value["stage"].as_str() {
        Some("host-link") => {
            let output = value["host"]["link"]["output"]
                .as_str()
                .unwrap_or("the linker said nothing");
            format!("The host did not link: {output}")
        }
        Some("vulkan-link") => {
            let output = value["vk"]["link"]["output"]
                .as_str()
                .unwrap_or("the linker said nothing");
            format!("The Vulkan back end did not link: {output}")
        }
        Some("vulkan-symbols") => {
            let count = value["vk"]["undefined"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            let first = value["vk"]["undefined"][0]
                .as_str()
                .unwrap_or("an unnamed symbol");
            format!(
                "The Vulkan back end has {count} unresolved symbol(s), so it would not load; the first: {first}"
            )
        }
        Some("vulkan-compile") => {
            "The build did not compile the Vulkan back end; no game folder was made.".into()
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
            "@@error --stream builds folders and reads .nup packages, not version.describe\n"
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
    fn a_missing_package_names_the_file_before_reading() {
        let mut out = Vec::new();
        assert_eq!(
            run(
                r#"{"command":"build.prepare","nup":"/no/such/game.nup","out":"/tmp/out","work":"/tmp/work"}"#,
                &mut out
            ),
            1
        );
        let text = String::from_utf8(out).unwrap();
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
    fn a_failed_vulkan_backend_says_which_file_or_symbol() {
        let compile = json!({
            "stage": "vulkan-compile",
            "vk": {"compiled": {"failed": [
                {"error": "vk_replay.cpp:9: error: nope"}
            ]}}
        });
        assert_eq!(
            failure_message(&compile),
            "The build did not compile the Vulkan back end: vk_replay.cpp:9: error: nope"
        );
        let link = json!({
            "stage": "vulkan-link",
            "vk": {"link": {"ok": false, "output": "undefined symbol: mkwvk_missing"}}
        });
        assert_eq!(
            failure_message(&link),
            "The Vulkan back end did not link: undefined symbol: mkwvk_missing"
        );
        let symbols = json!({
            "stage": "vulkan-symbols",
            "vk": {"undefined": ["mkwvk_missing"]}
        });
        assert_eq!(
            failure_message(&symbols),
            "The Vulkan back end has 1 unresolved symbol(s), so it would not load; the first: mkwvk_missing"
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
