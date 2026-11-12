# Local CI failure hook

`scripts/ci_failure_hook.sh` watches `main` of `dazemc/Hookshot-rs` on this machine and posts a short prompt into the
OpenCode session named by `OPENCODE_SESSION_ID` when a workflow run fails. It is a local poller: it opens no port,
registers no webhook and keeps no secret in the repository.

## Start it

From a checkout, with `gh` authenticated for the repository and `opencode` on `PATH`:

```sh
nohup env OPENCODE_SESSION_ID=$OPENCODE_SESSION_ID CI_WATCH_REPO=dazemc/Hookshot-rs \
  scripts/ci_failure_hook.sh >/tmp/opencode/ci-failure-hook.log 2>&1 &
```

`OPENCODE_SESSION_ID` is required. Without it the script logs an error and exits non-zero; it never guesses a session.
The first poll records the failed runs that already exist and posts nothing. Later polls post each new failure once.

`/tmp/opencode` is this machine's scratch directory; use another path if it does not exist. The script keeps its own
state and log under `~/.cache/hookshot-ci-watch` (see below).

## Stop it

Find the process and kill it:

```sh
pgrep -af 'scripts/ci_failure_hook.sh'
kill <pid>
```

Check the command line before killing when more than one checkout runs the hook: `pgrep -af` prints the full command.

## State and log

| Path | What |
|---|---|
| `~/.cache/hookshot-ci-watch/seen` | one failed run `databaseId` per line; a run in this file is never posted again |
| `~/.cache/hookshot-ci-watch/watch.log` | timestamped polls, baselines, posts and errors |
| `~/.cache/hookshot-ci-watch/baselined` | marks that the first baseline ran |
| `~/.cache/hookshot-ci-watch/lock` | `flock` target; overlapping polls cannot double-post |

`CI_WATCH_STATE_DIR` moves the whole directory. The nohup log holds the same lines as `watch.log`, plus anything the
commands print to stderr.

## Options and environment

```sh
OPENCODE_SESSION_ID=ses_... scripts/ci_failure_hook.sh            # poll forever
OPENCODE_SESSION_ID=ses_... scripts/ci_failure_hook.sh --once     # poll once, then exit
OPENCODE_SESSION_ID=ses_... scripts/ci_failure_hook.sh --baseline # record current failures, post nothing, exit
```

| Variable | Default | What |
|---|---|---|
| `OPENCODE_SESSION_ID` | (required) | the session to post into |
| `CI_WATCH_REPO` | `dazemc/Hookshot-rs` | repository to watch |
| `CI_WATCH_BRANCH` | `main` | branch to watch |
| `CI_WATCH_INTERVAL` | `60` | seconds between polls |
| `CI_WATCH_STATE_DIR` | `~/.cache/hookshot-ci-watch` | state and log directory |

A run counts as failed when `gh run list` reports `conclusion` `failure`, `timed_out` or `startup_failure`. The hook
does not fetch logs or artifacts. A post that fails is logged and retried on the next poll; the run is recorded only
after the local OpenCode server accepts the prompt.

## The call it makes

For each new failure it sends the workflow name, run URL and commit SHA with the OpenCode HTTP API through the CLI:

```sh
text='CI failure on main
Workflow: Windows builder
Run: https://github.com/dazemc/Hookshot-rs/actions/runs/43319753054
Commit: 5e1e441c8f620bad53a96955d1b2281352d1d9d9'
payload=$(jq -cn --arg text "$text" '{text: $text}')
opencode api post "/api/session/$OPENCODE_SESSION_ID/prompt" --data "$payload"
```

## Security

This is deliberately local-only. The prompt endpoint can put instructions into the OpenCode session, so a public
webhook or a reachable `POST /api/session/<id>/prompt` would let anyone on the network inject prompts into the agent.
Do not expose the OpenCode server or that endpoint publicly, and do not commit the session ID: the script reads it from
the environment.

The only network access is `gh run list`, authenticated as you on this machine. The hook reads run metadata, not game
files, logs or artifacts, so nothing made from the game reaches it.

## Test

`tests/ci_failure_hook_test.sh` puts fake `gh` and `opencode` binaries on `PATH`; it needs no GitHub credentials, no
OpenCode server and no game data:

```sh
bash tests/ci_failure_hook_test.sh
```

It checks that the first poll baselines without posting, that a new failure posts once with its URL and SHA, that a
second poll does not post it again, that `--baseline` posts nothing, that an unset session exits without calling
`opencode`, and that two overlapping polls post only once. The source-regressions workflow runs the same script on
`ubuntu-24.04`.
