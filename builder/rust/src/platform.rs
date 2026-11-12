//! The host platform's game-folder names and the location of the libraries
//! the build produced. Linux is this engine's native layout; the Windows
//! names and the `bin/` split follow nx2/make_game.py's `BESIDE` list.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
}

impl Platform {
    /// The platform being built for: the host, or Windows when RX_TARGET asks
    /// for a cross build, as nx2/toolchain.py's `windows()` does.
    pub fn current() -> Self {
        if cfg!(windows) || crate::toolchain::windows() {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// The standalone host executable, staged in `bin`.
    pub fn executable(self) -> &'static str {
        match self {
            Self::Linux => "hookshot",
            Self::Windows => "hookshot.exe",
        }
    }

    /// The Vulkan back end, staged in `bin`.
    pub fn vulkan_library(self) -> &'static str {
        match self {
            Self::Linux => "hookshot_vk.so",
            Self::Windows => "hookshot_vk.dll",
        }
    }

    /// The runtime libraries staged in `bin`, in the order nx2/make_game.py
    /// lists them. The primary SDL is first.
    pub fn libraries(self) -> &'static [&'static str] {
        match self {
            Self::Linux => &["libSDL2-2.0.so.0"],
            Self::Windows => &["SDL2.dll", "SDL3.dll", "wgpu_native.dll"],
        }
    }

    /// The SDL library whose presence the package report names: SDL2 on
    /// Windows is sdl2-compat, and loads SDL3.dll from beside itself.
    pub fn primary_sdl_library(self) -> &'static str {
        self.libraries()[0]
    }

    /// Where the builder's own standalone build leaves the Vulkan back end
    /// when the caller did not name one: nx2/build.py's `work/sa/build`.
    pub fn built_vulkan(self, _root: &Path, work: &Path) -> PathBuf {
        work.join("sa/build").join(self.vulkan_library())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_python_builder() {
        assert_eq!(Platform::Linux.executable(), "hookshot");
        assert_eq!(Platform::Linux.vulkan_library(), "hookshot_vk.so");
        assert_eq!(Platform::Linux.libraries(), ["libSDL2-2.0.so.0"]);
        assert_eq!(Platform::Linux.primary_sdl_library(), "libSDL2-2.0.so.0");

        assert_eq!(Platform::Windows.executable(), "hookshot.exe");
        assert_eq!(Platform::Windows.vulkan_library(), "hookshot_vk.dll");
        assert_eq!(
            Platform::Windows.libraries(),
            ["SDL2.dll", "SDL3.dll", "wgpu_native.dll"]
        );
        assert_eq!(Platform::Windows.primary_sdl_library(), "SDL2.dll");
    }

    #[test]
    fn built_vulkan_follows_each_build_layout() {
        let root = Path::new("/repo");
        let work = Path::new("/repo/work");
        assert_eq!(
            Platform::Linux.built_vulkan(root, work),
            work.join("sa/build/hookshot_vk.so")
        );
        assert_eq!(
            Platform::Windows.built_vulkan(root, work),
            work.join("sa/build/hookshot_vk.dll")
        );
    }

    #[test]
    fn the_current_platform_is_one_of_them() {
        assert!(matches!(
            Platform::current(),
            Platform::Linux | Platform::Windows
        ));
    }
}
