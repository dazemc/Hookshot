// FSR 3.1's passes are compiled under option names AMD's vendored SDK reads (upstream NotRexed/Hookshot 4e04e517,
// tests/test_fsr3_shader_options.py). A misspelt -D is dropped without a word: glslang compiles the pass with the option
// off, and the checked-in SPIR-V headers still match their sources. Frame interpolation was built that way with the
// upscaler's LOW_RESOLUTION_MOTION_VECTORS spelling of its own LOW_RES_MOTION_VECTORS, so every made frame smeared the
// top-left render-size corner. These are game-free source checks: they read the SDK's preprocessor conditions and the
// script's options, and need no glslang, game, dump or GPU.

use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// The text of a top-level Python list assigned to `name`, from its `name = [` to the `]` that starts a line.
fn python_list(text: &str, name: &str) -> String {
    let marker = format!("{name} = [");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("scripts/fsr3_shaders.py has no {name}"));
    let rest = &text[start + marker.len()..];
    let end = rest
        .find("\n]")
        .unwrap_or_else(|| panic!("{name} does not end with a top-level ]"));
    rest[..end].to_string()
}

/// The NAME of each -DNAME or -DNAME=value in a list.
fn defines(block: &str) -> BTreeSet<String> {
    let option = Regex::new(r#""-D([A-Za-z_]\w*)"#).unwrap();
    option
        .captures_iter(block)
        .map(|c| c[1].to_string())
        .collect()
}

/// Every identifier an #if, #ifdef, #ifndef or #elif tests.
fn asked(text: &str) -> BTreeSet<String> {
    let condition = Regex::new(r"(?m)^[ \t]*#[ \t]*(?:if|ifdef|ifndef|elif)\b(.*)$").unwrap();
    let identifier = Regex::new(r"[A-Za-z_]\w*").unwrap();
    let mut names = BTreeSet::new();
    for line in condition.captures_iter(text) {
        for name in identifier.find_iter(&line[1]) {
            names.insert(name.as_str().to_string());
        }
    }
    names
}

/// The SDK files a family's passes compile from: its pass GLSL and every header they include, followed through their
/// own includes (beside the including file, then in the GPU headers' folder and its family folders, as the script's -I
/// paths do). Only this family's files, so another family's spelling passed to it is caught too. AMD's _hlsl.h
/// callbacks are the HLSL build's; FFX_GLSL picks the _glsl.h ones.
fn compiled_files(passes: &Path) -> BTreeSet<PathBuf> {
    let gpu = root().join("third_party/fidelityfx-sdk/include/FidelityFX/gpu");
    let mut roots = vec![gpu.clone()];
    let mut families: Vec<PathBuf> = fs::read_dir(&gpu)
        .unwrap_or_else(|e| panic!("{}: {e}", gpu.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    families.sort();
    roots.extend(families);

    let include = Regex::new(r#"(?m)^[ \t]*#[ \t]*include[ \t]*"([^"]+)""#).unwrap();
    let mut todo: Vec<PathBuf> = fs::read_dir(passes)
        .unwrap_or_else(|e| panic!("{}: {e}", passes.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "glsl")
        })
        .collect();
    todo.sort();
    let mut seen = BTreeSet::new();
    while let Some(path) = todo.pop() {
        let path = fs::canonicalize(&path).unwrap_or(path);
        if !seen.insert(path.clone()) {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let text = text.replace("\\\n", " ");
        for include in include.captures_iter(&text) {
            let name = &include[1];
            if name.ends_with("_hlsl.h") {
                continue;
            }
            let mut bases = vec![path.parent().expect("a pass has a folder").to_path_buf()];
            bases.extend(roots.iter().cloned());
            for base in bases {
                let found = base.join(name);
                if found.is_file() {
                    todo.push(found);
                    break;
                }
            }
        }
    }
    seen
}

fn family_options(script: &str, options: &str, variants: &str) -> BTreeSet<String> {
    let mut names = defines(&python_list(script, options));
    names.extend(defines(&python_list(script, variants)));
    names
}

#[test]
fn every_fsr3_shader_option_is_one_the_sdk_reads() {
    let script = read("scripts/fsr3_shaders.py");
    let sdk = root().join("third_party/fidelityfx-sdk");
    let families = [
        (
            "upscaler",
            family_options(&script, "OPTIONS", "VARIANTS"),
            sdk.join("shaders/fsr3upscaler"),
        ),
        (
            "frame interpolation",
            family_options(&script, "FI_OPTIONS", "FI_VARIANTS"),
            sdk.join("shaders/frameinterpolation"),
        ),
        (
            "optical flow",
            family_options(&script, "OF_OPTIONS", "OF_VARIANTS"),
            sdk.join("shaders/opticalflow"),
        ),
    ];
    for (family, options, passes) in families {
        let files = compiled_files(&passes);
        assert!(
            !files.is_empty(),
            "{family}: no SDK files found under {}",
            passes.display()
        );
        let mut read_names = BTreeSet::new();
        for file in &files {
            let text =
                fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            read_names.extend(asked(&text.replace("\\\n", " ")));
        }
        assert!(
            !read_names.is_empty(),
            "{family}: no #if found in the SDK files its passes compile"
        );
        for name in &options {
            assert!(
                read_names.contains(name),
                "{family}: scripts/fsr3_shaders.py passes -D{name}, which no #if in the SDK files its passes compile \
                 reads: a misspelt option is dropped without a word"
            );
        }
    }
}

#[test]
fn every_interpolation_option_the_passes_ask_about_is_set() {
    let script = read("scripts/fsr3_shaders.py");
    let sdk = root().join("third_party/fidelityfx-sdk");
    let files = compiled_files(&sdk.join("shaders/frameinterpolation"));
    let mut read_names = BTreeSet::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        read_names.extend(asked(&text.replace("\\\n", " ")));
    }
    let asked: BTreeSet<String> = read_names
        .into_iter()
        .filter(|name| name.starts_with("FFX_FRAMEINTERPOLATION_OPTION_"))
        .collect();
    assert!(!asked.is_empty(), "the interpolation passes ask no option");
    let set = family_options(&script, "FI_OPTIONS", "FI_VARIANTS");
    let missing: Vec<&String> = asked.difference(&set).collect();
    assert!(
        missing.is_empty(),
        "options the interpolation passes ask about that scripts/fsr3_shaders.py leaves unset: {missing:?}"
    );
}

#[test]
fn interpolation_motion_is_the_render_sizes() {
    // The motion vk_fsr3fi.cpp hands the passes is the render size's (vk_fsr3fi.h, Inputs), so the one pass that reads
    // it, reconstruct-and-dilate, is compiled with the SDK's low-resolution option on.
    let script = read("scripts/fsr3_shaders.py");
    let options = defines(&python_list(&script, "FI_OPTIONS"));
    assert!(
        options.contains("FFX_FRAMEINTERPOLATION_OPTION_LOW_RES_MOTION_VECTORS"),
        "frame interpolation is not built with the SDK's LOW_RES_MOTION_VECTORS option"
    );
    assert!(
        !script.contains("FFX_FRAMEINTERPOLATION_OPTION_LOW_RESOLUTION_MOTION_VECTORS"),
        "the upscaler's LOW_RESOLUTION_MOTION_VECTORS spelling is not the interpolation's option"
    );
    let header = root().join(
        "third_party/fidelityfx-sdk/include/FidelityFX/gpu/frameinterpolation/\
         ffx_frameinterpolation_reconstruct_dilated_velocity_and_previous_depth.h",
    );
    let text = fs::read_to_string(&header).unwrap_or_else(|e| panic!("{}: {e}", header.display()));
    assert!(
        asked(&text).contains("FFX_FRAMEINTERPOLATION_OPTION_LOW_RES_MOTION_VECTORS"),
        "reconstruct-and-dilate no longer reads the SDK's low-resolution motion option"
    );
    let inputs = read("gpu/vk/vk_fsr3fi.h");
    assert!(
        inputs.contains("FFX_FRAMEINTERPOLATION_OPTION_LOW_RES_MOTION_VECTORS"),
        "vk_fsr3fi.h no longer names the permutation its inputs expect"
    );
}
