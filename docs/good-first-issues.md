# Good first issues

These are small, game-free tasks that make the Rust/Flutter builder better without needing a game dump. Pick one,
claim it on the GitHub issue, branch from `main` of [dazemc/Hookshot-rs](https://github.com/dazemc/Hookshot-rs), and
open a PR with the checklist in [`.github/PULL_REQUEST_TEMPLATE.md`](../.github/PULL_REQUEST_TEMPLATE.md).

If you want a task not listed here, look at the open issues labelled `good first issue` and `help wanted`, or the
[workflow](workflow.md) for the priority rules. Never attach game files, keys or generated game data.

## 1. Cache Rust dependencies in the source-regressions workflow

**Files:** `.github/workflows/source-regressions.yml`

The Rust job runs `cargo test --workspace` on Linux and Windows with no cargo registry or target cache, so every
run rebuilds the engine and the fixtures from cold. Add a cache keyed by `builder/rust/Cargo.lock`, the Rust
version and the OS, and restore it before the test step. Keep the cache read-only on pull requests if the
repository's policy requires it.

**Acceptance:** the workflow is actionlint-clean, a warm run restores the cache, a lockfile change misses it, and
the Linux and Windows jobs still pass.

## 2. Check that every `RX_`/`HOS_`/`HOOKSHOT_` switch is documented

**Files:** new test under `builder/rust/tests/` (or `tests/`), `docs/development.md`, `CONTRIBUTING.md`

The renderer and runtime read switches with `getenv("RX_...")` and `getenv("HOS_...")` in `gpu/vk/` and `host/`;
the builder and engine read `std::env::var("HOOKSHOT_...")` in `builder/rust/`. Add a game-free source check that
scans those calls and fails when a switch is missing from the switch tables in `docs/development.md` and
`CONTRIBUTING.md`. Keep a small allowlist for internal/test-only names and explain each entry.

**Acceptance:** the test fails on a deliberately undocumented switch, passes on the current tree, and runs in
`cargo test --workspace`.

## 3. Keyboard-only navigation and a visible focus ring in the Flutter builder

**Files:** `builder/flutter/lib/pages/`, `builder/flutter/lib/navigation.dart`, `builder/flutter/test/`

The builder is usable with a mouse; make the main flow usable with the keyboard alone. Tab and Shift-Tab move
between the primary controls, Enter/Space activate the focused control, Escape backs out of a dialog, and the
focused control has a visible focus ring that meets contrast guidance. Add widget tests for the focus order and
activation.

**Acceptance:** `flutter test` and `flutter analyze` pass; the tests cover the home, files, options and ready
screens; no mouse is needed to start a build.

## 4. Check Markdown anchors, not just file targets

**Files:** `builder/rust/tests/docs.rs`, `readme.md`, `docs/`

`docs.rs` now resolves every relative Markdown link's file target, so a moved file fails `cargo test --workspace`.
It does not check the `#fragment`, and several links to the README still name sections that no longer exist. Add
fragment checking with a GitHub-style slugger and an allowlist for headings generated at runtime, then fix the links
it finds.

**Acceptance:** the check fails on a deliberately broken anchor, passes on the current tree, and runs in the docs
workflow (`.github/workflows/docs.yml`).

## 5. Cover version-policy edge cases in Rust

**Files:** `builder/rust/src/version_policy.rs`, `builder/flutter/lib/updater/version_policy.dart`

The version policy decides which release a profile may switch to. Add tests for malformed tags, release
candidates versus releases, pre-release ordering, a pinned version that disappeared, and a nightly-style
`vX.Y.Z-rs.N-gHASH` tag. Keep the Rust and Dart rules in agreement where they overlap.

**Acceptance:** `cargo test --workspace` and `flutter test` pass; each new case has a comment saying which rule it
protects.

## 6. Make the missing-toolchain error actionable

**Files:** `builder/rust/src/toolchain.rs`, `builder/rust/src/vk_build.rs`, `builder/flutter/lib/diagnostics.dart`

When the Vulkan headers, CPM cache, llvm-mingw or Flutter SDK is missing, the build stops with a message that
names the variable but not the fix. Return the exact missing item, the environment variable or install command,
and a link to the matching section of `docs/development.md`. Add a test with an authored empty toolchain folder.

**Acceptance:** the builder shows the missing item and a copyable fix; the test does not need the real toolchain.

## Claiming and reviewing

- Comment on the GitHub issue with what you will change and how you will test it.
- Keep the PR to one task; do not mix a refactor or a second fix.
- Run the checks in the PR template. If a check cannot run on your machine, say why.
- Rebase on `main` before asking for review.

Thanks for helping. Every one of these removes a papercut for the next contributor.
