#![deny(warnings)]
//! Portable representations and small utilities for interoperating with
//! Windows APIs and wire formats.
//!
//! The security types ([`Sid`], [`Guid`], [`AceBuf`], [`AclBuf`], and
//! [`SecDesc`]) validate their native binary encodings and remain available
//! on every target. This makes them suitable for RPC payloads and tools that
//! inspect a Windows target from another operating system. Windows-only
//! facilities, such as the APC reactor, are conditionally exported.
//!
//! ```
//! use dolang_winterop::{AceBuf, AceBuildOptions, AclBuf, Sid};
//!
//! let sid: Sid = "S-1-5-32-544".parse()?;
//! let ace = AceBuf::allow(&sid, 0x0012_0000, AceBuildOptions::default())?;
//! let acl = AclBuf::from_aces([ace], None)?;
//! assert_eq!(acl.ace_count(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod access_mask;
#[cfg(windows)]
mod apc;
mod guid;
pub mod process;
mod sec_desc;
mod sid;
#[cfg(windows)]
mod win32_security;
mod win_error;

pub use access_mask::AccessMask;
#[cfg(windows)]
pub use apc::{ApcCancelled, ApcContext, ApcTask, Closed, Reactor, ReactorControl, TaskCancelled};
pub use guid::{Guid, GuidError};
pub use sec_desc::{
    ALL_SECURITY_INFORMATION, Ace, AceBuf, AceBuildError, AceBuildOptions, AceError, AceType, Aces,
    Acl, AclBuf, AclBuildError, AclError, DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, SACL_SECURITY_INFORMATION, SecDesc, SecDescError, SecDescUpdate,
};
pub use sid::{Sid, SidError};
pub use win_error::{win_error_code, win_error_name};
#[cfg(windows)]
pub use win32_security::with_security_privilege;
