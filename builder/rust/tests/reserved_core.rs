// The replay thread's reserved core (host/hos_thread.c; upstream NotRexed/Hookshot a435efd0 #2603 and 262f5614 #2805).
// On Intel's hybrid CPUs the last core is an efficiency core, 3-5x slower for the replay; Windows already chose the last
// performance core (EfficiencyClass), Linux always the last one. This compiles the real pick_reserved_core and
// parse_cpu_list with tests/reserved_core_test.c: the Ryzen, i7-12700K, i9-13900K, Lunar Lake, i5-1235U and i3-1215U
// shapes, the 8-logical/4-fast limits, and the cpulists the kernel's hybrid PMU gives. The source checks cover the
// pieces around them: the Linux /sys/devices/cpu_core/cpus read and HOS_RESERVE_CORE_MASK's validation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A C function's definition, braces matched.
fn block(text: &str, marker: &str) -> String {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + text[start..].find('{').expect("opening brace") + 1;
    let bytes = text.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    text[start..end].to_string()
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

#[test]
fn the_replay_keeps_the_last_performance_core() {
    let repo = root();
    let source = fs::read_to_string(repo.join("host/hos_thread.c")).expect("host/hos_thread.c");
    let pick = block(&source, "static uint64_t pick_reserved_core(");
    let parse = source
        .find("static uint64_t parse_cpu_list(")
        .map(|_| block(&source, "static uint64_t parse_cpu_list("))
        .expect("parse_cpu_list");
    let mut fixture =
        fs::read_to_string(repo.join("tests/reserved_core_test.c")).expect("tests/reserved_core_test.c");
    fixture = fixture.replace("/* PICK_RESERVED_CORE */", &pick);
    fixture = fixture.replace("/* PARSE_CPU_LIST */", &parse);

    // Linux asks the kernel's hybrid PMU for the performance cores, as Windows asks EfficiencyClass, and keeps the
    // last core when there is no such file (AMD, a CPU of one kind, an older kernel).
    let at = source
        .rfind("/sys/devices/cpu_core/cpus")
        .expect("the Linux performance cores");
    let start = source[..at]
        .rfind("static uint64_t last_fastest_core(")
        .expect("Linux last_fastest_core");
    let linux = block(&source[start..], "static uint64_t last_fastest_core(");
    assert!(
        linux.contains("/sys/devices/cpu_core/cpus"),
        "the Linux performance cores are not read from the hybrid PMU"
    );
    assert!(
        linux.contains("parse_cpu_list(line)"),
        "the hybrid PMU's cpulist is not parsed"
    );
    assert!(
        linux.contains("classes[n] = (masks[n] & fast) != 0;"),
        "the performance cores are not given the fast class"
    );
    let reserved = block(&source, "uint64_t hos_reserved_cpus(void) {");
    assert!(
        reserved.contains("#elif !defined(__EMSCRIPTEN__)")
            && reserved.contains("const uint64_t fast = last_fastest_core(&cores);"),
        "Linux does not ask last_fastest_core for the replay's core"
    );
    assert!(
        reserved.contains("if (logical >= 8) mask = last;"),
        "the last-core fallback is gone"
    );

    // HOS_RESERVE_CORE_MASK: hex, one mask, and a bad value is logged and ignored (the normal choice still runs).
    assert!(
        reserved.contains("getenv(\"HOS_RESERVE_CORE_MASK\")"),
        "HOS_RESERVE_CORE_MASK is not read"
    );
    assert!(
        reserved.contains("strtoull(override, &end, 16)"),
        "HOS_RESERVE_CORE_MASK is not parsed as hex"
    );
    assert!(
        reserved.contains("is not a hex mask, ignored"),
        "an invalid HOS_RESERVE_CORE_MASK is not reported"
    );
    let ignored = reserved
        .find("is not a hex mask, ignored")
        .expect("the invalid-mask log");
    let after = &reserved[ignored..];
    assert!(
        after.contains("if (getenv(\"HOS_RESERVE_CORE\") && !atoi(getenv(\"HOS_RESERVE_CORE\"))) return mask;"),
        "an invalid HOS_RESERVE_CORE_MASK disables the reservation instead of being ignored"
    );

    let Some(cc) = compiler() else {
        eprintln!("skipping the reserved core fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-reserved-core-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let src = dir.join("reserved_core_test.c");
    let exe = dir.join(if cfg!(windows) {
        "reserved_core_test.exe"
    } else {
        "reserved_core_test"
    });
    fs::write(&src, &fixture).expect("write the reserved core fixture");
    let build = Command::new(&cc)
        .args(["-std=c11", "-O1", "-Wall", "-Wextra", "-Werror"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C compiler");
    assert!(build.success(), "{cc} failed on {}", src.display());
    let run = Command::new(&exe).output().expect("run the reserved core fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).starts_with("PASS"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}
