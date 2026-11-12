#!/bin/sh
# Build the Rust engine and copy it beside the Flutter app for `flutter run`.
set -eu
here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
cargo build --release --workspace --manifest-path "$here/Cargo.toml"
for bundle in "$root"/builder/flutter/build/linux/x64/*/bundle; do
    [ -d "$bundle" ] || continue
    mkdir -p "$bundle/lib"
    cp "$here/target/release/libhookshot_engine.so" "$bundle/lib/"
    cp "$here/target/release/hookshot_engine" "$bundle/lib/"
    cp "$here/target/release/libhookshot_zbic.so" "$bundle/lib/"
done
