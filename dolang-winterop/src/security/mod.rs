//! Windows security identifiers, access masks, and security descriptors.

mod access_mask;
mod sec_desc;
mod sid;
#[cfg(windows)]
mod win32_security;

pub use access_mask::AccessMask;
pub use sec_desc::{
    ALL_SECURITY_INFORMATION, Ace, AceBuf, AceBuildError, AceBuildOptions, AceError, AceType, Aces,
    Acl, AclBuf, AclBuildError, AclError, AclKind, DACL_SECURITY_INFORMATION,
    GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, SACL_SECURITY_INFORMATION, SecDesc,
    SecDescComponent, SecDescError, SecDescUpdate,
};
pub use sid::{Sid, SidError};
#[cfg(windows)]
pub use win32_security::with_security_privilege;
