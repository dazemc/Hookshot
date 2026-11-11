//! The Linux launcher's gamescope arguments, from the Flutter options.

use serde_json::{Value, json};

fn number(request: &Value, key: &str, min: i64, max: i64) -> Option<i64> {
    request
        .get(key)
        .and_then(Value::as_i64)
        .filter(|v| *v >= min && *v <= max)
}

pub fn args(exe: &str, request: &Value) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(width) = number(request, "width", 320, 16384) {
        args.push("-W".into());
        args.push(width.to_string());
    }
    if let Some(height) = number(request, "height", 200, 16384) {
        args.push("-H".into());
        args.push(height.to_string());
    }
    if let Some(refresh) = number(request, "refresh", 1, 1000) {
        args.push("-r".into());
        args.push(refresh.to_string());
    }
    if request.get("fullscreen").and_then(Value::as_bool) == Some(true) {
        args.push("-f".into());
    }
    if request.get("hdr").and_then(Value::as_bool) == Some(true) {
        args.push("--hdr-enabled".into());
    }
    if request.get("grab_cursor").and_then(Value::as_bool) == Some(true) {
        args.push("--force-grab-cursor".into());
    }
    if request.get("rt").and_then(Value::as_bool) == Some(true) {
        args.push("--rt".into());
    }
    if let Some(extra) = request.get("extra").and_then(Value::as_str) {
        for word in extra
            .split_whitespace()
            .filter(|w| !w.is_empty() && *w != "--")
        {
            args.push(word.to_owned());
        }
    }
    args.push("--".into());
    args.push(exe.to_owned());
    args
}

pub fn command_json(exe: &str, request: &Value) -> Result<Value, String> {
    Ok(json!({
        "args": args(exe, request),
        "gamemoderun": request.get("gamemoderun").and_then(Value::as_bool).unwrap_or(false),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_style_options_map_to_arguments() {
        let request = json!({
            "width": 3840, "height": 2160, "refresh": 164, "fullscreen": true,
            "hdr": true, "grab_cursor": true, "rt": true, "extra": "--adaptive-sync"
        });
        assert_eq!(
            args("/g/hookshot", &request),
            [
                "-W",
                "3840",
                "-H",
                "2160",
                "-r",
                "164",
                "-f",
                "--hdr-enabled",
                "--force-grab-cursor",
                "--rt",
                "--adaptive-sync",
                "--",
                "/g/hookshot",
            ]
        );
    }
}
