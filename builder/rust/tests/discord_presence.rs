// Discord Rich Presence's connection is deliberately off the game's thread:
// one low-priority host thread owns it, writes the activity as its keep-alive,
// and reconnects on a timer when Discord is not there or the connection breaks.
// Upstream NotRexed/Hookshot has no change to host/discord_rpc.c since this
// fork's base, so this is a regression guard for what was already present:
// reconnect/backoff, the keep-alive, Ping/Pong, and the detached thread.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn discord_reconnects_on_a_thread_of_its_own() {
    let source = fs::read_to_string(root().join("host/discord_rpc.c")).expect("host/discord_rpc.c");

    // A thread, never the game's: Windows asks for a low-priority thread, Linux detaches one.
    assert!(
        source.contains("CreateThread(NULL, 256u << 10, presence_thread, NULL, 0, NULL)"),
        "the Windows presence thread is missing"
    );
    assert!(
        source.contains("THREAD_PRIORITY_LOWEST"),
        "the Windows presence thread is not low priority"
    );
    assert!(
        source.contains("pthread_create(&t, NULL, presence_thread, NULL)"),
        "the Linux presence thread is missing"
    );
    assert!(
        source.contains("pthread_detach(t)"),
        "the Linux presence thread is not detached"
    );
    let start = source
        .find("void hos_discord_start(void)")
        .expect("hos_discord_start");
    assert!(
        !source[start..].contains("presence_main();"),
        "the game thread must not run the presence loop itself"
    );

    // No client, a broken pipe or a refused status: try again in 15 s. A live
    // connection writes the activity every 60 s, which is the keep-alive.
    assert!(
        source.matches("retry_at = now_ms() + 15000;").count() >= 2,
        "the reconnect delay is not 15 s"
    );
    assert!(
        source.contains("send_at = now_ms() + 60000;"),
        "the 60 s keep-alive is missing"
    );
    assert!(
        source.contains("the connection broke; connecting again"),
        "a broken connection is not logged and retried"
    );

    // Discord's Ping is answered with a Pong so the client keeps the pipe open.
    assert!(source.contains("OP_PING"), "the Ping opcode is missing");
    assert!(source.contains("OP_PONG"), "the Pong opcode is missing");
    assert!(
        source.contains("write_frame(p, OP_PONG, buf)"),
        "a Ping is not answered with a Pong"
    );
}
