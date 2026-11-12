// Game-free checks of the release scripts and the workflows that call them:
// a draft keeps its tag when its notes are written, a release just made is
// waited for, and the AppImage carries every tracked ZBIC source a DLSS build
// reads. No game, release or network is involved.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A shell function's body, from its `name() {` to its closing `}` line.
fn shell_function<'a>(text: &'a str, name: &str) -> &'a str {
    let marker = format!("{name}() {{");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let end = text[start..]
        .find("\n}\n")
        .unwrap_or_else(|| panic!("{name} has no end"))
        + start
        + 3;
    &text[start..end]
}

#[test]
fn a_draft_keeps_its_tag_when_its_notes_are_written() {
    let files = source("scripts/release_files.sh");
    // (publish: both a prerelease and a version send their tag_name)
    let publish = shell_function(&files, "publish");
    assert_eq!(
        publish.matches("tag_name: $t").count(),
        2,
        "publish must send tag_name for both release kinds:\n{publish}"
    );
    let finish = shell_function(&files, "finish");
    assert!(
        finish.contains("--arg t \"$tag\"") && finish.contains("{tag_name: $t, body: $body}"),
        "finish changes the notes without their tag_name:\n{finish}"
    );

    let promote = source("scripts/release_promote.sh");
    assert!(
        promote.contains("--arg t \"$tag\"")
            && promote.contains("{tag_name: $t, draft: true, body: $body}"),
        "Promote's notes change a draft without its tag_name"
    );

    let rc = source("scripts/release_rc.sh");
    assert!(
        rc.contains(
            "jq -n --arg t \"$RC\" --arg n \"$TITLE\" --arg b \"$notes\" '{tag_name: $t, name: $n, body: $b, draft: false, prerelease: true, make_latest: \"false\"}'"
        ),
        "rc's final notes change its release without its tag_name"
    );
}

#[test]
fn a_release_just_made_is_waited_for() {
    let rc = source("scripts/release_rc.sh");
    let created = rc
        .find("gh api -X POST \"repos/$GH_REPO/releases\"")
        .expect("rc makes no release");
    let after = &rc[created..];
    assert!(
        after.contains("for _ in 1 2 3 4 5 6; do")
            && after.contains("info=$(\"$FILES\" find \"$RC\")")
            && after.contains("sleep 3"),
        "rc does not look for its release again:\n{after}"
    );

    let upload = source("scripts/release_upload.sh");
    let nightly = upload
        .find("gh release create nightly")
        .expect("upload makes no nightly");
    let after = &upload[nightly..];
    assert!(
        after.contains("for _ in 1 2 3 4 5 6; do")
            && after.contains("[[ -z $(\"$files\" find nightly) ]] || break")
            && after.contains("sleep 3"),
        "the nightly is not waited for after it is made:\n{after}"
    );
    let tag = upload
        .find("gh release create \"$tag\"")
        .expect("upload makes no tag release");
    let after = &upload[tag..];
    assert!(
        after.contains("for _ in 1 2 3 4 5 6; do")
            && after.contains("info=$(\"$files\" find \"$tag\")")
            && after.contains("sleep 3"),
        "a tag's release is not waited for after it is made:\n{after}"
    );
}

#[test]
fn a_sums_file_replaced_between_its_listing_and_its_read_is_read_again() {
    let files = source("scripts/release_files.sh");
    let uploaded = shell_function(&files, "uploaded");
    assert!(
        uploaded.contains("for i in 1 2 3; do")
            && uploaded.contains(
                "asset=$(assets \"$1\" | jq -r '.[] | select(.name == \"SHA256SUMS\") | .id')"
            )
            && uploaded.contains("gh api -H 'Accept: application/octet-stream'"),
        "uploaded() gives up when SHA256SUMS was replaced between its listing and its read:\n{uploaded}"
    );
}

#[test]
fn the_appimage_ships_every_tracked_zbic_source() {
    let appimage = source("scripts/linux/appimage.sh");
    assert!(
        appimage.contains("git ls-files -z -- host gpu third_party assets"),
        "the AppImage no longer carries the tracked source trees"
    );
    assert!(
        !appimage.contains("cp -a third_party/zbic/zbic.so"),
        "the AppImage copies only zbic.so, not the sources a DLSS build compiles"
    );
    for file in [
        "third_party/zbic/zstd.c",
        "third_party/zbic/zstd.h",
        "third_party/zbic/zstd_errors.h",
        "third_party/zbic/zbic_api.c",
    ] {
        assert!(
            root().join(file).is_file(),
            "{file} is not tracked: a DLSS build in the AppImage would stop"
        );
    }
}

#[test]
fn the_nightly_tag_only_moves_forward() {
    let upload = source("scripts/release_upload.sh");
    let advance = shell_function(&upload, "advance_nightly");
    assert!(
        advance.contains("repos/$GH_REPO/compare/$held...$sha")
            && advance.contains("-F force=false"),
        "the nightly tag is not moved as a checked fast-forward:\n{advance}"
    );
    assert!(
        advance.contains("behind)")
            && advance.contains("left to that commit's files")
            && advance.contains("diverged)")
            && advance.contains("main's newest commit"),
        "a nightly older than the tag, or main rewritten, is not handled:\n{advance}"
    );
    assert!(
        upload.contains("advance_nightly \"$sha\" || exit 0"),
        "the nightly no longer takes the tag only when it comes after the held one"
    );
    assert!(
        !upload.contains("is no longer main's newest commit"),
        "the nightly still waits for main's newest commit instead of moving forward"
    );
    assert!(
        upload.contains("Each file is named by the commit it was built from"),
        "the nightly's notes name one commit and date, which the two platforms race to write"
    );
}

#[test]
fn nightly_builds_do_not_cancel_each_other() {
    for file in [
        ".github/workflows/linux-appimage.yml",
        ".github/workflows/windows-builder.yml",
    ] {
        let workflow = source(file);
        assert!(
            workflow.contains("cancel-in-progress: ${{ github.event_name == 'pull_request' }}"),
            "{file}: a newer push cancels a nightly build that may be moving the tag"
        );
        assert!(
            !workflow.contains("ref_type != 'tag'"),
            "{file}: push builds are cancelled instead of queued one at a time"
        );
        assert!(
            workflow.contains("scripts/release_upload.sh counts on it"),
            "{file}: the concurrency comment does not say the nightly's forward-only move relies on it"
        );
    }
}

#[test]
fn the_docs_the_rust_tests_read_trigger_the_source_checks() {
    let workflow = source(".github/workflows/source-regressions.yml");
    for path in [
        "docs/development.md",
        "docs/versioning.md",
        "readme.md",
        "gpu/PROVENANCE.md",
        "scripts/windows/release_parts.sh",
    ] {
        assert!(
            workflow.matches(&format!("- '{path}'")).count() == 2,
            "{path} does not trigger both the push and pull request checks"
        );
    }
    let versioning = source("docs/versioning.md");
    assert!(
        versioning.contains("The tag only\nmoves forward")
            && versioning.contains("moved as a\nfast-forward"),
        "docs/versioning.md still says a nightly waits for main to be quiet"
    );
}

#[test]
fn a_manifest_is_not_uploaded_under_another_release() {
    let upload = source("scripts/release_upload.sh");
    let check = upload
        .find("hookshot-setup.txt | hookshot-linux.txt")
        .expect("release_upload.sh does not check a setup list's tag");
    let upload_call = upload
        .find("\"$files\" upload \"$tag\" \"$@\"")
        .expect("release_upload.sh does not upload the files");
    assert!(
        check < upload_call && upload.contains("not 'tag $tag'"),
        "release_upload.sh uploads a list without checking its tag:\n{upload}"
    );

    let parts = source("scripts/windows/release_parts.sh");
    assert!(
        parts.contains("safe_tag \"$tag\" || die \"not a version name: $tag\""),
        "release_parts.sh makes a list from a tag it did not validate"
    );
    assert!(
        parts.contains("[ \"$(head -n 1 \"$out/hookshot-setup.txt\")\" = \"tag $tag\" ]"),
        "release_parts.sh writes a setup list without checking its first line"
    );
}

#[cfg(unix)]
#[test]
fn release_upload_refuses_a_manifest_for_another_tag() {
    let dir = std::env::temp_dir().join(format!(
        "hookshot-release-upload-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let list = dir.join("hookshot-setup.txt");
    fs::write(
        &list,
        "tag v9.9.9\npart 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef 1 . https://e/a\ntree\n",
    )
    .unwrap();
    let run = std::process::Command::new("bash")
        .arg(root().join("scripts/release_upload.sh"))
        .arg("v1.0.0")
        .arg(&list)
        .output()
        .expect("run release_upload.sh");
    assert!(!run.status.success(), "a stale list was accepted");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("not 'tag v1.0.0'"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_release_workflow_finishes_before_it_publishes() {
    let workflow = source(".github/workflows/release.yml");
    let finish = workflow
        .find("scripts/release_files.sh finish \"$TAG\" SHA256SUMS")
        .expect("the Release workflow does not finish the release");
    let publish = workflow
        .find("scripts/release_files.sh publish \"$TAG\"")
        .expect("the Release workflow does not publish the release");
    assert!(
        finish < publish,
        "the Release workflow publishes before its files and sums are there"
    );
    assert!(
        workflow.contains("scripts/release_rc.sh"),
        "the Release workflow no longer updates rc"
    );
}
