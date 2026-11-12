//! NVIDIA's DLSS SDK: the files a build reads and the runtime the game
//! loads, ported from scripts/ngx_sdk.py. NVIDIA's files are downloaded from
//! NVIDIA's repository, verified by SHA-256, and stay on this PC.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BASE: &str =
    "https://raw.githubusercontent.com/NVIDIA/DLSS/9981c6350dc58c0ce21f024fd65e31e851c31689";
const LICENSE_URL: &str =
    "https://github.com/NVIDIA/DLSS/blob/9981c6350dc58c0ce21f024fd65e31e851c31689/LICENSE.txt";

const COMMON: [(&str, &str); 24] = [
    (
        "LICENSE.txt",
        "d4216e39ebef5f9b50a6712ebb37beeb5379862a67733a9999c651f21592aaf0",
    ),
    (
        "include/nvsdk_ngx_defs_dlssd.h",
        "2e97fe1595b1334f71b51724f2cb67496e8c296a7a2eec5606da53055882e716",
    ),
    (
        "include/nvsdk_ngx_defs_dlssg.h",
        "c29765ab9e0125f9d724ae9811cc2abe03738d17486ec8eee80d78a1504549e1",
    ),
    (
        "include/nvsdk_ngx_defs.h",
        "ea23f33497cd274860d1c25a97644fce807dcb0037c594547203343103fad03e",
    ),
    (
        "include/nvsdk_ngx_defs_vk.h",
        "0a24d0861ace7d6b9362a67b7f08bea1b33ea123c5ce730b19133de8a891d031",
    ),
    (
        "include/nvsdk_ngx.h",
        "dc38e7467cf415379c9d12ae1b6e4a494c453ed92720fb53e92aecb523e7b848",
    ),
    (
        "include/nvsdk_ngx_helpers_cuda.h",
        "f2851a76107bdf4fbcb7b261f3c35573c312d8ecb4bc18fb02d9e2cec7705042",
    ),
    (
        "include/nvsdk_ngx_helpers_d3d.h",
        "ec75224f36ed6580baaf250fa83620405ad81a1d98c2c436654a0c9cf6a6b8ba",
    ),
    (
        "include/nvsdk_ngx_helpers_dlssd_cuda.h",
        "7e35cde214d905346a487b9a28c05d35e1c55b480e237fe3de69580a7e332730",
    ),
    (
        "include/nvsdk_ngx_helpers_dlssd_d3d.h",
        "6df9d02c6f47feee3afc8d12340a9f554a7ddce4ddacb1b6a13a42b26c2c6293",
    ),
    (
        "include/nvsdk_ngx_helpers_dlssd.h",
        "a25fdda925b479dfe869593aeba311ef08793da54e0391f2fe2fe756cbef5532",
    ),
    (
        "include/nvsdk_ngx_helpers_dlssd_vk.h",
        "11df8fbda532a97fe867c8ef2b40058b49b7898183710f3468e55b1eedb8f40b",
    ),
    (
        "include/nvsdk_ngx_helpers_dlssg_d3d.h",
        "6335ebcb95de39a361e87fe0257d6ce0723e5d8deaa15db3b35afacf2b3f4220",
    ),
    (
        "include/nvsdk_ngx_helpers_dlssg.h",
        "6881d21419c40c78ec948b326902ece09fc112b0223144d51c4905f3fced7a1f",
    ),
    (
        "include/nvsdk_ngx_helpers_dlssg_vk.h",
        "b371d90b9f4372b9a415e68a0afbdc2f31b7b552e219a9637d848749bc965316",
    ),
    (
        "include/nvsdk_ngx_helpers.h",
        "5bcbadfe7478b802cf6d3aca4dc5ddd7d0889b99726e69c63f9e9bd555f44471",
    ),
    (
        "include/nvsdk_ngx_helpers_vk.h",
        "c192bff72138f12f770db48e12c1d8f712dfc81ea0e8e055ebb27d8dd1b31623",
    ),
    (
        "include/nvsdk_ngx_loader.h",
        "abdf5f66a692620538357d29fdf78e3e2a677ee8140b7a638962ecb766585e75",
    ),
    (
        "include/nvsdk_ngx_params_dlssd.h",
        "d41e3ba8f1d72583b55273092c99bf426dc4a0d46de827229e25d5b1325a370e",
    ),
    (
        "include/nvsdk_ngx_params_dlssg.h",
        "c24fa5aa68ffa2808c828b75cfa32843f1606f80a145f84055ac450d7e6c74ad",
    ),
    (
        "include/nvsdk_ngx_params.h",
        "943bc8cc5cdae03b6303016fbad3183636f2335ae27a2d18776798c3b4efabbc",
    ),
    (
        "include/nvsdk_ngx_standalone_common.h",
        "38413c3338b0aea0202c07da95d426bfee7080a42913823fa2d9dd585b087b53",
    ),
    (
        "include/nvsdk_ngx_standalone_cuda.h",
        "b23436ed5040051ac8e81554c9af080f2463c941435bbc3b82222e5431a09499",
    ),
    (
        "include/nvsdk_ngx_vk.h",
        "2d364ce7132881eb669e9498fd570d74cba563b1fbebc82c235f8e1ad2dd8b6d",
    ),
];

const LINUX_LIBRARY: (&str, &str) = (
    "lib/Linux_x86_64/libnvsdk_ngx.a",
    "d3c50bb0d9624fc35e502a8e929b8fc1e2b505017ebdb84c0d5b627e11a235df",
);
const LINUX_RUNTIME: (&str, &str) = (
    "lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.310.9.16",
    "30982c937afed85c96b8d313323de9a650a2eec6de1bb0f714b21750f4ce98f6",
);
const LINUX_FRAMEGEN: (&str, &str) = (
    "lib/Linux_x86_64/rel/libnvidia-ngx-dlssg.so.310.9.16",
    "574839cae01c835de4d5845c36ca9cf75361f8160bd52397142c36a24e29e6f7",
);
const WINDOWS_LIBRARY: (&str, &str) = (
    "lib/Windows_x86_64/x64/nvsdk_ngx_s.lib",
    "7b9a5239c1145af5f645ab472aa066f9ea79db36ac819562312aae69d65f3765",
);
const WINDOWS_RUNTIME: (&str, &str) = (
    "lib/Windows_x86_64/rel/nvngx_dlss.dll",
    "d00b5b0ab29c3ca101b70c3a5ecb3a01b7ef33766a3383a28a18623083e6123e",
);
const WINDOWS_FRAMEGEN: (&str, &str) = (
    "lib/Windows_x86_64/rel/nvngx_dlssg.dll",
    "8d59d49b36410dd6b62fef2503a5871d347415bd85d943fa0848563a95dd50c7",
);

/// The two runtimes NGX loads from the game folder: DLSS's and DLSS Frame
/// Generation's. Without the latter beside the former, NGX takes the
/// driver's own copy, which may be older (one that makes a single frame, or
/// refuses every set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeKind {
    Dlss,
    FrameGeneration,
}

impl RuntimeKind {
    pub const ALL: [Self; 2] = [Self::Dlss, Self::FrameGeneration];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dlss => "dlss",
            Self::FrameGeneration => "framegen",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows)
            || std::env::var("RX_TARGET").is_ok_and(|v| v.eq_ignore_ascii_case("windows"))
        {
            Self::Windows
        } else {
            Self::Linux
        }
    }
    fn library(self) -> (&'static str, &'static str) {
        if self == Self::Windows {
            WINDOWS_LIBRARY
        } else {
            LINUX_LIBRARY
        }
    }
    fn runtime(self, kind: RuntimeKind) -> (&'static str, &'static str) {
        match (self, kind) {
            (Self::Windows, RuntimeKind::Dlss) => WINDOWS_RUNTIME,
            (Self::Windows, RuntimeKind::FrameGeneration) => WINDOWS_FRAMEGEN,
            (Self::Linux, RuntimeKind::Dlss) => LINUX_RUNTIME,
            (Self::Linux, RuntimeKind::FrameGeneration) => LINUX_FRAMEGEN,
        }
    }
    pub fn runtime_name(self, kind: RuntimeKind) -> &'static str {
        self.runtime(kind).0.rsplit('/').next().unwrap_or_default()
    }
}

/// Where a file of NVIDIA's repository goes in the SDK folder.
pub fn destination(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("lib/") {
        let name = rest.rsplit('/').next().unwrap_or(rest);
        if path.contains("/rel/") {
            format!("runtime/{name}")
        } else {
            format!("lib/{name}")
        }
    } else {
        path.to_owned()
    }
}

/// NVIDIA's static library for the build (`lib/libnvsdk_ngx.a` on Linux,
/// `lib/nvsdk_ngx_s.lib` on Windows).
pub fn library(sdk: &Path, platform: Platform) -> PathBuf {
    sdk.join(destination(platform.library().0))
}

/// A runtime the game folder carries: DLSS's
/// (`runtime/libnvidia-ngx-dlss.so.*` or `runtime/nvngx_dlss.dll`) or DLSS
/// Frame Generation's (`runtime/libnvidia-ngx-dlssg.so.*` or
/// `runtime/nvngx_dlssg.dll`).
pub fn runtime(sdk: &Path, platform: Platform, kind: RuntimeKind) -> PathBuf {
    sdk.join(destination(platform.runtime(kind).0))
}

fn files(platform: Platform, with_runtime: bool) -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = COMMON.to_vec();
    out.push(platform.library());
    if with_runtime {
        out.extend(RuntimeKind::ALL.map(|kind| platform.runtime(kind)));
    }
    out
}

fn sha256(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut digest = Sha256::new();
    digest.update(&data);
    Ok(format!("{:x}", digest.finalize()))
}

fn fetch_one(path: &str, digest: &str, sdk: &Path) -> Result<bool, String> {
    let dst = sdk.join(destination(path));
    if dst.is_file() && sha256(&dst)?.eq_ignore_ascii_case(digest) {
        return Ok(false);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let part = dst.with_extension(format!(
        "{}.part",
        dst.extension().and_then(|e| e.to_str()).unwrap_or("file")
    ));
    let url = format!("{BASE}/{path}");
    let mut easy = curl::easy::Easy::new();
    easy.url(&url).map_err(|e| e.to_string())?;
    easy.follow_location(true).map_err(|e| e.to_string())?;
    easy.timeout(Duration::from_secs(60))
        .map_err(|e| e.to_string())?;
    {
        let mut file = File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut transfer = easy.transfer();
        transfer
            .write_function(move |data| {
                file.write_all(data)
                    .map(|_| data.len())
                    .map_err(|_| curl::easy::WriteError::Pause)
            })
            .map_err(|e| e.to_string())?;
        transfer.perform().map_err(|e| {
            let _ = fs::remove_file(&part);
            format!("{url}: {e}")
        })?;
    }
    let code = easy.response_code().map_err(|e| e.to_string())?;
    if code != 200 {
        let _ = fs::remove_file(&part);
        return Err(format!("{url}: HTTP {code}"));
    }
    let got = sha256(&part)?;
    if !got.eq_ignore_ascii_case(digest) {
        let _ = fs::remove_file(&part);
        return Err(format!("{url}: SHA-256 {got}, expected {digest}"));
    }
    fs::rename(&part, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(true)
}

/// Download every file the platform's build reads (and its runtime when
/// asked), verifying each SHA-256. Returns the files written and reused.
pub fn fetch(sdk: &Path, platform: Platform, with_runtime: bool) -> Result<Value, String> {
    fs::create_dir_all(sdk).map_err(|e| format!("{}: {e}", sdk.display()))?;
    let mut written = Vec::new();
    let mut reused = Vec::new();
    for (path, digest) in files(platform, with_runtime) {
        if fetch_one(path, digest, sdk)? {
            written.push(destination(path));
        } else {
            reused.push(destination(path));
        }
    }
    Ok(json!({
        "sdk": sdk.to_string_lossy(),
        "platform": if platform == Platform::Windows { "windows" } else { "linux" },
        "written": written,
        "reused": reused,
        "notice": format!("NVIDIA DLSS: NVIDIA's files, under NVIDIA's RTX SDKs license ({LICENSE_URL}); they stay on this PC."),
    }))
}

/// Copy the SDK's runtimes into the game folder's ngx directory: DLSS's and,
/// when the SDK folder has it, DLSS Frame Generation's. An SDK folder fetched
/// before frame generation's runtime was in the table still installs DLSS's;
/// the next fetch adds the other.
pub fn install_runtime(sdk: &Path, platform: Platform, folder: &Path) -> Result<Value, String> {
    let dlss = runtime(sdk, platform, RuntimeKind::Dlss);
    if !dlss.is_file() {
        return Ok(json!({"placed": false, "reason": format!("{} is missing", dlss.display())}));
    }
    fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut runtimes = Vec::new();
    let mut missing = Vec::new();
    let mut dlss_path = PathBuf::new();
    let mut dlss_reused = false;
    let mut framegen_path: Option<PathBuf> = None;
    for kind in RuntimeKind::ALL {
        let src = runtime(sdk, platform, kind);
        let name = platform.runtime_name(kind);
        if !src.is_file() {
            missing.push(name);
            continue;
        }
        let dst = folder.join(name);
        let reused = dst.is_file()
            && fs::metadata(&dst)
                .map(|meta| meta.len())
                .unwrap_or(u64::MAX)
                == fs::metadata(&src).map(|meta| meta.len()).unwrap_or(0)
            && sha256(&dst)? == sha256(&src)?;
        if !reused {
            let part = folder.join(format!("{name}.part"));
            fs::copy(&src, &part).map_err(|e| format!("{}: {e}", part.display()))?;
            fs::rename(&part, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        }
        if kind == RuntimeKind::Dlss {
            dlss_path = dst.clone();
            dlss_reused = reused;
        } else {
            framegen_path = Some(dst.clone());
        }
        runtimes.push(json!({
            "kind": kind.label(),
            "path": dst.to_string_lossy(),
            "reused": reused,
        }));
    }
    if dlss_path.as_os_str().is_empty() {
        return Ok(json!({"placed": false, "reason": format!("{} is missing", dlss.display())}));
    }
    Ok(json!({
        "placed": true,
        "path": dlss_path.to_string_lossy(),
        "reused": dlss_reused,
        "framegen": framegen_path.as_ref().map(|path| path.to_string_lossy().into_owned()),
        "runtimes": runtimes,
        "missing": missing,
    }))
}

/// Remove both of NGX's runtimes from a game folder's ngx directory and the
/// directory when it is empty: a folder rebuilt without DLSS loses them too.
pub fn remove_runtimes(folder: &Path, platform: Platform) -> bool {
    let mut removed = false;
    for kind in RuntimeKind::ALL {
        let path = folder.join(platform.runtime_name(kind));
        if path.is_file() && fs::remove_file(&path).is_ok() {
            removed = true;
        }
    }
    let _ = fs::remove_dir(folder);
    removed
}

pub fn fetch_json(request: &Value) -> Result<Value, String> {
    let sdk = request
        .get("sdk")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if sdk.is_empty() {
        return Err("build.ngx_fetch needs the SDK folder".into());
    }
    let runtime = request
        .get("runtime")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    fetch(Path::new(sdk), Platform::current(), runtime)
}

pub fn install_json(request: &Value) -> Result<Value, String> {
    let sdk = request
        .get("sdk")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let folder = request
        .get("folder")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if sdk.is_empty() || folder.is_empty() {
        return Err("build.ngx_install needs the SDK folder and the game folder".into());
    }
    install_runtime(Path::new(sdk), Platform::current(), Path::new(folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-ngx-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn sdk_runtimes(sdk: &Path, platform: Platform) {
        for kind in RuntimeKind::ALL {
            let path = runtime(sdk, platform, kind);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"runtime").unwrap();
        }
    }

    #[test]
    fn destinations_match_the_python_layout() {
        assert_eq!(destination("include/nvsdk_ngx.h"), "include/nvsdk_ngx.h");
        assert_eq!(destination(LINUX_LIBRARY.0), "lib/libnvsdk_ngx.a");
        assert_eq!(
            destination(LINUX_RUNTIME.0),
            "runtime/libnvidia-ngx-dlss.so.310.9.16"
        );
        assert_eq!(
            destination(LINUX_FRAMEGEN.0),
            "runtime/libnvidia-ngx-dlssg.so.310.9.16"
        );
        assert_eq!(destination(WINDOWS_RUNTIME.0), "runtime/nvngx_dlss.dll");
        assert_eq!(destination(WINDOWS_FRAMEGEN.0), "runtime/nvngx_dlssg.dll");
    }

    #[test]
    fn every_file_has_a_sha256_and_both_runtimes_are_in_the_table() {
        for (platform, names) in [
            (
                Platform::Linux,
                (
                    "libnvidia-ngx-dlss.so.310.9.16",
                    "libnvidia-ngx-dlssg.so.310.9.16",
                ),
            ),
            (Platform::Windows, ("nvngx_dlss.dll", "nvngx_dlssg.dll")),
        ] {
            let table = files(platform, true);
            for (path, digest) in &table {
                assert!(!path.is_empty());
                assert_eq!(digest.len(), 64, "{path}");
                assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()), "{path}");
            }
            for (kind, name) in [
                (RuntimeKind::Dlss, names.0),
                (RuntimeKind::FrameGeneration, names.1),
            ] {
                assert_eq!(platform.runtime_name(kind), name);
                assert!(
                    table
                        .iter()
                        .any(|(path, _)| destination(path) == format!("runtime/{name}")),
                    "{name} is not fetched"
                );
            }
            // (the build-only set leaves both runtimes for the fetch)
            assert!(
                !files(platform, false)
                    .iter()
                    .any(|(path, _)| path.contains("/rel/"))
            );
        }
    }

    #[test]
    fn install_places_both_runtimes_and_reuses_them() {
        let base = temp("install");
        let platform = Platform::Linux;
        let sdk = base.join("sdk");
        let folder = base.join("game/ngx");
        sdk_runtimes(&sdk, platform);
        let answer = install_runtime(&sdk, platform, &folder).unwrap();
        assert_eq!(answer["placed"], true);
        assert_eq!(answer["reused"], false);
        assert!(answer["missing"].as_array().unwrap().is_empty());
        assert!(
            folder
                .join(platform.runtime_name(RuntimeKind::Dlss))
                .is_file()
        );
        assert!(
            folder
                .join(platform.runtime_name(RuntimeKind::FrameGeneration))
                .is_file()
        );
        assert_eq!(
            Path::new(answer["framegen"].as_str().unwrap()),
            folder.join(platform.runtime_name(RuntimeKind::FrameGeneration))
        );
        let again = install_runtime(&sdk, platform, &folder).unwrap();
        assert_eq!(again["reused"], true);
        assert!(
            again["runtimes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|runtime| runtime["reused"] == true)
        );
        // (an older SDK folder still installs DLSS's, and says what is missing)
        fs::remove_file(runtime(&sdk, platform, RuntimeKind::FrameGeneration)).unwrap();
        let older = install_runtime(&sdk, platform, &folder).unwrap();
        assert_eq!(older["placed"], true);
        assert_eq!(older["framegen"], Value::Null);
        assert_eq!(
            older["missing"],
            json!([platform.runtime_name(RuntimeKind::FrameGeneration)])
        );
        // (a folder rebuilt without DLSS loses both and the directory)
        assert!(remove_runtimes(&folder, platform));
        assert!(!folder.exists());
        assert!(!remove_runtimes(&folder, platform));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn windows_runtime_names_are_the_dlls_the_game_loads() {
        assert_eq!(
            Platform::Windows.runtime_name(RuntimeKind::Dlss),
            "nvngx_dlss.dll"
        );
        assert_eq!(
            Platform::Windows.runtime_name(RuntimeKind::FrameGeneration),
            "nvngx_dlssg.dll"
        );
        assert_eq!(
            Platform::Linux.runtime_name(RuntimeKind::Dlss),
            "libnvidia-ngx-dlss.so.310.9.16"
        );
        assert_eq!(
            Platform::Linux.runtime_name(RuntimeKind::FrameGeneration),
            "libnvidia-ngx-dlssg.so.310.9.16"
        );
    }
}
