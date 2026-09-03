//! Target-platform descriptions and capabilities.

use serde::{Deserialize, Serialize};

/// Operating system that produced a target description or native error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OperatingSystem {
    /// FreeBSD.
    FreeBsd,
    /// Linux.
    Linux,
    /// macOS.
    Macos,
    /// Windows.
    Windows,
}

impl OperatingSystem {
    /// Returns the operating system of the current host.
    pub fn current() -> Self {
        #[cfg(target_os = "linux")]
        return Self::Linux;
        #[cfg(target_os = "macos")]
        return Self::Macos;
        #[cfg(target_os = "freebsd")]
        return Self::FreeBsd;
        #[cfg(windows)]
        return Self::Windows;
        #[cfg(not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "freebsd",
            windows
        )))]
        compile_error!("unsupported target operating system");
    }

    /// Returns the path syntax associated with this operating system.
    pub const fn path_kind(&self) -> crate::path::Kind {
        match self {
            Self::Linux | Self::Macos | Self::FreeBsd => crate::path::Kind::Unix,
            Self::Windows => crate::path::Kind::Windows,
        }
    }
}

/// CPU architecture reported by a VFS target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Architecture {
    /// 64-bit x86.
    X86_64,
    /// 64-bit ARM.
    Aarch64,
}

/// Broad operating-system family used for platform-specific behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingSystemFamily {
    /// Unix-like operating systems.
    Unix,
    /// Windows operating systems.
    Windows,
}

/// Target operating-system and processor information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetInfo {
    /// Target operating system.
    pub(crate) os: OperatingSystem,
    /// Target CPU architecture.
    pub(crate) arch: Architecture,
    /// Number of logical CPUs available to the target.
    pub(crate) logical_cpus: u32,
    /// Whether the target is Windows running under Wine, when applicable.
    pub(crate) is_wine: Option<bool>,
}

impl Architecture {
    /// Returns the architecture of the current host.
    pub fn current() -> Self {
        #[cfg(target_arch = "x86_64")]
        return Self::X86_64;
        #[cfg(target_arch = "aarch64")]
        return Self::Aarch64;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        compile_error!("unsupported target architecture");
    }
}

impl OperatingSystem {
    /// Returns the broad family containing this operating system.
    pub fn family(&self) -> OperatingSystemFamily {
        match self {
            Self::FreeBsd | Self::Linux | Self::Macos => OperatingSystemFamily::Unix,
            Self::Windows => OperatingSystemFamily::Windows,
        }
    }
}

impl TargetInfo {
    /// Returns the target operating system.
    pub const fn os(&self) -> OperatingSystem {
        self.os
    }
    /// Returns the target CPU architecture.
    pub const fn arch(&self) -> Architecture {
        self.arch
    }
    /// Returns the number of logical CPUs available to the target.
    pub const fn logical_cpus(&self) -> u32 {
        self.logical_cpus
    }
    /// Returns whether Windows is running under Wine, when applicable.
    pub const fn is_wine(&self) -> Option<bool> {
        self.is_wine
    }
    /// Returns a description of the current host.
    pub fn current() -> Self {
        Self {
            os: OperatingSystem::current(),
            arch: Architecture::current(),
            logical_cpus: std::thread::available_parallelism()
                .map_or(1, |count| u32::try_from(count.get()).unwrap_or(u32::MAX)),
            is_wine: current_wine_status(),
        }
    }
}

#[cfg(windows)]
fn current_wine_status() -> Option<bool> {
    Some(dolang_winterop::is_wine())
}

#[cfg(not(windows))]
fn current_wine_status() -> Option<bool> {
    None
}
