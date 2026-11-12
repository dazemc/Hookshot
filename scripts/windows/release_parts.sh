#!/usr/bin/env bash
# The Windows release's parts, without Python: llvm-mingw, the latest release's
# setup list, the Flutter package folder and the setup's part zips.
#
#   scripts/windows/release_parts.sh previous <out list> --repo <owner/name> [--repo ...] [--skip-tag <tag>]
#       the newest stable release's hookshot-setup.txt (of the first repository
#       that has one; not --skip-tag's, the release being made); prints its tag
#   scripts/windows/release_parts.sh llvm-mingw <folder>|--name
#       llvm-mingw's official release, SHA-256 checked, unpacked into <folder>;
#       prints its folder, or with --name its cache-key name
#   scripts/windows/release_parts.sh a64emit <llvm-mingw folder> <out.dll>
#       suyu's AArch64 -> C translator for the Rust engine, built directly
#       with llvm-mingw (not third_party/a64-kit/lift/build_emit.py)
#   scripts/windows/release_parts.sh package <out folder> --bundle <Flutter Release folder>
#       --engine <cargo target/release> --tag <tag> [--a64emit <dll>] [--setup <setup exe>]
#       [--previous <setup list>] [--repository <owner/name>] [--cpm-add <folder>]
#       the package folder the setup splits: the Flutter app and its Rust
#       engine, a64emit, the tracked sources under tools/ and the play profile;
#       --cpm-add adds a <package>/<version> folder (dav1d, scripts/windows/dav1d.sh)
#       to the earlier cpm part's tools/cpm when it lacks it
#   scripts/windows/release_parts.sh cpm-add <setup list> <folder> <tools folder>
#       the earlier cpm part into <tools folder>/cpm with the folder's
#       <package>/<version> folders added when it lacks one
#   scripts/windows/release_parts.sh parts <package folder> <tag> <out folder>
#       --base <url> [--reuse <setup list>] [--require-reuse]
#       the app part zip and hookshot-setup.txt; a cpm part when the package
#       has tools/cpm (dav1d added), else the earlier list's; third_party,
#       wgpu-native and llvm-mingw keep the earlier list's lines, so unchanged
#       parts are not uploaded again
#   scripts/windows/release_parts.sh zip <out.zip> <folder>
#       a zip of the folder's contents (the RecompLauncher zip)
#
# The tests, the setup and the app read the same list format as before
# (builder/setup/setup.c). No game data is involved: the parts hold the app,
# its tools and the play profile (addresses and counts made on the development
# PC).
set -euo pipefail

ROOT=$(unset CDPATH; cd -- "$(dirname "$0")/../.." && pwd)

LLVM_NAME=llvm-mingw-20261104-ucrt-x86_64
LLVM_SHA=552191b5754c49a5a20723276a6fc56541ea1922c4c50b7b669022333fc1b40f
LLVM_SIZE=191037046
LLVM_URL=https://github.com/mstorsjo/llvm-mingw/releases/download/20261104/llvm-mingw-20261104-ucrt-x86_64.zip
LLVM_TAR_NAME=llvm-mingw-20261104-ucrt-ubuntu-22.04-x86_64
LLVM_TAR_SHA=919380403de87dca3c581c2053230cd298af5e891286628ba0613b7fed370926
LLVM_TAR_SIZE=84104244
LLVM_TAR_URL=https://github.com/mstorsjo/llvm-mingw/releases/download/20261104/llvm-mingw-20261104-ucrt-ubuntu-22.04-x86_64.tar.xz

say() { printf '%s\n' "$*" >&2; }
die() { say "error: $*"; exit 1; }

usage() {
    sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
}

is_windows() {
    case "$(uname -s)" in
        MINGW* | MSYS* | CYGWIN* | Windows_NT) return 0 ;;
        *) return 1 ;;
    esac
}

# (Git Bash hands a native tool a D:\... path, but its own coreutils and
# tar read POSIX paths; the scripts keep POSIX paths inside and print the
# Windows form only where a Windows program stores it)
posix_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -u "$1"
    else
        printf '%s' "$1"
    fi
}

win_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    else
        printf '%s' "$1"
    fi
}

cache_dir() {
    printf '%s/release-parts' "$(posix_path "${RUNNER_TEMP:-${TMPDIR:-/tmp}}")"
}

file_size() {
    wc -c <"$1" | tr -d '[:space:]'
}

file_sha() {
    # (coreutils prefixes the hash with a backslash when the file name has
    # one, as a Windows path does; reading stdin keeps the output a bare hash)
    sha256sum <"$1" | awk '{print $1}'
}

check_file() { # path sha size
    [ -f "$1" ] || return 1
    [ "$(file_size "$1")" = "$3" ] || return 1
    [ "$(file_sha "$1")" = "$2" ]
}

fetch() { # url dest
    curl -fsSL --retry 5 --retry-delay 5 --connect-timeout 30 --max-time 900 \
        -o "$2.part" "$1"
    mv -f "$2.part" "$2"
}

fetch_verified() { # url sha size dest
    local url=$1 sha=$2 size=$3 dest=$4 try
    if check_file "$dest" "$sha" "$size"; then
        return 0
    fi
    mkdir -p "$(dirname "$dest")"
    for try in 1 2 3; do
        # (a proxy or a truncated response can leave a short file; retry clean)
        rm -f "$dest" "$dest.part"
        say "downloading $(basename "$url") ($((size >> 20)) MB, try $try)"
        if fetch "$url" "$dest" && check_file "$dest" "$sha" "$size"; then
            return 0
        fi
        say "retrying $(basename "$url"): the download did not match"
        sleep 2
    done
    die "$url: not the size and SHA-256 the list gives"
}

windows_tar() {
    local path="${SYSTEMROOT:-C:/Windows}/System32/tar.exe"
    if command -v cygpath >/dev/null 2>&1; then
        path=$(cygpath -u "$path")
    fi
    printf '%s' "$path"
}

extract_zip() { # zip folder
    local archive folder
    archive=$(posix_path "$1")
    folder=$(posix_path "$2")
    mkdir -p "$folder"
    if command -v unzip >/dev/null 2>&1; then
        unzip -q "$archive" -d "$folder"
    elif command -v bsdtar >/dev/null 2>&1; then
        bsdtar -xf "$archive" -C "$folder"
    elif command -v 7z >/dev/null 2>&1; then
        7z x -y -o"$folder" "$archive" >/dev/null
    else
        "$(windows_tar)" -xf "$archive" -C "$folder"
    fi
}

create_zip() { # out folder
    local out folder
    out=$(posix_path "$1")
    folder=$(posix_path "$2")
    mkdir -p "$(dirname "$out")"
    # (zip runs after cd, so a relative dist/ path must become absolute)
    out=$(cd "$(dirname "$out")" && pwd)/$(basename "$out")
    rm -f "$out"
    if command -v zip >/dev/null 2>&1; then
        (cd "$folder" && zip -qr "$out" .)
    elif command -v bsdtar >/dev/null 2>&1; then
        (cd "$folder" && bsdtar -a -cf "$out" .)
    elif command -v 7z >/dev/null 2>&1; then
        (cd "$folder" && 7z a -tzip "$out" . >/dev/null)
    else
        (cd "$folder" && "$(windows_tar)" -a -cf "$out" .)
    fi
}

part_line() { # setup list kind (app, cpm, third_party, wgpu-native or llvm-mingw)
    awk -v kind="$2" '
        $1 == "part" {
            if (kind == "llvm-mingw") {
                if ($5 ~ /^llvm-mingw-/) { print; exit }
            } else if (index($7, "HookshotBuilder-" kind "-") > 0) {
                print; exit
            }
        }' "$1"
}

part_url() {
    printf '%s' "$1" | awk '{print $7}'
}

part_sha() {
    printf '%s' "$1" | awk '{print $2}'
}

part_size() {
    printf '%s' "$1" | awk '{print $3}'
}

download_part() { # setup list kind -> prints the cached zip
    local list=$1 kind=$2 line url sha size dest
    line=$(part_line "$list" "$kind")
    [ -n "$line" ] || die "no $kind part in $list"
    url=$(part_url "$line")
    sha=$(part_sha "$line")
    size=$(part_size "$line")
    dest="$(cache_dir)/$(basename "$url")"
    fetch_verified "$url" "$sha" "$size" "$dest"
    printf '%s' "$dest"
}

previous() {
    [ $# -ge 1 ] || usage
    local out=$1
    shift
    local -a repos=()
    local skip=""
    while [ $# -gt 0 ]; do
        case "$1" in
            --repo) repos+=("${2:?}"); shift 2 ;;
            --skip-tag) skip="${2:?}"; shift 2 ;;
            *) usage ;;
        esac
    done
    [ ${#repos[@]} -gt 0 ] || die "previous needs --repo"
    command -v jq >/dev/null 2>&1 || die "previous needs jq"
    local repo json tag url line
    json=$(mktemp)
    for repo in "${repos[@]}"; do
        if [ -n "${GH_TOKEN:-}" ]; then
            curl -fsSL --retry 3 --retry-delay 5 \
                -H "Authorization: Bearer $GH_TOKEN" \
                -H "Accept: application/vnd.github+json" \
                "https://api.github.com/repos/$repo/releases?per_page=30" -o "$json" || {
                say "$repo: GitHub did not answer"
                continue
            }
        else
            curl -fsSL --retry 3 --retry-delay 5 -H "Accept: application/vnd.github+json" \
                "https://api.github.com/repos/$repo/releases?per_page=30" -o "$json" || {
                say "$repo: GitHub did not answer"
                continue
            }
        fi
        line=$(jq -r --arg skip "$skip" '
            .[] | select((.draft | not) and (.prerelease | not) and (.tag_name != $skip))
            | .tag_name as $tag | .assets[]
            | select(.name == "hookshot-setup.txt")
            | "\($tag)\t\(.browser_download_url)"' "$json" | head -n 1)
        if [ -n "$line" ]; then
            tag=${line%%$'\t'*}
            url=${line#*$'\t'}
            if fetch "$url" "$out"; then
                printf '%s\n' "$tag"
                rm -f "$json"
                return 0
            fi
            rm -f "$out"
        fi
    done
    rm -f "$json"
    say "no release with a hookshot-setup.txt in ${repos[*]}"
    return 1
}

llvm_mingw() {
    [ $# -ge 1 ] || usage
    local name sha size url tar=""
    if is_windows; then
        name=$LLVM_NAME sha=$LLVM_SHA size=$LLVM_SIZE url=$LLVM_URL
    else
        name=$LLVM_TAR_NAME sha=$LLVM_TAR_SHA size=$LLVM_TAR_SIZE url=$LLVM_TAR_URL tar=1
    fi
    if [ "$1" = "--name" ]; then
        printf '%s\n' "$name"
        return 0
    fi
    local folder target
    folder=$(posix_path "$1")
    target="$folder/$name"
    if [ -f "$target/bin/clang.exe" ] || [ -f "$target/bin/clang" ]; then
        win_path "$target"
        return 0
    fi
    local dest
    dest="$(cache_dir)/$(basename "$url")"
    fetch_verified "$url" "$sha" "$size" "$dest"
    mkdir -p "$folder"
    say "unpacking $(basename "$dest")"
    if [ -n "$tar" ]; then
        tar -xJf "$dest" -C "$folder"
    else
        extract_zip "$dest" "$folder"
    fi
    [ -d "$target" ] || die "$(basename "$dest") did not unpack $name"
    win_path "$target"
}

a64emit() {
    [ $# -eq 2 ] || usage
    local toolchain=$1 out=$2
    local cxx="$toolchain/bin/x86_64-w64-mingw32-clang++"
    if [ ! -f "$cxx" ]; then cxx="$toolchain/bin/clang++"; fi
    [ -f "$cxx" ] || die "no clang++ in $toolchain"
    mkdir -p "$(dirname "$out")"
    "$cxx" -std=c++20 -O2 -shared -o "$out" \
        "$ROOT/third_party/a64-kit/lift/a64emit.cpp" \
        -Wno-everything --target=x86_64-w64-mingw32 -fms-extensions \
        -D_CRT_SECURE_NO_WARNINGS -fuse-ld=lld -static \
        -Wl,-Xlink=/nodefaultlib:libcmt \
        -Wl,-Xlink=/nodefaultlib:oldnames \
        -Wl,-Xlink=/nodefaultlib:libvcruntime \
        -Wl,-Xlink=/nodefaultlib:msvcrt
    say "$out: $(file_size "$out") bytes"
}

cpm_add() {
    [ $# -eq 3 ] || usage
    local list=$1 add=$2 tools=$3
    list=$(posix_path "$list")
    add=$(posix_path "$add")
    tools=$(posix_path "$tools")
    [ -d "$add" ] || die "$add: no <package>/<version> folders"
    local cpm="$tools/cpm"
    if [ -n "$list" ] && [ -f "$list" ]; then
        local zip tmp
        zip=$(download_part "$list" cpm)
        tmp=$(mktemp -d)
        extract_zip "$zip" "$tmp"
        if [ -d "$tmp/HookshotBuilder/tools/cpm" ]; then
            mkdir -p "$cpm"
            cp -a "$tmp/HookshotBuilder/tools/cpm"/. "$cpm"/
            say "cpm: the earlier part unpacked into $cpm"
        else
            say "warning: the earlier cpm part has no tools/cpm"
        fi
        rm -rf "$tmp"
    fi
    local src name version dst added=0
    for src in "$add"/*/*; do
        [ -d "$src" ] || continue
        name=$(basename "$(dirname "$src")")
        version=$(basename "$src")
        dst="$cpm/$name/$version"
        if [ -d "$dst" ]; then
            say "cpm: $name/$version is already there"
        else
            mkdir -p "$cpm/$name"
            cp -a "$src" "$dst"
            added=1
        fi
    done
    if [ "$added" = 0 ]; then
        say "cpm: the earlier part has every package"
    fi
}

package() {
    [ $# -ge 1 ] || usage
    local out=$1
    shift
    local bundle="" engine="" a64="" setup="" tag="" previous="" repository="" cpm_add_folder=""
    while [ $# -gt 0 ]; do
        case "$1" in
            --bundle) bundle="${2:?}"; shift 2 ;;
            --engine) engine="${2:?}"; shift 2 ;;
            --a64emit) a64="${2:?}"; shift 2 ;;
            --setup) setup="${2:?}"; shift 2 ;;
            --tag) tag="${2:?}"; shift 2 ;;
            --previous) previous="${2:?}"; shift 2 ;;
            --repository) repository="${2:?}"; shift 2 ;;
            --cpm-add) cpm_add_folder="${2:?}"; shift 2 ;;
            *) usage ;;
        esac
    done
    [ -n "$bundle" ] || die "package needs --bundle"
    [ -n "$engine" ] || die "package needs --engine"
    [ -n "$tag" ] || die "package needs --tag"

    out=$(posix_path "$out")
    bundle=$(posix_path "$bundle")
    engine=$(posix_path "$engine")
    a64=$(posix_path "$a64")
    setup=$(posix_path "$setup")
    previous=$(posix_path "$previous")

    local app="$out/HookshotBuilder" tools="$out/HookshotBuilder/tools"
    rm -rf "$out"
    mkdir -p "$app" "$tools"
    cp -a "$bundle"/. "$app"/

    # (the setup and the launcher start HookshotBuilder.exe, whatever the
    # Flutter runner called it)
    if [ -f "$app/hookshot_builder.exe" ]; then
        mv -f "$app/hookshot_builder.exe" "$app/HookshotBuilder.exe"
    fi
    [ -f "$app/HookshotBuilder.exe" ] || die "the Flutter bundle has no hookshot_builder.exe"
    for name in hookshot_engine.exe hookshot_engine.dll hookshot_zbic.dll; do
        if [ -f "$engine/$name" ]; then
            cp -f "$engine/$name" "$app/$name"
        fi
    done
    [ -f "$app/hookshot_engine.exe" ] || die "the package has no hookshot_engine.exe"
    [ -f "$app/hookshot_engine.dll" ] || die "the package has no hookshot_engine.dll"
    if [ -n "$a64" ]; then
        cp -f "$a64" "$app/a64emit.dll"
    fi
    [ -f "$app/a64emit.dll" ] || say "warning: no a64emit.dll: the engine cannot translate a game"
    if [ -n "$setup" ]; then
        cp -f "$setup" "$app/HookshotBuilder-Setup.exe"
    fi
    cp -f "$ROOT/LICENSE" "$app/LICENSE"

    # (the tracked source trees the engine compiles: third_party's prebuilt
    # files come from the setup's third_party part, as before)
    git -C "$ROOT" archive --format=tar HEAD -- \
        gpu host nx2 scripts third_party assets readme.md LICENSE | tar -xf - -C "$tools"
    printf '%s\n' "$tag" >"$tools/VERSION"
    printf '%s\n' "$tag" >"$tools/version.txt"
    if [ -n "$repository" ]; then
        case "$repository" in
            */*) ;;
            *) die "repository must be owner/name" ;;
        esac
        printf '%s\n' "$repository" >"$tools/repository.txt"
        printf '%s\n' "$repository" >"$app/repository.txt"
    fi

    # (the play profile in the earlier app part: addresses and counts, no game
    # code; a build without it works, a little slower in play)
    if [ -n "$previous" ]; then
        local zip tmp
        zip=$(download_part "$previous" app)
        tmp=$(mktemp -d)
        extract_zip "$zip" "$tmp"
        if [ -d "$tmp/HookshotBuilder/tools/work" ]; then
            mkdir -p "$tools/work"
            cp -a "$tmp/HookshotBuilder/tools/work"/. "$tools/work"/
            say "play profile: $(find "$tools/work" -type f | wc -l | tr -d ' ') files"
        else
            say "warning: the earlier app part has no play profile"
        fi
        rm -rf "$tmp"
    fi
    if [ -n "$cpm_add_folder" ] && [ -n "$previous" ]; then
        cpm_add "$previous" "$cpm_add_folder" "$tools"
    fi

    find "$out" -name __pycache__ -prune -exec rm -rf {} +
    find "$tools/assets/pipeline_states" -name '*.bin.xz' -size +0 -print -quit 2>/dev/null |
        grep -q . || die "assets/pipeline_states/ holds no pipeline state list"
    say "package folder: $out"
}

parts() {
    [ $# -ge 3 ] || usage
    local package tag out
    package=$(posix_path "$1")
    tag=$2
    out=$(posix_path "$3")
    shift 3
    local base="" reuse="" require=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --base) base="${2:?}"; shift 2 ;;
            --reuse) reuse="${2:?}"; shift 2 ;;
            --require-reuse) require=1; shift ;;
            *) usage ;;
        esac
    done
    [ -f "$package/HookshotBuilder/HookshotBuilder.exe" ] || die "not a package folder: $package"
    [ -n "$base" ] || die "parts needs --base"
    base=${base%/}
    mkdir -p "$out"
    rm -f "$out"/HookshotBuilder-app.zip "$out"/HookshotBuilder-app-*.zip \
        "$out"/HookshotBuilder-cpm-*.zip "$out/hookshot-setup.txt"
    printf '%s\n' "$tag" >"$package/HookshotBuilder/tools/version.txt"
    printf '%s\n' "$tag" >"$package/HookshotBuilder/tools/VERSION"

    local zip="$out/HookshotBuilder-app.zip" sha size named
    create_zip "$zip" "$package"
    sha=$(file_sha "$zip")
    size=$(file_size "$zip")
    named="$out/HookshotBuilder-app-${sha:0:12}.zip"
    mv -f "$zip" "$named"

    local cpm_line=""
    if [ -d "$package/HookshotBuilder/tools/cpm" ]; then
        local stage="$out/.cpm-part" cpm_zip="$out/HookshotBuilder-cpm.zip" csha csize cnamed
        rm -rf "$stage"
        mkdir -p "$stage/HookshotBuilder/tools"
        cp -a "$package/HookshotBuilder/tools/cpm" "$stage/HookshotBuilder/tools/cpm"
        create_zip "$cpm_zip" "$stage"
        csha=$(file_sha "$cpm_zip")
        csize=$(file_size "$cpm_zip")
        cnamed="$out/HookshotBuilder-cpm-${csha:0:12}.zip"
        mv -f "$cpm_zip" "$cnamed"
        cpm_line="part $csha $csize . - - $base/$(basename "$cnamed")"
        rm -rf "$stage"
        say "cpm: a new part with $(find "$package/HookshotBuilder/tools/cpm" -type f | wc -l | tr -d ' ') files, $((csize >> 20)) MB compressed, $csha"
    fi

    local -a lines=("tag $tag" "part $sha $size . - - $base/$(basename "$named")")
    local name line
    for name in cpm third_party wgpu-native; do
        line=""
        if [ "$name" = cpm ] && [ -n "$cpm_line" ]; then line="$cpm_line"; fi
        if [ -z "$line" ] && [ -n "$reuse" ]; then line=$(part_line "$reuse" "$name"); fi
        if [ -n "$line" ]; then
            lines+=("$line")
            if [ "$name" = cpm ] && [ -n "$cpm_line" ]; then
                say "cpm: the new part with dav1d"
            else
                say "$name: as in $(basename "$reuse")"
            fi
        elif [ "$require" = 1 ]; then
            die "no $name part in $reuse: a release needs it"
        else
            say "warning: no $name part in the earlier list: the package lacks $name"
        fi
    done
    line=""
    if [ -n "$reuse" ]; then line=$(part_line "$reuse" llvm-mingw); fi
    if [ -z "$line" ]; then
        line="part $LLVM_SHA $LLVM_SIZE HookshotBuilder/tools $LLVM_NAME llvm-mingw $LLVM_URL"
    fi
    lines+=("$line")
    lines+=("run HookshotBuilder/HookshotBuilder.exe")
    printf '%s\n' "${lines[@]}" >"$out/hookshot-setup.txt"
    say "app: $(find "$package" -type f | wc -l | tr -d ' ') files, $((size >> 20)) MB compressed, $sha"
    say "setup list: $out/hookshot-setup.txt"
}

zip_folder() {
    [ $# -eq 2 ] || usage
    create_zip "$1" "$2"
    say "$1: $(file_size "$1") bytes"
}

[ $# -gt 0 ] || usage
command=$1
shift
case "$command" in
    previous) previous "$@" ;;
    llvm-mingw) llvm_mingw "$@" ;;
    a64emit) a64emit "$@" ;;
    cpm-add) cpm_add "$@" ;;
    package) package "$@" ;;
    parts) parts "$@" ;;
    zip) zip_folder "$@" ;;
    *) usage ;;
esac
