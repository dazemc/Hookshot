#!/usr/bin/env bash
# Game-free checks for scripts/ci_failure_hook.sh. Fake gh and opencode binaries stand in for GitHub and the local
# OpenCode server, so this needs no GitHub credentials, no server and no game data.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$here/.." && pwd)
hook=$root/scripts/ci_failure_hook.sh
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ci-failure-hook-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

fake=$tmp/bin
state=$tmp/state
gh_calls=$tmp/gh.calls
opencode_calls=$tmp/opencode.calls
runs=$tmp/runs.json
mkdir -p "$fake" "$state"
: >"$gh_calls"
: >"$opencode_calls"

cat >"$fake/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$FAKE_GH_CALLS"
cat "$FAKE_GH_RUNS"
SH
cat >"$fake/opencode" <<'SH'
#!/usr/bin/env bash
{
    printf 'CALL\n'
    printf 'ARG %s\n' "$@"
    printf 'END\n'
} >>"$FAKE_OPENCODE_CALLS"
sleep "${FAKE_OPENCODE_SLEEP:-0}"
SH
chmod +x "$fake/gh" "$fake/opencode"

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}
pass() { printf 'ok - %s\n' "$*"; }

calls() { grep -c '^CALL$' "$opencode_calls" || true; }

run_hook() {
    env PATH="$fake:$PATH" \
        OPENCODE_SESSION_ID=ses_test \
        CI_WATCH_STATE_DIR="$state" \
        FAKE_GH_RUNS="$runs" \
        FAKE_GH_CALLS="$gh_calls" \
        FAKE_OPENCODE_CALLS="$opencode_calls" \
        FAKE_OPENCODE_SLEEP="${FAKE_OPENCODE_SLEEP:-}" \
        "$hook" "$@"
}

# (one run per line, with the fields `gh run list --json` prints)
run_json() {
    printf '[%s]\n' "$(printf '%s\n' "$@" | paste -sd, -)"
}
run_entry() {   # id name status conclusion sha
    printf '{"databaseId":%s,"name":"%s","status":"%s","conclusion":"%s","headSha":"%s","url":"https://github.com/dazemc/Hookshot-rs/actions/runs/%s","updatedAt":"2026-11-12T22:00:00Z"}' \
        "$1" "$2" "$3" "$4" "$5" "$1"
}

# 1. The first poll records the failures already there and posts nothing.
run_json \
    "$(run_entry 100 'Windows builder' completed failure aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)" \
    "$(run_entry 101 'Release' completed success bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)" >"$runs"
run_hook --once >"$tmp/first.log" 2>&1
[[ $(calls) -eq 0 ]] || fail "the first poll posted a baseline failure"
grep -Fxq 100 "$state/seen" || fail "the first poll did not record run 100"
if grep -Fxq 101 "$state/seen"; then fail "the first poll recorded a successful run"; fi
grep -Fq 'run list -R dazemc/Hookshot-rs --branch main --limit 30 --json databaseId,name,status,conclusion,headSha,url,updatedAt' \
    "$gh_calls" || fail "gh was not called with the expected repository, branch and fields"
grep -Fq 'baseline: recorded 1 existing failed run(s); nothing posted' "$state/watch.log" ||
    fail "the baseline was not logged"
pass "the first poll baselines existing failures without posting"

# 2. A new failure posts once, with the workflow, URL and commit.
run_json \
    "$(run_entry 100 'Windows builder' completed failure aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)" \
    "$(run_entry 101 'Release' completed success bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)" \
    "$(run_entry 102 'Game-free source regressions' completed failure cccccccccccccccccccccccccccccccccccccccc)" >"$runs"
run_hook --once >"$tmp/second.log" 2>&1
[[ $(calls) -eq 1 ]] || fail "a new failure did not post exactly once (posted $(calls) times)"
grep -Fxq 'ARG /api/session/ses_test/prompt' "$opencode_calls" ||
    fail "opencode was not called for the session in OPENCODE_SESSION_ID"
grep -Fxq 'ARG api' "$opencode_calls" || fail "opencode was not called with the api command"
grep -Fxq 'ARG post' "$opencode_calls" || fail "opencode was not called with POST"
payload=$(awk '/^ARG --data$/{getline; sub(/^ARG /, ""); print; exit}' "$opencode_calls")
[[ -n $payload ]] || fail "the post had no --data payload"
jq -e '.text | contains("CI failure on main")
    and contains("Game-free source regressions")
    and contains("https://github.com/dazemc/Hookshot-rs/actions/runs/102")
    and contains("cccccccccccccccccccccccccccccccccccccccc")' <<<"$payload" >/dev/null ||
    fail "the payload is missing the workflow, URL or commit: $payload"
grep -Fxq 102 "$state/seen" || fail "the new failure was not recorded"
pass "a new failure posts once with the workflow, URL and commit"

# 3. Polling again does not post the same run again.
run_hook --once >"$tmp/third.log" 2>&1
[[ $(calls) -eq 1 ]] || fail "the second poll posted run 102 again"
pass "a second poll does not post the same failure again"

# 4. --baseline records the current failures and posts nothing, even when one is new.
run_json \
    "$(run_entry 100 'Windows builder' completed failure aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)" \
    "$(run_entry 102 'Game-free source regressions' completed failure cccccccccccccccccccccccccccccccccccccccc)" \
    "$(run_entry 103 'Linux AppImage' completed timed_out dddddddddddddddddddddddddddddddddddddddd)" >"$runs"
run_hook --baseline >"$tmp/baseline.log" 2>&1
[[ $(calls) -eq 1 ]] || fail "--baseline posted a failure"
grep -Fxq 103 "$state/seen" || fail "--baseline did not record the timed-out run"
pass "--baseline records current failures without posting"

# 5. An unset session exits non-zero before gh or opencode is called.
gh_before=$(wc -l <"$gh_calls")
if env -u OPENCODE_SESSION_ID PATH="$fake:$PATH" \
    CI_WATCH_STATE_DIR="$state" \
    FAKE_GH_RUNS="$runs" \
    FAKE_GH_CALLS="$gh_calls" \
    FAKE_OPENCODE_CALLS="$opencode_calls" \
    "$hook" --once >"$tmp/no-session.log" 2>&1; then
    fail "an unset OPENCODE_SESSION_ID exited zero"
fi
[[ $(calls) -eq 1 ]] || fail "an unset OPENCODE_SESSION_ID still called opencode"
[[ $(wc -l <"$gh_calls") -eq $gh_before ]] || fail "an unset OPENCODE_SESSION_ID still called gh"
grep -Fq 'refusing to post into a guessed session' "$state/watch.log" ||
    fail "the missing session was not logged"
pass "an unset session exits non-zero without calling opencode"

# 6. Two overlapping polls cannot both post the same new failure.
run_json \
    "$(run_entry 100 'Windows builder' completed failure aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)" \
    "$(run_entry 102 'Game-free source regressions' completed failure cccccccccccccccccccccccccccccccccccccccc)" \
    "$(run_entry 103 'Linux AppImage' completed timed_out dddddddddddddddddddddddddddddddddddddddd)" \
    "$(run_entry 104 'Vulkan back end with NVIDIA DLSS' completed failure eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee)" >"$runs"
before=$(calls)
FAKE_OPENCODE_SLEEP=0.3 run_hook --once >"$tmp/concurrent-1.log" 2>&1 &
first_pid=$!
FAKE_OPENCODE_SLEEP=0.3 run_hook --once >"$tmp/concurrent-2.log" 2>&1 &
second_pid=$!
wait "$first_pid"
wait "$second_pid"
[[ $(calls) -eq $((before + 1)) ]] ||
    fail "two overlapping polls posted run 104 $(($(calls) - before)) times"
pass "two overlapping polls post a new failure only once"

printf 'all ci failure hook checks passed\n'
