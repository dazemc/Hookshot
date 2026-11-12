// Game-free checks of the CI workflows themselves: every action pinned, every
// workflow read-only at the top, every job timed out, runners and the Flutter
// SDK pinned, release builds kept off the caches, the AppImage's package retry
// refreshing its index, and the Windows parts refusing an archive member that
// leaves its folder. No network, game or release is involved.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn workflows() -> Vec<(String, String)> {
    let dir = root().join(".github/workflows");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("workflow entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (
                name,
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            )
        })
        .collect()
}

/// A shell function's body, from its `name() {` to its closing `}` line, at
/// whatever indentation it has (a workflow's `run:` block indents its functions).
fn shell_function<'a>(text: &'a str, name: &str) -> &'a str {
    let marker = format!("{name}() {{");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let indent = &text[line_start..start];
    let end_marker = format!("\n{indent}}}\n");
    let offset = text[start..]
        .find(&end_marker)
        .unwrap_or_else(|| panic!("{name} has no end"));
    let close = start + offset + 1 + indent.len() + 1;
    &text[start..close]
}

/// The jobs of a workflow, by their key and their whole block. A job key is a
/// two-space-indented name after `jobs:`; its fields are four spaces in.
fn jobs(workflow: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = workflow.lines().collect();
    let Some(start) = lines.iter().position(|line| *line == "jobs:") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut current: Option<(String, usize)> = None;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if let Some(name) = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|name| {
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
        {
            if let Some((prev, at)) = current.take() {
                out.push((prev, lines[at..i].join("\n")));
            }
            current = Some((name.to_string(), i));
        }
    }
    if let Some((name, at)) = current {
        out.push((name, lines[at..].join("\n")));
    }
    out
}

#[test]
fn every_action_is_pinned_to_a_commit() {
    for (file, workflow) in workflows() {
        for line in workflow.lines() {
            let Some(rest) = line
                .trim_start()
                .strip_prefix("- uses: ")
                .or_else(|| line.trim_start().strip_prefix("uses: "))
            else {
                continue;
            };
            let action = rest.split_whitespace().next().unwrap_or(rest);
            if action.starts_with("./") {
                continue;
            }
            let (name, rev) = action
                .split_once('@')
                .unwrap_or_else(|| panic!("{file}: {action} has no pinned revision"));
            assert_eq!(
                rev.len(),
                40,
                "{file}: {name} is not pinned to a full commit: {action}"
            );
            assert!(
                rev.chars().all(|c| c.is_ascii_hexdigit()),
                "{file}: {name}'s revision is not a commit: {action}"
            );
        }
    }
}

#[test]
fn every_workflow_starts_read_only() {
    for (file, workflow) in workflows() {
        assert!(
            workflow.contains("\npermissions:\n  contents: read\n")
                || workflow.starts_with("permissions:\n  contents: read\n"),
            "{file} does not declare permissions: contents: read at the top"
        );
        assert!(
            !workflow.contains("write-all"),
            "{file} asks for write-all instead of the one permission a job needs"
        );
    }
}

#[test]
fn every_job_has_a_timeout() {
    for (file, workflow) in workflows() {
        for (name, block) in jobs(&workflow) {
            // (a reusable workflow call's time is its own jobs')
            if block.contains("uses: ./.github/workflows/") {
                continue;
            }
            assert!(
                block.contains("timeout-minutes:"),
                "{file}: job {name} has no timeout-minutes"
            );
        }
    }
}

#[test]
fn runners_are_pinned() {
    for (file, workflow) in workflows() {
        for line in workflow.lines() {
            if let Some(image) = line.trim_start().strip_prefix("runs-on: ") {
                assert!(
                    !image.trim().ends_with("-latest"),
                    "{file}: {image} moves under the workflow; pin the image"
                );
            }
        }
    }
    let regressions = source(".github/workflows/source-regressions.yml");
    assert!(
        regressions.contains("os: [ubuntu-24.04, windows-2022]"),
        "the Rust tests' runners are not pinned"
    );
}

#[test]
fn the_flutter_sdk_is_the_same_everywhere() {
    let appimage = source(".github/workflows/linux-appimage.yml");
    let tests = source(".github/workflows/source-regressions.yml");
    let windows = source(".github/workflows/windows-builder.yml");
    let version = |text: &str, key: &str| {
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with(key))
            .unwrap_or_else(|| panic!("no {key}"));
        line.split('\'')
            .nth(1)
            .unwrap_or_else(|| panic!("{key} has no quoted value: {line}"))
            .to_string()
    };
    let appimage_version = version(&appimage, "FLUTTER_VERSION:");
    assert_eq!(appimage_version, version(&tests, "flutter-version:"));
    assert_eq!(appimage_version, version(&windows, "flutter-version:"));
    let sha = version(&appimage, "FLUTTER_SHA256:");
    assert_eq!(
        sha.len(),
        64,
        "the AppImage's Flutter SHA-256 is not a SHA-256"
    );
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn release_builds_do_not_use_caches() {
    for file in [
        ".github/workflows/linux-appimage.yml",
        ".github/workflows/windows-builder.yml",
    ] {
        let workflow = source(file);
        for (name, block) in jobs(&workflow) {
            let mut step = String::new();
            for line in block.lines() {
                if let Some(rest) = line.strip_prefix("      - ") {
                    if !step.is_empty() && step.contains("uses: actions/cache") {
                        assert!(
                            step.contains("if: env.RELEASE_TAG == ''"),
                            "{file}: job {name}'s cache step runs on a release:\n{step}"
                        );
                    }
                    step = format!("      - {rest}\n");
                } else {
                    step.push_str(line);
                    step.push('\n');
                }
            }
            if step.contains("uses: actions/cache") {
                assert!(
                    step.contains("if: env.RELEASE_TAG == ''"),
                    "{file}: job {name}'s cache step runs on a release:\n{step}"
                );
            }
        }
    }
    let windows = source(".github/workflows/windows-builder.yml");
    assert!(
        windows.contains("cache: ${{ env.RELEASE_TAG == '' }}"),
        "the Windows Flutter SDK cache is used by release builds"
    );
}

#[test]
fn a_flutter_lockfile_change_reuses_the_pub_cache() {
    let workflow = source(".github/workflows/linux-appimage.yml");
    assert!(
        workflow.contains("key: flutter-${{ env.FLUTTER_VERSION }}-linux\n"),
        "the Flutter SDK cache is invalidated by a pubspec.lock change"
    );
    assert!(
        workflow.contains("key: flutter-pub-${{ hashFiles('builder/flutter/pubspec.lock') }}")
            && workflow.contains("restore-keys: flutter-pub-"),
        "a changed pubspec.lock starts the pub cache from nothing"
    );
}

#[test]
fn the_appimage_package_retry_refreshes_its_index() {
    let workflow = source(".github/workflows/linux-appimage.yml");
    let toolchain = shell_function(&workflow, "toolchain_packages");
    assert!(
        toolchain.contains("apt-get update &&"),
        "a retried package install uses the index the failed try had:\n{toolchain}"
    );
    assert!(
        workflow.contains("retry_packages toolchain_packages"),
        "the package install is no longer retried"
    );
}

#[test]
fn the_windows_parts_refuse_an_archive_member_that_leaves_its_folder() {
    let script = source("scripts/windows/release_parts.sh");
    let extract = shell_function(&script, "extract_zip");
    assert!(
        script.contains("zip_members() {"),
        "the parts no longer list an archive before unpacking it"
    );
    assert!(
        extract.contains("members=$(zip_members \"$archive\")"),
        "extract_zip does not check the archive's members:\n{extract}"
    );
    assert!(
        extract.contains("die \"$archive: $member leaves $folder\""),
        "extract_zip does not refuse a member outside its folder:\n{extract}"
    );
}

#[test]
fn downloads_are_https_only() {
    for (file, workflow) in workflows() {
        for line in workflow.lines() {
            let command = line.trim_start();
            if !(command.starts_with("curl ") || command.contains("$(curl ")) {
                continue;
            }
            assert!(
                line.contains("--proto '=https'"),
                "{file}: a download is not https-only: {line}"
            );
        }
    }
    let parts = source("scripts/windows/release_parts.sh");
    assert!(
        shell_function(&parts, "fetch").contains("--proto '=https'"),
        "release_parts.sh fetch follows a link off https"
    );
    for script in ["scripts/linux/appimage.sh", "scripts/windows/dav1d.sh"] {
        assert!(
            source(script).contains("--proto '=https'"),
            "{script} downloads without restricting the transfer to https"
        );
    }
}

#[test]
fn the_appimage_build_uses_the_lockfile() {
    let appimage = source("scripts/linux/appimage.sh");
    assert!(
        appimage.contains("cargo build --release --locked --workspace"),
        "the AppImage's engine build ignores Cargo.lock"
    );
}
