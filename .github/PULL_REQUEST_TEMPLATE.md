## Why

<!-- One or two plain sentences: the bug, the missing feature, or the cost this removes. -->

## What changed

<!-- The files and behaviour a reviewer should look at first. -->

## Tests

- [ ] `cargo test --manifest-path builder/rust/Cargo.toml --workspace`
- [ ] `flutter test && flutter analyze` in `builder/flutter` (if touched)
- [ ] Game-free C/C++ fixture (if touched)
- [ ] In the game on my hardware: what I did, what I saw, and the numbers from `hookshot_log.txt` (if runtime)

```text
paste the test output or the log lines here
```

## Not tested

<!-- Another OS, another GPU vendor, real input devices, a game area you could not reach. -->

## Screenshots

<!-- Required when the change shows on screen. Before and after for a fix. -->

## Game data

- [ ] This PR contains no game files, keys, saves, generated caches, or build output.
- [ ] Any game run used my own dump; nothing made from it is committed.

## AI assistance

<!-- If a tool helped, name it here and in the commit trailers. Delete this section if not applicable. -->
