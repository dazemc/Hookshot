// Game-free checks of the pipeline and program caches: the file-backed pipeline cache (upstream fc5ab981), the
// foreign-translator stamp (f5c9acc1), the bounded wait for translating shaders (65a4d874) and the zstd program cache
// (136452cd). The C++ fixtures under tests/ compile and run when a compiler is present; the source checks read the
// back end's wiring. No game, dump or GPU is needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// A C/C++ definition's braces, matched, from its first line.
fn function(text: &str, marker: &str) -> String {
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

fn index(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"))
}

fn compiler(env: &str, candidates: &[&str]) -> Option<String> {
    let candidates = std::env::var(env)
        .ok()
        .into_iter()
        .chain(candidates.iter().map(|s| s.to_string()));
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

fn compile(
    name: &str,
    source: &Path,
    includes: &[PathBuf],
    extra: &[&str],
    link: &[PathBuf],
) -> Option<PathBuf> {
    let cxx = compiler("CXX", &["c++", "g++", "clang++"])?;
    let dir = scratch(name);
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut build = Command::new(&cxx);
    build.args(["-std=c++20", "-Wall", "-Wextra", "-Werror"]);
    for include in includes {
        build.arg(format!("-I{}", include.display()));
    }
    build.arg(source);
    for object in link {
        build.arg(object);
    }
    build.args(extra);
    if !cfg!(windows) {
        build.arg("-pthread");
    }
    build.arg("-o").arg(&exe);
    let built = build.output().expect("run the C++ compiler");
    assert!(
        built.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    Some(exe)
}

fn compile_and_run(
    name: &str,
    source: &Path,
    includes: &[PathBuf],
    extra: &[&str],
    link: &[PathBuf],
    argument: Option<&Path>,
) -> Option<Output> {
    let exe = compile(name, source, includes, extra, link)?;
    let mut run = Command::new(&exe);
    if let Some(argument) = argument {
        run.arg(argument);
    }
    let output = run.output().expect("run the fixture");
    let _ = fs::remove_dir_all(exe.parent().unwrap());
    Some(output)
}

fn run_fixture(name: &str, relative: &str) -> Option<Output> {
    let repo = root();
    let dir = scratch(&format!("{name}-dir"));
    let output = compile_and_run(name, &repo.join(relative), &[], &[], &[], Some(&dir));
    let _ = fs::remove_dir_all(&dir);
    Some(output?)
}

#[test]
fn the_pipeline_cache_is_read_and_written_without_full_size_heap_copies() {
    let Some(run) = run_fixture("pipeline-cache-file", "tests/pipeline_cache_file_test.cpp") else {
        eprintln!("skipping the pipeline cache file fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("\"mappedRead\":true"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );

    // The production cache paths use the file-backed helper, not a cache-sized vector; the start sweeps old temporaries.
    let draw = read("gpu/vk/vk_draw.cpp");
    let read_fn = function(&draw, "PipelineCacheFile pcache_file_read(");
    assert!(read_fn.contains("PipelineCacheFile::read"));
    assert!(!read_fn.contains("std::vector<uint8_t>"));
    let write_fn = function(&draw, "size_t play_file_write(");
    assert!(write_fn.contains("PipelineCacheFile::writable"));
    assert!(!write_fn.contains("std::vector<uint8_t>"));
    let save_fn = function(&draw, "void pipeline_cache_save(bool force)");
    assert!(save_fn.contains("PipelineCacheFile::writable"));
    assert!(!save_fn.contains("std::vector<uint8_t>"));
    let init = function(&draw, "void pcache_init()");
    assert!(init.contains("PipelineCacheFile::cleanup_temporaries"));
    // A play file of another driver or a failed creation is removed only after its view is released.
    let merge = function(&draw, "size_t play_files_merge()");
    assert!(merge.contains("part.rejected()"));
    assert!(merge.contains("part.reset()"));
}

#[test]
fn a_native_io_failure_keeps_the_old_pipeline_cache() {
    // (upstream's fixture: every create, map, flush, close and replace refusal around the same production helper; the old
    // cache stays readable, a temporary is cleaned, and a read failure is unavailable, not another translator's file)
    let repo = root();
    let dir = scratch("pipeline-cache-io-fault-dir");
    let run = compile_and_run(
        "pipeline-cache-io-fault",
        &repo.join("tests/pipeline_cache_io_fault_test.cpp"),
        &[repo.join("gpu/vk")],
        &[],
        &[],
        Some(&dir),
    );
    let _ = fs::remove_dir_all(&dir);
    let Some(run) = run else {
        eprintln!("skipping the pipeline cache I/O fault fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("\"oldOutputPreserved\":true"), "{stdout}");
    assert!(stdout.contains("\"retryPassed\":true"), "{stdout}");
    assert!(stdout.contains("\"ownedTemporariesCleaned\":true"), "{stdout}");
    assert!(
        stdout.contains("\"readIoFailureNotIncompatible\":true"),
        "{stdout}"
    );
}

#[test]
fn a_pipeline_cache_header_is_checked_before_the_file_is_mapped() {
    let Some(run) = run_fixture("pcache-read", "tests/pcache_read_test.cpp") else {
        eprintln!("skipping the pipeline cache header fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let header = read("gpu/vk/pcache_read.h");
    assert!(header.contains("inline bool header_ok("));
    assert!(header.contains("VK_PIPELINE_CACHE_HEADER_VERSION_ONE"));
    assert!(header.contains("head_size <= size"));
}

#[test]
fn another_translators_pipeline_cache_files_are_dropped() {
    let Some(run) = run_fixture("pcache-stamp", "tests/pcache_stamp_test.cpp") else {
        eprintln!("skipping the pipeline cache stamp fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let draw = read("gpu/vk/vk_draw.cpp");
    let init = function(&draw, "void pcache_init()");
    assert!(
        index(&init, "pcache_drop_stale()") < index(&init, "pcache_file_read(pcache_path)"),
        "pcache_init must drop another translator's files before it reads them"
    );
    let drop = function(&draw, "bool pcache_drop_stale()");
    assert!(drop.contains("pcache_stamp::check(dir, program_cache_stamp())"));
    assert!(drop.contains("RX_VK_PCACHE_STAMP"));
    assert!(drop.contains("pcache_load_off()"));
    // The warm pass's stage libraries were in those files: the prepare makes them again after a drop.
    let prepare = function(&draw, "bool prepare_run(");
    assert!(prepare.contains("pcache_drop_stale()"));
    assert!(prepare.contains("prepared_for.warmed = 0"));
}

#[test]
fn the_first_start_builds_the_listed_pipelines_itself() {
    // This fork predates RX_VK_LOAD=create and its hookshot_pipelines.bin.built marker (upstream #2760, #2865, #2869):
    // precompile_run always builds every noted pipeline before play, so there is no preparation marker to trust and the
    // first start after a preparation does the build itself. Check that wiring, and that play begins after it.
    let draw = read("gpu/vk/vk_draw.cpp");
    let precompile = function(&draw, "void precompile_run(");
    assert!(precompile.contains("pipe_notes_read()"));
    assert!(precompile.contains("build_graphics("));
    assert!(precompile.contains("program_cache_load_some("));
    let main = read("gpu/vk/vk_main.cpp");
    assert!(
        index(&main, "precompile_run(progress);") < index(&main, "pipeline_cache_play();"),
        "the listed pipelines are built before play begins"
    );
    // The prepare's driver part warms compute pipelines into the cache too, as a first start in start mode does.
    let prepare = function(&draw, "bool prepare_run(");
    assert!(prepare.contains("guarded_compute(pcache"));
    assert!(prepare.contains("pcache_built_since_save++"));
}

#[test]
fn a_translating_shader_wait_can_be_bounded() {
    let Some(run) = run_fixture("translate-wait", "tests/translate_wait_test.cpp") else {
        eprintln!("skipping the translate wait fixture: no C++ compiler");
        return;
    };
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let jobs = read("gpu/vk/vk_jobs.cpp");
    let wait_for = function(&jobs, "bool jobs_wait_for(");
    assert!(wait_for.contains("done_cv.wait_for("));
    assert!(
        !wait_for.contains("run_guarded"),
        "the bounded wait must not run the queued job on the waiting thread"
    );
    let draw = read("gpu/vk/vk_draw.cpp");
    let draw_fn = function(
        &draw,
        "void draw(VkCommandBuffer cmd, NvnState& s, const DrawArgs& d)",
    );
    assert!(draw_fn.contains("RX_VK_TRANSLATE_WAIT_MS"));
    assert!(draw_fn.contains("translate_wait::parse("));
    assert!(draw_fn.contains("jobs_wait_for(cp->ready, budget_left)"));
    assert!(draw_fn.contains("if (!ready) return void(st_pending_skips++)"));
    assert!(
        draw_fn.contains("wait_for(cp->ready)"),
        "the default still waits every translation out"
    );
}

#[test]
fn the_program_cache_compresses_records_with_zstd_and_can_be_switched_off() {
    let repo = root();
    let Some(cc) = compiler("CC", &["cc", "gcc", "clang"]) else {
        eprintln!("skipping the zstd program cache fixture: no C compiler");
        return;
    };
    let Some(cxx) = compiler("CXX", &["c++", "g++", "clang++"]) else {
        eprintln!("skipping the zstd program cache fixture: no C++ compiler");
        return;
    };
    let dir = scratch("program-cache-zstd");
    let zbic = repo.join("third_party/zbic");
    let object = dir.join(if cfg!(windows) { "zstd.obj" } else { "zstd.o" });
    let mut build_zstd = Command::new(&cc);
    build_zstd
        .arg("-c")
        .arg(zbic.join("zstd.c"))
        .args(["-O1", "-DZSTD_ZBIC_SUPPORT=0", "-Wno-everything"])
        .arg(format!("-I{}", zbic.display()))
        .arg("-o")
        .arg(&object);
    let built = build_zstd.output().expect("run the C compiler");
    assert!(
        built.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut build = Command::new(&cxx);
    build
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(format!("-I{}", zbic.display()))
        .arg(repo.join("tests/program_cache_zstd_test.cpp"))
        .arg(&object)
        .arg("-o")
        .arg(&exe);
    let built = build.output().expect("run the C++ compiler");
    assert!(
        built.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&exe).output().expect("run the fixture");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let cache = read("gpu/vk/vk_progcache.cpp");
    assert!(cache.contains("disk_zstd = env_on(\"RX_VK_PROGRAM_CACHE_ZSTD\")"));
    assert!(cache.contains("hookshot_programs.zst.bin"));
    assert!(cache.contains("kDiskMagicZstd"));
    // The uncompressed format is unchanged and remains the default.
    assert!(cache.contains("constexpr uint32_t kDiskMagic = 0x31505348;"));
    assert!(cache.contains("constexpr uint32_t kProgramCacheFormat = 1;"));
    let save = function(&cache, "void program_cache_save(");
    assert!(save.contains("ZSTD_compress("));
    assert!(save.contains("compressed_ok ? kCompressedKind : 0"));
    for marker in ["void program_cache_read(", "void program_cache_each("] {
        assert!(
            function(&cache, marker).contains("program_cache_payload("),
            "{marker} reads its record through the payload helper"
        );
    }
    let shader = read("gpu/vk/vk_shader.cpp");
    assert!(
        function(&shader, "SeedVariants seed_compute(").contains("program_cache_payload("),
        "the prepare's compute variants read compressed records too"
    );
    let build = read("nx2/build.py");
    assert!(build.contains("third_party\", \"zbic"));
    assert!(build.contains("-DZSTD_ZBIC_SUPPORT=0"));
    assert!(
        !build.contains("zstd_jobs(cc, zflags, obj_dir, \"vk_zstd_\")"),
        "the back end builds zbic's zstd once, not third_party/zstd as well"
    );
}
