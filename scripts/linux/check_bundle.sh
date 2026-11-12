#!/bin/bash
# Game-free checks of the Linux AppImage or its AppDir: the Flutter app, the Rust engine, the a64emit translator and the
# sources a game is built from are there, nothing needs Python, and (with xvfb-run) the window starts.
#
#   scripts/linux/check_bundle.sh [<Hookshot-...-x86_64.AppImage> | <AppDir>]   (default: the newest in build-linux/)
#
# The image is extracted the way its runtime does, so the checks read what a player downloads, not the checkout.
set -euo pipefail
cd "$(dirname "$0")/../.."
TARGET=${1:-$(find build-linux -maxdepth 1 -name 'Hookshot-*-x86_64.AppImage' -printf '%T@ %p\n' 2>/dev/null |
    sort -rn | head -1 | cut -d' ' -f2-)}
[ -n "$TARGET" ] && [ -e "$TARGET" ] || { echo "no AppImage or AppDir to check (scripts/linux/appimage.sh makes one)"; exit 1; }
TARGET=$(readlink -f "$TARGET")
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
if [ -d "$TARGET" ]; then
    R=$TARGET
else
    (cd "$TMP" && "$TARGET" --appimage-extract >/dev/null) || { echo "$TARGET did not extract: not an AppImage"; exit 1; }
    R=$TMP/squashfs-root
fi
A=$R/usr/lib/hookshot/flutter
T=$R/usr/share/hookshot
fail() { echo "FAIL: $*" >&2; exit 1; }

echo "== the Flutter bundle"
[ -x "$A/hookshot_builder" ] || fail "usr/lib/hookshot/flutter/hookshot_builder is missing"
[ -f "$A/data/icudtl.dat" ] || fail "the Flutter data/ folder is missing"
missing=$(ldd "$A/hookshot_builder" | awk '/not found/ {print}')
[ -z "$missing" ] || fail "shared libraries are missing: $missing"
echo "   hookshot_builder loads ($(stat -c %s "$A/hookshot_builder") bytes)"

echo "== the Rust engine"
[ -x "$A/lib/hookshot_engine" ] || fail "the engine CLI is missing"
[ -f "$A/lib/libhookshot_engine.so" ] || fail "libhookshot_engine.so is missing"
[ -f "$A/lib/libhookshot_zbic.so" ] || fail "libhookshot_zbic.so is missing"
toolchain=$(printf '{"command":"build.toolchain"}' | "$A/lib/hookshot_engine")
grep -q '"clang_ready":true' <<<"$toolchain" || fail "the engine finds no clang: $toolchain"
echo '{"command":"build.kit"}' | "$A/lib/hookshot_engine" | grep -q '"root"' || fail "the engine does not answer build.kit"

echo "== the a64emit translator"
for f in "$A/lib/liba64emit.so" "$T/third_party/a64-kit/lift/liba64emit.so"; do
    [ -f "$f" ] || fail "$f is missing"
    nm -D --defined-only "$f" | grep -q ' a64_translate_batch$' || fail "$f does not export a64_translate_batch"
done
# (a NOP and an ADD: the engine loads liba64emit.so from the image and translates them)
translated=$(printf '{"command":"lift.translate","items":[[4096,3573751839],[4100,2432696320]]}' | "$A/lib/hookshot_engine")
grep -q '"unhandled":0' <<<"$translated" || fail "the engine could not translate with the image's liba64emit.so: $translated"
echo "   liba64emit.so exports a64_translate_batch and translates"

echo "== the sources a game is built from"
for d in host gpu third_party assets builder; do [ -d "$T/$d" ] || fail "usr/share/hookshot/$d is missing"; done
[ -f "$T/VERSION" ] || fail "VERSION is missing"
[ -f "$T/repository.txt" ] || fail "repository.txt is missing"
[ -f "$R/hookshot-linux.txt" ] || fail "hookshot-linux.txt (the update list) is missing"
[ -n "$(find "$T/assets/pipeline_states" -name '*.bin.xz' 2>/dev/null)" ] || fail "assets/pipeline_states/ is empty"
grep -q '^tag ' "$R/hookshot-linux.txt" || fail "the update list has no tag"
grep -q '^part ' "$R/hookshot-linux.txt" || fail "the update list has no part"

echo "== no Python"
# (no interpreter, no script, no Python shebang: the engine is the only build path)
py=$(find "$R" \( -name 'python' -o -name 'python3' -o -name 'python3.*' -o -name '*.py' -o -name '*.pyc' \) -print -quit)
[ -z "$py" ] || fail "Python is in the image: $py"
shebang=$(grep -rIl --exclude='*.so' --exclude='*.a' --exclude='*.bin*' -m1 '^#!.*[Pp]ython' "$R" 2>/dev/null | head -1 || true)
[ -z "$shebang" ] || fail "a script runs Python: $shebang"

echo "== the window starts"
if command -v xvfb-run >/dev/null 2>&1; then
    mkdir -p "$TMP/home" "$TMP/config"
    set +e
    HOME="$TMP/home" XDG_CONFIG_HOME="$TMP/config" timeout -k 5 20 xvfb-run -a "$R/AppRun" \
        > "$TMP/app.log" 2>&1
    code=$?
    set -e
    # (124: still running when the 20 s ran out, which is what a window that opened does)
    if [ "$code" -ne 124 ]; then
        cat "$TMP/app.log"
        fail "hookshot_builder exited with $code instead of staying open"
    fi
    echo "   AppRun stayed open for 20 s under Xvfb"
else
    echo "   xvfb-run is not installed: the bundle's libraries were checked, the window was not started"
fi

echo "a player's builder is in the image: everything above is from $TARGET"
