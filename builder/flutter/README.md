# Hookshot Builder (Flutter)

The desktop app for Hookshot: a Flutter UI over the Rust engine in [`../rust`](../rust). It builds a game folder from
the player's own `.nup` or an unpacked `exefs`/`romfs` pair, launches the game, manages profiles and saves, updates a
built folder and pins a release. The app never invokes Python; a missing engine is a typed error.

## Run

Build the engine first, then the app:

```sh
cargo build --release --workspace --manifest-path ../rust/Cargo.toml
flutter pub get
flutter run -d linux    # or: flutter run -d windows
```

The app looks for `../rust/target/release/hookshot_engine` (and the debug build); set `HOOKSHOT_ENGINE_CLI` to use
another engine. `HOOKSHOT_TOOLS` points it at another source tree, and `HOOKSHOT_BUILDER_HOME` moves its settings,
downloads and backups.

## Test

```sh
flutter test
flutter analyze
```

The Rust side's tests are `cargo test --workspace` in `../rust`. The full contributor path, including the game build
and the Windows toolchain, is in [CONTRIBUTING.md](../../CONTRIBUTING.md); the architecture and the engine's JSON
commands are in [docs/fork.md](../../docs/fork.md).
