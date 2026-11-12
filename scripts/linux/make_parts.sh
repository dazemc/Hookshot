#!/bin/bash
# The parts a Linux update downloads (builder/flutter/lib/updater/installer.dart), from an AppDir. Written without
# Python: each directory that changes on its own is one deterministic zip, and the list names their SHA-256 and size.
#
#   scripts/linux/make_parts.sh <AppDir> <tag> <out folder> [<base url>]
#
# The zips are uploaded to the linux-parts release; the version's own release carries hookshot-linux.txt, which names
# them. The list is not inside a zip (it names the zips' checksums) and is also written beside the AppDir's AppRun.
set -euo pipefail
APPDIR=$(readlink -f "$1")
TAG=$2
OUT=$(readlink -f "$3")
BASE=${4:-https://github.com/dazemc/Hookshot-rs/releases/download/linux-parts}
BASE=${BASE%/}
PARTS=(usr/share/hookshot usr/lib/hookshot/flutter)
[ -f "$APPDIR/AppRun" ] || { echo "not an AppDir (no AppRun): $APPDIR"; exit 1; }
mkdir -p "$OUT"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

# (mtimes fixed and names sorted: the same tree packs to the same bytes, so an unchanged part keeps its name and the
# installer copies the one it already has instead of downloading it again)
for i in "${!PARTS[@]}"; do
    dest=${PARTS[$i]}
    [ -d "$APPDIR/$dest" ] || { echo "the AppDir has no $dest"; exit 1; }
    stage=$STAGE/$i
    mkdir -p "$stage"
    cp -a "$APPDIR/$dest/." "$stage/"
    TZ=UTC find "$stage" -exec touch -h -t 202601010000.00 {} +
    leaf=${dest//\//-}
    zip=$OUT/HookshotLinux-$leaf.zip
    rm -f "$zip"
    (cd "$stage" && TZ=UTC find . -mindepth 1 \( -type f -o -type l \) -printf '%P\n' | LC_ALL=C sort |
        TZ=UTC zip -y -X -q -9 "$zip" -@)
    sha=$(sha256sum "$zip" | cut -d' ' -f1)
    size=$(stat -c %s "$zip")
    named=$OUT/HookshotLinux-$leaf-${sha:0:12}.zip
    mv -f "$zip" "$named"
    {
        printf 'part %s %d %s %s/%s\n' "$sha" "$size" "$dest" "$BASE" "${named##*/}"
        printf 'tree\n'
    } >> "$STAGE/list"
    printf '%-32s %5d MB  %s\n' "$dest" "$((size >> 20))" "${sha:0:16}"
done

{ printf 'tag %s\n' "$TAG"; cat "$STAGE/list"; } > "$OUT/hookshot-linux.txt"
cp "$OUT/hookshot-linux.txt" "$APPDIR/hookshot-linux.txt"
