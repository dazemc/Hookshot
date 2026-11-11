//! Hookshot Builder engine, the Rust side of the Flutter app.
//!
//! One JSON entry point keeps the FFI surface tiny:
//! `hookshot_engine_call(request_json)` returns a malloc'd JSON response,
//! released by `hookshot_engine_free`. The same commands are available from
//! the CLI for tests and for development without Flutter.

mod diagnostics;
mod disk;
mod gamescope;
mod kit;
mod nso;
mod nup;
mod profiles;
mod rtgen;
mod stage;
mod version;
mod version_policy;
mod zbic;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde_json::{Value, json};

/// # Safety
/// `request` must be a valid, NUL-terminated UTF-8 C string. The returned
/// pointer is owned by the caller and must be freed with `hookshot_engine_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hookshot_engine_call(request: *const c_char) -> *mut c_char {
    let text = if request.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(request) }
            .to_string_lossy()
            .into_owned()
    };
    let response = dispatch(&text);
    CString::new(response)
        .unwrap_or_else(|_| CString::new("{\"error\":\"response contains NUL\"}").unwrap())
        .into_raw()
}

/// # Safety
/// `response` must be a pointer returned by `hookshot_engine_call` and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hookshot_engine_free(response: *mut c_char) {
    if !response.is_null() {
        drop(unsafe { CString::from_raw(response) });
    }
}

/// Serve one JSON request. Kept public for the CLI and Rust tests.
pub fn dispatch(text: &str) -> String {
    let request: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => return error_json(&format!("invalid request: {error}")),
    };
    let command = request.get("command").and_then(Value::as_str).unwrap_or("");
    let result = match command {
        "profiles.list" => profiles::state(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "selected").as_deref(),
        ),
        "profiles.add" => profiles::add(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "name").as_deref().unwrap_or(""),
        ),
        "profiles.rename" => profiles::rename(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "id").as_deref().unwrap_or(""),
            string(&request, "name").as_deref().unwrap_or(""),
        ),
        "profiles.remove" => profiles::remove(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "id").as_deref().unwrap_or(""),
        ),
        "profiles.export" => profiles::export(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "id").as_deref().unwrap_or(""),
            string(&request, "parent").as_deref().unwrap_or(""),
        ),
        "policy.pinned" => {
            version_policy::pinned_json(&string(&request, "folder").unwrap_or_default())
        }
        "policy.set" => version_policy::set_json(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "tag").as_deref(),
        ),
        "policy.backup" => version_policy::backup_json(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "root").as_deref(),
        ),
        "nup.list" => nup::list_json(&string(&request, "path").unwrap_or_default()),
        "nup.verify" => nup::verify_json(
            &string(&request, "path").unwrap_or_default(),
            string(&request, "title").as_deref(),
            string(&request, "only").as_deref(),
        ),
        "nup.stand_in" => nup::stand_in_json(
            &string(&request, "romfs").unwrap_or_default(),
            string(&request, "heads").as_deref(),
        ),
        "nup.extract" => nup::extract_json(
            &string(&request, "path").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            string(&request, "only").as_deref(),
            string(&request, "title").as_deref(),
        ),
        "gamescope.command" => {
            gamescope::command_json(string(&request, "exe").as_deref().unwrap_or(""), &request)
        }
        "version.describe" => version::describe_json(),
        "build.check" => stage::check_json(
            string(&request, "nup").as_deref(),
            string(&request, "exefs").as_deref(),
            string(&request, "work").as_deref().unwrap_or(""),
        ),
        "build.check_exefs" => nso::check_json(&string(&request, "exefs").unwrap_or_default()),
        "build.kit" => kit::json(),
        "build.spinning" => disk::spinning_json(&string(&request, "path").unwrap_or_default()),
        "build.rtgen" => rtgen::write_json(
            string(&request, "out").as_deref().unwrap_or(""),
            string(&request, "kit").as_deref().unwrap_or(""),
        ),
        "diagnostics" => diagnostics::json(
            &string(&request, "folder").unwrap_or_default(),
            string(&request, "tools").as_deref(),
            string(&request, "builder").as_deref(),
            string(&request, "game").as_deref(),
        ),
        _ => Err(format!("unknown command: {command}")),
    };
    match result {
        Ok(value) => value.to_string(),
        Err(error) => error_json(&error),
    }
}

fn string(request: &Value, key: &str) -> Option<String> {
    request.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn error_json(error: &str) -> String {
    json!({"error": error}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_request_is_json_error() {
        let value: Value = serde_json::from_str(&dispatch("{")).unwrap();
        assert!(value["error"].as_str().unwrap().contains("invalid request"));
    }

    #[test]
    fn unknown_command_is_json_error() {
        let value: Value = serde_json::from_str(&dispatch("{\"command\":\"nope\"}")).unwrap();
        assert_eq!(value["error"], "unknown command: nope");
    }
}
