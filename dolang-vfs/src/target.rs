//! Target-platform descriptions and capabilities.

use serde::{Deserialize, Serialize};

/// Operating system that produced a target description or native error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingSystem {
    FreeBsd,
    Linux,
    Macos,
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
    pub const fn path_type(&self) -> typed_path::PathType {
        match self {
            Self::Linux | Self::Macos | Self::FreeBsd => typed_path::PathType::Unix,
            Self::Windows => typed_path::PathType::Windows,
        }
    }
}

/// CPU architecture reported by a VFS target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    X86_64,
    Aarch64,
}

/// Broad operating-system family used for platform-specific behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingSystemFamily {
    Unix,
    Windows,
}

/// Target operating-system and processor information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetInfo {
    pub operating_system: OperatingSystem,
    pub architecture: Architecture,
    pub logical_cpu_count: u32,
    pub is_wine: Option<bool>,
}

impl Architecture {
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
    pub fn family(&self) -> OperatingSystemFamily {
        match self {
            Self::FreeBsd | Self::Linux | Self::Macos => OperatingSystemFamily::Unix,
            Self::Windows => OperatingSystemFamily::Windows,
        }
    }
}
impl TargetInfo {
    pub fn current() -> Self {
        Self {
            operating_system: OperatingSystem::current(),
            architecture: Architecture::current(),
            logical_cpu_count: std::thread::available_parallelism()
                .map_or(1, |count| u32::try_from(count.get()).unwrap_or(u32::MAX)),
            is_wine: current_wine_status(),
        }
    }
}

#[cfg(windows)]
fn current_wine_status() -> Option<bool> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::core::w;

    const WINE_GET_VERSION: &[u8] = b"wine_get_version\0";

    let ntdll = unsafe { GetModuleHandleW(w!("ntdll.dll")) };
    Some(!ntdll.is_null() && unsafe { GetProcAddress(ntdll, WINE_GET_VERSION.as_ptr()) }.is_some())
}

#[cfg(not(windows))]
fn current_wine_status() -> Option<bool> {
    None
}
