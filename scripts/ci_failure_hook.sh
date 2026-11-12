#!/usr/bin/env bash
# A local CI failure hook for dazemc/Hookshot-rs.
#
# It polls `gh run list` for the watched branch and posts a short prompt into the current OpenCode session through
# `opencode api`. It is a local process only: it opens no port and registers no webhook, so nothing outside this
# machine can inject a prompt through it. The first poll records the failures that already exist and posts nothing;
# after that, each new failed run is posted once (deduplicated by its databaseId).
#
#   OPENCODE_SESSION_ID=ses_... scripts/ci_failure_hook.sh
#   OPENCODE_SESSION_ID=ses_... scripts/ci_failure_hook.sh --once
#   OPENCODE_SESSION_ID=ses_... scripts/ci_failure_hook.sh --baseline
#
# CI_WATCH_REPO, CI_WATCH_BRANCH, CI_WATCH_INTERVAL and CI_WATCH_STATE_DIR change the defaults below.
set -euo pipefail
shopt -s inherit_errexit

default_repo=dazemc/Hookshot-rs
default_branch=main
default_interval=60

repo=${CI_WATCH_REPO:-$default_repo}
branch=${CI_WATCH_BRANCH:-$default_branch}
interval=${CI_WATCH_INTERVAL:-$default_interval}
state_dir=${CI_WATCH_STATE_DIR:-${XDG_CACHE_HOME:-${HOME:?HOME is not set}/.cache}/hookshot-ci-watch}
seen=$state_dir/seen
baselined=$state_dir/baselined
log_file=$state_dir/watch.log
lock_file=$state_dir/lock

once=0
force_baseline=0

usage() {
    cat <<EOF
usage: OPENCODE_SESSION_ID=ses_... $0 [--once | --baseline]

  --once      poll once, then exit
  --baseline  record the failures that exist now, post nothing, then exit
  -h, --help  show this help

environment:
  OPENCODE_SESSION_ID   the session to post into (required, never guessed)
  CI_WATCH_REPO         repository to watch (default: $default_repo)
  CI_WATCH_BRANCH       branch to watch (default: $default_branch)
  CI_WATCH_INTERVAL     seconds between polls (default: $default_interval)
  CI_WATCH_STATE_DIR    state and log directory (default: ~/.cache/hookshot-ci-watch)
EOF
}

while (($#)); do
    case $1 in
        --once) once=1 ;;
        --baseline) force_baseline=1; once=1 ;;
        -h | --help) usage; exit 0 ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
    shift
done

mkdir -p "$state_dir"
umask 077
touch "$seen" "$log_file"

log() {
    local line
    line="$(date -u '+%Y-%m-%dT%H:%M:%SZ') $*"
    printf '%s\n' "$line" >>"$log_file"
    printf '%s\n' "$line"
}

die() {
    log "error: $*"
    exit 1
}

# (the session comes from the environment, never from a default or a guess: posting into the wrong session would put
# CI text into somebody else's conversation)
if [[ -z ${OPENCODE_SESSION_ID:-} ]]; then
    log "error: OPENCODE_SESSION_ID is not set; refusing to post into a guessed session"
    exit 2
fi

if [[ ! $interval =~ ^[1-9][0-9]*$ ]]; then
    die "CI_WATCH_INTERVAL must be a positive number of seconds, not '$interval'"
fi
for tool in gh jq flock opencode; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool is not on PATH"
done

exec 9>"$lock_file"

failed_runs() {   # gh's JSON on stdin -> databaseId, name, headSha, url per failed run, tab separated
    jq -r '
        .[]
        | select(.status == "completed")
        | select(.conclusion == "failure" or .conclusion == "timed_out" or .conclusion == "startup_failure")
        | [.databaseId, .name, .headSha, .url]
        | @tsv
    '
}

post_failure() {   # name sha url -> 0 when the local OpenCode server accepted the prompt
    local name=$1 sha=$2 url=$3 text payload response
    text=$(printf 'CI failure on %s\nWorkflow: %s\nRun: %s\nCommit: %s' "$branch" "$name" "$url" "$sha")
    payload=$(jq -cn --arg text "$text" '{text: $text}')
    if response=$(opencode api post "/api/session/$OPENCODE_SESSION_ID/prompt" --data "$payload" 2>&1); then
        return 0
    fi
    log "opencode api post failed for $url: $response"
    return 1
}

poll() (   # the subshell holds the lock only for this poll; it is released when the subshell exits
    flock 9
    local runs failed baseline_now=$force_baseline count=0 id name sha url
    if [[ ! -e $baselined ]]; then
        baseline_now=1
    fi

    if ! runs=$(gh run list -R "$repo" --branch "$branch" --limit 30 \
        --json databaseId,name,status,conclusion,headSha,url,updatedAt); then
        log "gh run list failed for $repo branch $branch; will try again"
        return 1
    fi
    if ! failed=$(failed_runs <<<"$runs"); then
        log "could not read the run list from gh; will try again"
        return 1
    fi

    # (a baseline replaces the state with the failures that exist now: no notification is sent for any of them)
    if ((baseline_now)); then
        : >"$seen"
    fi

    while IFS=$'\t' read -r id name sha url; do
        [[ -n $id ]] || continue
        if grep -Fxq -- "$id" "$seen"; then
            continue
        fi
        if ((baseline_now)); then
            printf '%s\n' "$id" >>"$seen"
            count=$((count + 1))
            continue
        fi
        # (recorded only after the post is accepted, so a failed post is tried again on the next poll)
        if post_failure "$name" "$sha" "$url"; then
            printf '%s\n' "$id" >>"$seen"
            count=$((count + 1))
            log "posted failure $id: $name $url (commit $sha)"
        fi
    done <<<"$failed"

    if ((baseline_now)); then
        printf 'baselined %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" >"$baselined.tmp"
        mv "$baselined.tmp" "$baselined"
        log "baseline: recorded $count existing failed run(s); nothing posted"
    elif ((count > 0)); then
        log "posted $count new failure(s)"
    fi
    return 0
)

log "watching $repo branch $branch every ${interval}s; state in $state_dir"

if ((once)); then
    if poll; then
        exit 0
    fi
    exit 1
fi

while :; do
    poll || true
    sleep "$interval"
done
