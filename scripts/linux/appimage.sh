#!/bin/bash
# The Linux AppImage: Hookshot Builder (Flutter) with the Rust engine, the
# a64emit translator and the sources a game is built from. No Python runtime
# and no game data are in it: the game is built from the player's own copy.
#
#   scripts/linux/appimage.sh    -> build-linux/Hookshot-<version>-x86_64.AppImage and its .zsync
#
# Needs: flutter (3.47 or newer), cargo/rustc, clang/clang++; the Flutter
# Linux toolchain (cmake, ninja, pkg-config, libgtk-3-dev); curl for
# appimagetool. The image itself carries none of those: a player's PC
# builds the game with its own clang, as it would from a checkout.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
B=$ROOT/build-linux
APPDIR=$B/AppDir
TOOLS=$B/tools
# (--long: a nightly built on a tagged commit is v2.1.0-rc.1-0-g1a2b3c4, so the updaters know it is the nightly:
# builder/flutter/lib/updater/manifest.dart track_of)
VERSION=${HOOKSHOT_VERSION:-$(git describe --tags --long --always --match "v[0-9]*" 2>/dev/null || echo dev)}
REPOSITORY=${HOOKSHOT_REPOSITORY:-${GITHUB_REPOSITORY:-dazemc/Hookshot-rs}}

# ---- the Rust engine: the Flutter app's FFI library, its CLI, ZBIC ----
echo "== cargo build --release (hookshot_engine)"
cargo build --release --workspace --manifest-path builder/rust/Cargo.toml
for f in hookshot_engine libhookshot_engine.so libhookshot_zbic.so; do
    [ -f "builder/rust/target/release/$f" ] || { echo "builder/rust/target/release/$f was not built"; exit 1; }
done

# ---- the Flutter builder app and its bundle ----
command -v flutter >/dev/null || { echo "flutter is not on PATH"; exit 1; }
echo "== flutter build linux --release"
(cd builder/flutter && flutter pub get && flutter build linux --release)
BUNDLE=$(ls -d builder/flutter/build/linux/x64/release/bundle 2>/dev/null || true)
[ -n "$BUNDLE" ] && [ -x "$BUNDLE/hookshot_builder" ] || { echo "flutter build made no Linux bundle"; exit 1; }

# ---- the a64emit translator, built here rather than by the Python build_emit.py ----
E=third_party/a64-kit/lift
CXX=${CXX:-clang++}
echo "== $CXX liba64emit.so"
"$CXX" -std=c++20 -O2 -shared -fPIC -static-libstdc++ -static-libgcc -Wno-everything \
    -o "$E/liba64emit.so" "$E/a64emit.cpp"
# (the stamp build_emit.py reads; written without it so a checkout's Python build skips the rebuild)
cat "$E/a64emit.cpp" "$E/suyu/arm64_to_c.h" | sha1sum | cut -d' ' -f1 > "$E/liba64emit.so.sha1"

# ---- the AppDir ----
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/lib/hookshot" "$APPDIR/usr/share/hookshot"
L=$APPDIR/usr/lib/hookshot
T=$APPDIR/usr/share/hookshot

# (the Flutter bundle, executable and its lib/ and data/ beside each other)
cp -a "$BUNDLE" "$L/flutter"
cp -a builder/rust/target/release/hookshot_engine builder/rust/target/release/libhookshot_engine.so \
      builder/rust/target/release/libhookshot_zbic.so "$L/flutter/lib/"
cp "$E/liba64emit.so" "$L/flutter/lib/"

# (the source trees the engine compiles a game from, and the translator at the path it is found in a checkout. Only files
# git tracks: a developer's build outputs, or anything made from a dump, never ride along)
git ls-files -z -- host gpu third_party assets | tar --null -T - -cf - | tar -xf - -C "$T"
cp "$E/liba64emit.so" "$T/third_party/a64-kit/lift/liba64emit.so"
cp "$E/liba64emit.so.sha1" "$T/third_party/a64-kit/lift/liba64emit.so.sha1"
cp -a LICENSE readme.md "$T/"
printf '%s\n' "$VERSION" > "$T/VERSION"
printf '%s\n' "$REPOSITORY" > "$T/repository.txt"
mkdir -p "$T/builder/linux"
cp builder/linux/AppRun builder/linux/hookshot-builder.desktop "$T/builder/linux/"
if [ -d work ]; then
    mkdir -p "$T/work/image"
    for f in hot_funcs.txt hot_order.txt hookshot_pieces.prof; do [ -f "work/$f" ] && cp "work/$f" "$T/work/"; done
    [ -f work/image/chunks.txt ] && cp work/image/chunks.txt "$T/work/image/"
fi
# (the pipeline state list's sources: nx2/make_game.py merges them beside the game, and the engine's stage does too.
# Without them a new install's first field load is drawn white for seconds while its pipelines compile)
[ -n "$(find "$T/assets/pipeline_states" -name '*.bin.xz' 2>/dev/null)" ] || \
    { echo "assets/pipeline_states/ holds no pipeline state list: a release ships them (docs/development.md, \"The pipeline state list\")"; exit 1; }
# (no Python in the image at all: the engine is the builder's only build path)
find "$T" \( -name '*.py' -o -name '*.pyc' \) -delete
find "$T" -name __pycache__ -prune -exec rm -rf {} +

# ---- the AppDir's own files ----
cp builder/linux/AppRun "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
cp builder/linux/hookshot-builder.desktop "$APPDIR/"
cp assets/hookshot.png "$APPDIR/hookshot.png"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
cp assets/hookshot.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/hookshot.png"

# (the update list, beside AppRun and outside every hashed part: it names the parts' checksums, and the Flutter
# updater (builder/flutter/lib/updater/installer.dart) downloads only the parts that changed)
BASE="https://github.com/$REPOSITORY/releases/download/linux-parts"
scripts/linux/make_parts.sh "$APPDIR" "$VERSION" "$B/parts" "$BASE"

# ---- appimagetool, pinned (its own runtime download fails whenever GitHub answers 504) ----
APPIMAGETOOL=1.9.1
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
APPIMAGE_RUNTIME=20251108
APPIMAGE_RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
mkdir -p "$TOOLS"
fetch() {   # url file sha256
    [ -f "$2" ] && echo "$3  $2" | sha256sum -c --quiet - && return 0
    echo "downloading $1"
    curl -fL --retry 5 --retry-delay 10 --proto '=https' --tlsv1.2 --connect-timeout 20 --max-time 300 -o "$2.part" "$1"
    echo "$3  $2.part" | sha256sum -c --quiet -
    mv -f "$2.part" "$2"
}
fetch "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL/appimagetool-x86_64.AppImage" \
    "$TOOLS/appimagetool" "$APPIMAGETOOL_SHA256"
chmod +x "$TOOLS/appimagetool"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$APPIMAGE_RUNTIME/runtime-x86_64" \
    "$TOOLS/runtime-x86_64" "$APPIMAGE_RUNTIME_SHA256"

# The update information AppImageUpdate and Gear Lever read: the newest .zsync of this repository's latest release (or of
# the release HOOKSHOT_UPDATE_RELEASE names). appimagetool writes it into the image and makes the .zsync beside it.
UPDATE_REPO=$REPOSITORY
UPDATE_INFO=${HOOKSHOT_UPDATE_INFO:-"gh-releases-zsync|${UPDATE_REPO%%/*}|${UPDATE_REPO#*/}|${HOOKSHOT_UPDATE_RELEASE:-latest}|Hookshot-*-x86_64.AppImage.zsync"}
OUT=$B/Hookshot-$VERSION-x86_64.AppImage
rm -f "$OUT" "$OUT.zsync"
echo "== appimagetool"
(cd "$B" && ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOLS/appimagetool" --runtime-file "$TOOLS/runtime-x86_64" \
    --comp zstd -u "$UPDATE_INFO" "$APPDIR" "$(basename "$OUT")")
[ -f "$OUT.zsync" ] || { echo "appimagetool made no $OUT.zsync"; exit 1; }
echo "$OUT: $(du -h "$OUT" | cut -f1), update information: $UPDATE_INFO"
