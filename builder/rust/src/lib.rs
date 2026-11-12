//! Hookshot Builder engine, the Rust side of the Flutter app.
//!
//! One JSON entry point keeps the FFI surface tiny:
//! `hookshot_engine_call(request_json)` returns a malloc'd JSON response,
//! released by `hookshot_engine_free`. The same commands are available from
//! the CLI for tests and for development without Flutter; `--stream` turns a
//! build request into the builder's `@@` progress lines.

mod a64;
mod chunks;
mod compile;
mod diagnostics;
mod disk;
mod driver;
mod emitter;
mod fpx_headers;
mod gamescope;
mod host;
mod host_build;
mod image;
mod insn;
mod kit;
mod lift;
mod lift_cache;
mod lifted_c;
mod linker;
mod logo_rgba;
mod lz4;
mod nso;
mod nso_image;
mod nup;
mod oot_patches;
mod orchestrate;
mod package;
mod platform;
mod poll;
mod profiles;
mod rtgen;
mod shader;
mod split_hot;
mod stage;
mod standalone;
mod stream;
mod text;
mod toolchain;
mod ui_fonts;
mod version;
mod version_policy;
mod win_manifest;
mod zbic;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde_json::{Value, json};

pub use stream::run as run_stream;

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
        "build.host" => Ok(host::json()),
        "build.all" => orchestrate::all_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "romfs").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            &string(&request, "work").unwrap_or_default(),
            &string(&request, "assets").unwrap_or_default(),
            &string(&request, "vk").unwrap_or_default(),
            &string(&request, "language").unwrap_or_default(),
        ),
        "build.package" => package::package_json(
            &string(&request, "work").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            &string(&request, "romfs").unwrap_or_default(),
            &string(&request, "vk").unwrap_or_default(),
            &string(&request, "assets").unwrap_or_default(),
            &string(&request, "language").unwrap_or_default(),
        ),
        "build.host_link" => host_build::link_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "work").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
        )
        .map_err(String::from),
        "build.cpu" => host::check_json(),
        "build.jobs" => {
            let given = request
                .get("jobs")
                .and_then(Value::as_u64)
                .map(|jobs| jobs as usize);
            let environment = string(&request, "hookshot_jobs");
            host::chosen_jobs(given, environment.as_deref())
                .map(|jobs| json!({"jobs": jobs, "cpus": host::cpus()}))
        }
        "build.prepare_dump" => stage::prepare_dump_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "romfs").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            &string(&request, "work").unwrap_or_default(),
        ),
        "build.prepare" => stage::prepare_json(
            &string(&request, "nup").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            &string(&request, "work").unwrap_or_default(),
        ),
        "build.compile_one" => compile::compile_one_json(&request, &kit::root()),
        "build.compile_many" => compile::many_json(&request, &kit::root()),
        "build.classify" => {
            let word = request.get("word").and_then(Value::as_u64).unwrap_or(0) as u32;
            let pc = request.get("pc").and_then(Value::as_u64).unwrap_or(0);
            Ok(
                json!({"kind": format!("{:?}", a64::classify(word, pc)), "pc_relative": a64::pc_relative(word)}),
            )
        }
        "build.plan" => compile::plan_json(&request, &kit::root()),
        "lift.translate" => emitter::translate_json(&request),
        "lift.tables" => lift::tables_json(&string(&request, "exefs").unwrap_or_default()),
        "lift.fpx_header" => lift::fpx_json(&string(&request, "dir").unwrap_or_default()),
        "lift.write" => lift::write_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            request.get("limit").and_then(Value::as_u64).unwrap_or(0) as usize,
            request
                .get("chunk_bytes")
                .and_then(Value::as_u64)
                .unwrap_or(24 << 20) as usize,
            request
                .get("standalone")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        "lift.emit" => lift::emit_json(
            &string(&request, "exefs").unwrap_or_default(),
            u64::from_str_radix(&string(&request, "entry").unwrap_or_default(), 16).unwrap_or(0),
        ),
        "lift.coverage" => {
            let pcs: Vec<u64> = request
                .get("pcs")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|pc| u64::from_str_radix(pc, 16).ok())
                        .collect()
                })
                .unwrap_or_default();
            lift::coverage_json(&string(&request, "exefs").unwrap_or_default(), &pcs)
        }
        "lift.debug" => lift::debug_json(
            &string(&request, "exefs").unwrap_or_default(),
            u64::from_str_radix(&string(&request, "br").unwrap_or_default(), 16).unwrap_or(0),
            request.get("reg").and_then(Value::as_u64).unwrap_or(0) as u8,
            u64::from_str_radix(&string(&request, "table").unwrap_or_default(), 16).unwrap_or(0),
        ),
        "lift.flags" => {
            let text = string(&request, "text").unwrap_or_default();
            let (read, written) = text::flag_masks(&text);
            let dead = request.get("dead").and_then(Value::as_u64).unwrap_or(0) as u8;
            Ok(
                json!({"reads": read, "writes": written, "dropped": text::drop_flag_stores(&text, dead)}),
            )
        }
        "lift.marks" => {
            Ok(json!({"text": text::mark_groups(&string(&request, "text").unwrap_or_default())}))
        }
        "lift.vfcmp" => {
            Ok(json!({"text": text::vectorize_fcmp(&string(&request, "text").unwrap_or_default())}))
        }
        "lift.fpx" => {
            Ok(json!({"text": text::outline_fpx(&string(&request, "text").unwrap_or_default())}))
        }
        "lift.vset" => {
            Ok(json!({"text": text::widen_vstores(&string(&request, "text").unwrap_or_default())}))
        }
        "lift.reg_writes" => {
            Ok(json!({"mask": text::reg_writes(&string(&request, "text").unwrap_or_default())}))
        }
        "lift.stack" => {
            Ok(json!({"stores": text::stack_stores(&string(&request, "text").unwrap_or_default())}))
        }
        "lift.frame" => Ok(text::frame_json(
            &string(&request, "text").unwrap_or_default(),
        )),
        "lift.runtime" => {
            emitter::runtime_text(request.get("which").and_then(Value::as_i64).unwrap_or(0) as i32)
                .map(|text| json!({"text": text}))
        }
        "build.eta" => {
            let pending: Vec<f64> = request
                .get("pending")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default();
            Ok(
                json!({"seconds": compile::eta_left(&pending, request.get("jobs").and_then(Value::as_u64).unwrap_or(1) as usize)}),
            )
        }
        "build.split_big" => {
            let files: Vec<String> = request
                .get("files")
                .and_then(Value::as_array)
                .map(|files| {
                    files
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            chunks::split_big_json(
                &string(&request, "gen").unwrap_or_default(),
                &files,
                request.get("budget").and_then(Value::as_u64).unwrap_or(0),
            )
        }
        "build.toolchain" => Ok(toolchain::json()),
        "build.link_check" => {
            linker::check_symbols_json(&string(&request, "file").unwrap_or_default())
        }
        "build.link" => linker::link_json(&request),
        "build.seed_pipelines" => stage::seed_pipelines_json(
            &string(&request, "root").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
        ),
        "build.shaders" => shader::census_json(
            &string(&request, "romfs").unwrap_or_default(),
            string(&request, "out").as_deref(),
        ),
        "build.driver" => driver::plan_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "romfs").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            &string(&request, "work").unwrap_or_default(),
            request
                .get("lift_limit")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            request
                .get("lift_all")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            request
                .get("standalone")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            request
                .get("compile")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        "build.check" => stage::check_json(
            string(&request, "nup").as_deref(),
            string(&request, "exefs").as_deref(),
            string(&request, "work").as_deref().unwrap_or(""),
        ),
        "build.check_exefs" => nso::check_json(&string(&request, "exefs").unwrap_or_default()),
        "nso.load" => nso_image::json(&string(&request, "exefs").unwrap_or_default()),
        "image.adr" => {
            let hle: Vec<String> = request
                .get("hle")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let skip: Vec<String> = request
                .get("skip")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            image::adr_json(
                &string(&request, "exefs").unwrap_or_default(),
                request.get("aslr").and_then(Value::as_u64).unwrap_or(0),
                &hle,
                &skip,
            )
        }
        "image.build" => {
            let hle: Vec<String> = request
                .get("hle")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let skip: Vec<String> = request
                .get("skip")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            image::build_json(
                &string(&request, "exefs").unwrap_or_default(),
                &string(&request, "out").unwrap_or_default(),
                request.get("aslr").and_then(Value::as_u64).unwrap_or(0),
                request
                    .get("program_args")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                &hle,
                &skip,
            )
        }
        "nso.dump" => nso_image::dump_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
        ),
        "build.kit" => kit::json(),
        "build.ui_fonts" => ui_fonts::write_json(&string(&request, "out").unwrap_or_default()),
        "build.image" => standalone::image_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "out").unwrap_or_default(),
            &string(&request, "host").unwrap_or_default(),
        ),
        "build.standalone" => standalone::write_json(
            &string(&request, "exefs").unwrap_or_default(),
            &string(&request, "gen").unwrap_or_default(),
            &string(&request, "host").unwrap_or_default(),
        ),
        "build.win_manifest" => {
            win_manifest::write_json(&string(&request, "out").unwrap_or_default())
        }
        "build.logo" => logo_rgba::write_json(
            &string(&request, "out").unwrap_or_default(),
            string(&request, "png").as_deref(),
        ),
        "build.polls" => {
            let text = string(&request, "text").unwrap_or_default();
            Ok(poll::json(&text))
        }
        "build.split_hot" => split_hot::split_json(
            &string(&request, "gen").unwrap_or_default(),
            request.get("pieces").and_then(Value::as_u64).unwrap_or(8) as usize,
        ),
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
