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
