#![deny(warnings)]
#![cfg_attr(docsrs, feature(doc_cfg))]
//! Portable representations and small utilities for interoperating with
//! Windows APIs and wire formats.
//!
//! The [`security`] and [`guid`] modules validate their native binary encodings
//! and remain available
//! on every target. This makes them suitable for RPC payloads and tools that
//! inspect a Windows target from another operating system. Windows-only
//! facilities, such as the APC reactor, are conditionally exported.
//!
//! ```
//! use dolang_winterop::security::{AccessMask, AceBuf, AceBuildOptions, AclBuf, Sid};
//!
//! let sid: Sid = "S-1-5-32-544".parse()?;
//! let ace = AceBuf::allow(
//!     &sid,
//!     AccessMask::from_bits_retain(0x0012_0000),
//!     AceBuildOptions::new(),
//! )?;
//! let acl = AclBuf::from_aces([ace], None)?;
//! assert_eq!(acl.ace_count(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#[cfg(any(windows, docsrs))]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub mod apc;
pub mod error;
pub mod guid;
pub mod process;
pub mod security;

/// Returns whether the current process is running under Wine.
#[cfg(windows)]
#[cfg_attr(docsrs, doc(auto_cfg = false))]
pub fn is_wine() -> bool {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::core::w;

    const WINE_GET_VERSION: &[u8] = b"wine_get_version\0";

    let ntdll = unsafe { GetModuleHandleW(w!("ntdll.dll")) };
    !ntdll.is_null() && unsafe { GetProcAddress(ntdll, WINE_GET_VERSION.as_ptr()) }.is_some()
}

/// Returns whether the current process is running under Wine.
#[cfg(not(windows))]
#[cfg_attr(docsrs, doc(auto_cfg = false))]
pub const fn is_wine() -> bool {
    false
}
