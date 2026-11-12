// The integrated GPU's texture budget (gpu/vk/igpu_budget.h, gpu/vk/vk_texture.cpp; upstream NotRexed/Hookshot a435efd0
// #2603, with 85a6dde3's Linux build fix). An iGPU's "VRAM" is the system's RAM, so the old fixed 2.5 GB was too much on
// an 8 GB laptop and too little on a 32 GB one. The tiers, the reserve and the driver's allowance are pure arithmetic in
// the header; tests/test_igpu_budget.cpp compiles it for 8/12/16/24/32/48 GB, the unknown-RAM fallback, a low driver
// allowance, RX_VK_VRAM_MB and a discrete GPU. The source checks keep the call site honest: the env override is read
// first and still wins, the tier runs only for an integrated GPU without it, and the system RAM is read once.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

#[test]
fn the_integrated_gpu_budget_follows_the_system_ram_tiers() {
    let repo = root();
    let Some(cxx) = compiler() else {
        eprintln!("skipping the iGPU budget fixture: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-igpu-budget-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) {
        "igpu_budget.exe"
    } else {
        "igpu_budget"
    });
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-O1", "-Wall", "-Wextra", "-Werror"])
        .arg(repo.join("tests/test_igpu_budget.cpp"))
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C++ compiler");
    assert!(
        build.success(),
        "{cxx} failed on tests/test_igpu_budget.cpp"
    );
    let run = Command::new(&exe)
        .output()
        .expect("run the iGPU budget fixture");
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

#[test]
fn the_renderer_keeps_the_vram_override_and_discrete_budgets() {
    let repo = root();
    let source =
        fs::read_to_string(repo.join("gpu/vk/vk_texture.cpp")).expect("gpu/vk/vk_texture.cpp");
    let start = source
        .find("static uint64_t own = 0;")
        .expect("texture_frame's budget");
    let end = source[start..]
        .find("const bool preset_budget")
        .map(|at| start + at)
        .expect("the preset budget after the card's own");
    let budget = &source[start..end];

    // RX_VK_VRAM_MB is read before the card's budget, so it still wins over the tiers and the presets.
    assert!(
        budget.contains("const bool vram_env = getenv(\"RX_VK_VRAM_MB\") != nullptr;"),
        "the VRAM override is not read before the budget"
    );
    assert!(
        budget.contains("vram_env ? (uint64_t)atoi(getenv(\"RX_VK_VRAM_MB\")) << 20"),
        "RX_VK_VRAM_MB no longer sets the budget itself"
    );
    // The tiers run only for an integrated GPU and only when the override is unset.
    assert!(
        budget.contains(
            "if (!vram_env && dev.props.deviceType == VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU)"
        ),
        "the system RAM tiers are not kept to an integrated GPU without RX_VK_VRAM_MB"
    );
    assert!(
        budget.contains("igpu_budget::choose(system_ram, driver_budget, false, true)"),
        "the integrated GPU's budget does not use the tested tiers"
    );
    // The system RAM is read once, not every tenth frame (the old block re-read /proc/meminfo).
    assert!(
        budget.contains("static const uint64_t system_ram = system_ram_bytes();"),
        "the system RAM is not read once"
    );
    // The platform reads: Windows' GlobalMemoryStatusEx and Linux's /proc/meminfo, no game data.
    let read = source
        .find("static uint64_t system_ram_bytes()")
        .expect("system_ram_bytes");
    let read_end = source[read..]
        .find("\n}\n")
        .map(|at| read + at)
        .expect("system_ram_bytes' end");
    let read = &source[read..read_end];
    assert!(
        read.contains("GlobalMemoryStatusEx"),
        "Windows' RAM read is missing"
    );
    assert!(
        read.contains("/proc/meminfo"),
        "Linux's RAM read is missing"
    );
    assert!(read.contains("MemTotal:"), "Linux's MemTotal is not read");

    // The header is pure: no Vulkan, no Windows, no environment.
    let header =
        fs::read_to_string(repo.join("gpu/vk/igpu_budget.h")).expect("gpu/vk/igpu_budget.h");
    for forbidden in [
        "<windows.h>",
        "<vulkan",
        "getenv(",
        "GlobalMemoryStatusEx",
        "/proc/meminfo",
    ] {
        assert!(
            !header.contains(forbidden),
            "igpu_budget.h should stay pure, but has {forbidden}"
        );
    }
}
