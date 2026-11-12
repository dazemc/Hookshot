// Game-free checks of builder/setup/setup.c's archive gate: a setup part's
// zip is scanned before Windows' tar.exe sees it. The scanner is cut out of
// the source and compiled as it is, then run on authored zips (no release,
// game or network). The parts.txt reader is checked beside it: a checksum
// must be a whole line, never a substring of another.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn slice(source: &str, begin: &str, end: &str) -> String {
    let start = source
        .find(begin)
        .unwrap_or_else(|| panic!("missing {begin}"));
    let stop = source[start..]
        .find(end)
        .unwrap_or_else(|| panic!("missing {end}"))
        + start;
    source[start..stop].to_string()
}

fn compiler() -> Option<String> {
    let candidates =
        std::env::var("CC")
            .ok()
            .into_iter()
            .chain(["cc".into(), "gcc".into(), "clang".into()]);
    for candidate in candidates {
        if Command::new(&candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hookshot-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

fn u16(value: u16) -> [u8; 2] {
    value.to_le_bytes()
}

fn u32(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

/// One stored (uncompressed) member: central name, local-header name, bytes,
/// Unix mode in the high half of the external attributes.
fn member<'a>(
    name: &'a str,
    local: &'a str,
    data: &'a [u8],
    mode: u32,
) -> (&'a str, &'a str, &'a [u8], u32) {
    (name, local, data, mode)
}

fn zip(entries: &[(&str, &str, &[u8], u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, local, data, mode) in entries {
        let offset = out.len() as u32;
        let local_name = local.as_bytes();
        let name = name.as_bytes();
        out.extend(u32(0x0403_4b50)); // local header
        out.extend(u16(20)); // version needed
        out.extend(u16(0)); // flags
        out.extend(u16(0)); // stored
        out.extend(u16(0)); // time
        out.extend(u16(0)); // date
        out.extend(u32(0)); // crc (the scanner does not read it)
        out.extend(u32(data.len() as u32));
        out.extend(u32(data.len() as u32));
        out.extend(u16(local_name.len() as u16));
        out.extend(u16(0)); // extra
        out.extend(local_name);
        out.extend(*data);

        central.extend(u32(0x0201_4b50)); // central header
        central.extend(u16(20)); // made by
        central.extend(u16(20)); // needed
        central.extend(u16(0)); // flags
        central.extend(u16(0)); // stored
        central.extend(u16(0)); // time
        central.extend(u16(0)); // date
        central.extend(u32(0)); // crc
        central.extend(u32(data.len() as u32));
        central.extend(u32(data.len() as u32));
        central.extend(u16(name.len() as u16));
        central.extend(u16(0)); // extra
        central.extend(u16(0)); // comment
        central.extend(u16(0)); // disk
        central.extend(u16(0)); // internal attributes
        central.extend(u32(mode << 16));
        central.extend(u32(offset));
        central.extend(name);
    }
    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend(central);
    out.extend(u32(0x0605_4b50)); // end of central directory
    out.extend(u16(0)); // disk
    out.extend(u16(0)); // central directory disk
    out.extend(u16(entries.len() as u16));
    out.extend(u16(entries.len() as u16));
    out.extend(u32(central_size));
    out.extend(u32(central_offset));
    out.extend(u16(0)); // comment
    out
}

#[test]
fn setup_refuses_archive_members_that_leave_the_install_folder() {
    let source =
        fs::read_to_string(root().join("builder/setup/setup.c")).expect("builder/setup/setup.c");
    let scanner = slice(&source, "/* ZIP-SAFETY-BEGIN */", "/* ZIP-SAFETY-END */");
    let Some(cc) = compiler() else {
        eprintln!("skipping the compiled archive checks: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = scratch("setup-archive");
    let fixture = dir.join("archive.c");
    let exe = dir.join(if cfg!(windows) {
        "archive.exe"
    } else {
        "archive"
    });
    let mut text = String::from(
        "#define _POSIX_C_SOURCE 200809L\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n#include <sys/types.h>\n\n",
    );
    text.push_str(&scanner);
    text.push_str(
        r#"
int main(int argc, char **argv) {
    if (argc < 2) return 3;
    if (!strcmp(argv[1], "zip")) {
        if (argc != 3) return 3;
        FILE *f = fopen(argv[2], "rb");
        if (!f) return 2;
        const int ok = zip_members_safe(f);
        fclose(f);
        return ok ? 0 : 1;
    }
    if (!strcmp(argv[1], "parts")) {
        if (argc != 4) return 3;
        return parts_has(argv[2], argv[3]) ? 0 : 1;
    }
    return 3;
}
"#,
    );
    fs::write(&fixture, text).expect("write fixture");
    let build = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(&fixture)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C compiler");
    assert!(build.success(), "{cc} failed on {}", fixture.display());

    let run_zip = |name: &str, bytes: &[u8], want_ok: bool| {
        let path = dir.join(format!("{name}.zip"));
        fs::write(&path, bytes).expect("write zip");
        let run = Command::new(&exe)
            .arg("zip")
            .arg(&path)
            .output()
            .expect("run the archive fixture");
        let ok = run.status.success();
        assert_eq!(
            ok,
            want_ok,
            "{name}: expected {}, got {} ({})",
            if want_ok { "safe" } else { "refused" },
            if ok { "safe" } else { "refused" },
            String::from_utf8_lossy(&run.stderr)
        );
    };

    run_zip(
        "good",
        &zip(&[
            member(
                "HookshotBuilder/HookshotBuilder.exe",
                "HookshotBuilder/HookshotBuilder.exe",
                b"exe",
                0o100755,
            ),
            member(
                "data/flutter_assets/a",
                "data/flutter_assets/a",
                b"asset",
                0o100644,
            ),
            member("data/", "data/", b"", 0o40755),
        ]),
        true,
    );
    run_zip(
        "traversal",
        &zip(&[member("../evil.txt", "../evil.txt", b"x", 0o100644)]),
        false,
    );
    run_zip(
        "backslash",
        &zip(&[member("..\\evil.txt", "..\\evil.txt", b"x", 0o100644)]),
        false,
    );
    run_zip(
        "drive",
        &zip(&[member("C:/evil.txt", "C:/evil.txt", b"x", 0o100644)]),
        false,
    );
    run_zip(
        "absolute",
        &zip(&[member("/evil.txt", "/evil.txt", b"x", 0o100644)]),
        false,
    );
    run_zip(
        "empty-part",
        &zip(&[member("a//evil.txt", "a//evil.txt", b"x", 0o100644)]),
        false,
    );
    run_zip(
        "trailing-space",
        &zip(&[member(".. /evil.txt", ".. /evil.txt", b"x", 0o100644)]),
        false,
    );
    run_zip(
        "symlink",
        &zip(&[member("link", "link", b"../outside", 0o120777)]),
        false,
    );
    run_zip(
        "fifo",
        &zip(&[member("fifo", "fifo", b"", 0o010644)]),
        false,
    );
    run_zip(
        "local-name-differs",
        &zip(&[member("good.txt", "../evil.txt", b"x", 0o100644)]),
        false,
    );

    let sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let parts = |text: &str, want: bool| {
        let run = Command::new(&exe)
            .arg("parts")
            .arg(text)
            .arg(sha)
            .output()
            .expect("run the parts fixture");
        assert_eq!(run.status.success(), want, "{text:?}");
    };
    parts(&format!("x\n{sha}\n"), true);
    parts(&format!("prefix{sha}\n"), false);
    parts(&format!("{sha}0\n"), false);
    parts(sha, true);
    let _ = fs::remove_dir_all(dir);
}
