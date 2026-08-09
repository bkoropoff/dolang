#![deny(warnings)]
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
//! use dolang_winterop::security::{AceBuf, AceBuildOptions, AclBuf, Sid};
//!
//! let sid: Sid = "S-1-5-32-544".parse()?;
//! let ace = AceBuf::allow(&sid, 0x0012_0000, AceBuildOptions::new())?;
//! let acl = AclBuf::from_aces([ace], None)?;
//! assert_eq!(acl.ace_count(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#[cfg(windows)]
pub mod apc;
pub mod error;
pub mod guid;
pub mod process;
pub mod security;
