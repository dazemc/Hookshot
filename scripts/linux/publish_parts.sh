#!/bin/bash
# Upload the Linux builder's parts (scripts/linux/make_parts.sh) to the linux-parts release, which is not "latest".
# A zip whose name is already there is left (its checksum is in the name). hookshot-linux.txt itself goes on the
# version release, with the AppImage. Run from CI on a tag, with GH_TOKEN and GITHUB_REPOSITORY. Needs jq.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PARTS=${1:-$ROOT/build-linux/parts}
REPO=${GITHUB_REPOSITORY:?}
TOKEN=${GH_TOKEN:?}
API="https://api.github.com/repos/$REPO"
AUTH=(-H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" -H "User-Agent: Hookshot")
J=$(mktemp)   # (the release's JSON: a private file, not a fixed name in /tmp)
trap 'rm -f "$J"' EXIT

code=$(curl -s -o "$J" -w '%{http_code}' "${AUTH[@]}" "$API/releases/tags/linux-parts")
if [ "$code" = "404" ]; then
    jq -n --arg sha "${GITHUB_SHA:?}" \
        --arg body "Parts the Linux builder downloads. A file name is its checksum; a part that did not change is not uploaded again. This release is not a version of the game." '{
        tag_name: "linux-parts",
        target_commitish: $sha,
        name: "Linux builder parts",
        body: $body,
        prerelease: true,
        make_latest: "false"
    }' | curl -fsSL "${AUTH[@]}" -X POST "$API/releases" -d @- > "$J"
fi
jq -e '.upload_url' "$J" >/dev/null || { echo "linux-parts could not be read"; exit 1; }
upload=$(jq -r '.upload_url' "$J" | cut -d'{' -f1)
have=$(jq -r '.assets[]?.name' "$J")
for zip in "$PARTS"/HookshotLinux-*.zip; do
    [ -f "$zip" ] || continue
    name=$(basename "$zip")
    if printf '%s\n' "$have" | grep -qx "$name"; then
        echo "already on linux-parts: $name"
        continue
    fi
    echo "uploading $name"
    # (a release and the nightly of the same commit can upload the same part at once: then one of them wins)
    if ! curl -fsSL --retry 5 --retry-delay 10 -X POST "${AUTH[@]}" -H "Content-Type: application/zip" \
        --data-binary @"$zip" "$upload?name=$name" >/dev/null; then
        curl -fsSL "${AUTH[@]}" "$API/releases/tags/linux-parts" |
            jq -e --arg n "$name" '[.assets[]?.name] | index($n) != null' >/dev/null
        echo "already on linux-parts: $name (uploaded by another run)"
    fi
done
echo "parts published"
