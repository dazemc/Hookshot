// Game-free checks of the AMD/RADV VRAM-import rules ported from upstream: 4 MB imports where the pools have VRAM mirrors
// (#2685), a retired pool whose mirror was refused gives its import back (#2795), the Linux 4500 MB live-import cap measured
// on RADV (#2839), and trimming an import mostly outside every game pool (#2825). The pure headers are compiled and run;
// the wiring in vk_memory.cpp is read. No game, dump or GPU.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiler() -> Option<String> {
    let candidates =
        std::env::var("CXX")
            .ok()
            .into_iter()
            .chain(["c++".into(), "g++".into(), "clang++".into()]);
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

/// Compile one fixture against the tree, returning its output. None when there is no compiler.
fn compile_and_run(name: &str, source: &Path) -> Option<Output> {
    let cxx = compiler()?;
    let dir = scratch(name);
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let built = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(source)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        built.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&exe).output().expect("run the fixture");
    let _ = fs::remove_dir_all(dir);
    Some(run)
}

fn expect_ok(name: &str, run: Option<Output>) {
    let Some(run) = run else {
        eprintln!("skipping {name}: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    assert!(
        run.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains(": ok"),
        "{name}: {}",
        String::from_utf8_lossy(&run.stdout)
    );
}

#[test]
fn imports_are_four_megabytes_where_a_mirror_covers_the_padding() {
    let repo = root();
    expect_ok(
        "import_chunk",
        compile_and_run(
            "amd-import-chunk",
            &repo.join("tests/import_chunk_test.cpp"),
        ),
    );
    let memory = fs::read_to_string(repo.join("gpu/vk/vk_memory.cpp")).expect("vk_memory.cpp");
    for needle in [
        "#include \"import_chunk.h\"",
        "return mkwvk::import_chunk_size::bytes(e, mirror_on());",
        "bool mirror_on();",
    ] {
        assert!(
            memory.contains(needle),
            "vk_memory.cpp no longer has {needle}"
        );
    }
}

#[test]
fn a_refused_mirror_does_not_keep_a_retired_import() {
    let repo = root();
    let memory = fs::read_to_string(repo.join("gpu/vk/vk_memory.cpp")).expect("vk_memory.cpp");
    assert!(
        memory.contains(
            "if ((p.mstate == Pool::MIRROR_NONE || p.mstate == Pool::MIRROR_REFUSED) && !p.inherit_pending &&"
        ),
        "retired_imports_give_back keeps the import of a retired pool whose mirror was refused"
    );
}

#[test]
fn the_linux_live_import_cap_is_the_measured_rule() {
    let repo = root();
    expect_ok(
        "retired_give_back",
        compile_and_run(
            "amd-retired-give-back",
            &repo.join("tests/retired_give_back_test.cpp"),
        ),
    );
    let header =
        fs::read_to_string(repo.join("gpu/vk/retired_give_back.h")).expect("retired_give_back.h");
    for needle in [
        "constexpr uint64_t kSmallBudget = 12ull << 30;",
        "constexpr uint64_t kLiveBudgetCap = 4500ull << 20;",
        "constexpr uint64_t live_budget(uint64_t budget, bool env_pct_set, int env_cap_mb = -1)",
    ] {
        assert!(
            header.contains(needle),
            "retired_give_back.h no longer has {needle}"
        );
    }
    let memory = fs::read_to_string(repo.join("gpu/vk/vk_memory.cpp")).expect("vk_memory.cpp");
    assert!(memory.contains("retired_give_back::live_budget(budget, (env_high && *env_high) || (env_low && *env_low), cap_mb)"));
    assert!(
        memory.contains("const uint64_t want = retired_give_back::want(used, cap, high, low);")
    );
}

#[test]
fn an_import_mostly_outside_the_game_pools_is_trimmed() {
    let repo = root();
    expect_ok(
        "mirror_trim",
        compile_and_run("amd-mirror-trim", &repo.join("tests/test_mirror_trim.cpp")),
    );
    let memory = fs::read_to_string(repo.join("gpu/vk/vk_memory.cpp")).expect("vk_memory.cpp");
    for needle in [
        "#include \"mirror_trim.h\"",
        "bool mirrors_trim() {",
        "void trims_settle() {",
        "mirrors_trim();",
        "trims_settle();",
        "if (!r.trimmed) r.inherit_pending = false;",
        "p.trims = std::max(p.trims, o->second.trims)",
    ] {
        assert!(
            memory.contains(needle),
            "vk_memory.cpp no longer has {needle}"
        );
    }
    let trim = fs::read_to_string(repo.join("gpu/vk/mirror_trim.h")).expect("mirror_trim.h");
    assert!(trim.contains("inline bool worth(uint64_t size, const std::vector<Range>& parts)"));
    assert!(trim.contains("inline uint64_t wait(unsigned trims)"));
}

#[test]
fn radv_on_linux_amd_defaults_to_the_compute_mesh_path() {
    let repo = root();
    let main = fs::read_to_string(repo.join("gpu/vk/vk_main.cpp")).expect("vk_main.cpp");
    assert!(
        main.contains("#ifndef _WIN32\n    if (mesh_mode_used == 0 && dev.props.vendorID == 0x1002) mesh_mode_used = 2;"),
        "Linux AMD no longer defaults to Compute: RADV's native task/mesh path deadlocks (issue 2245)"
    );
    assert!(main.contains("int mesh_mode_used = mesh_mode;"));
}
