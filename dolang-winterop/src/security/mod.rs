//! Security identifiers, access masks, and security descriptors.

mod access_mask;
mod sec_desc;
mod sid;
#[cfg(any(windows, docsrs))]
mod win32_security;

pub use access_mask::{AccessMask, TokenGroupAttributes};
pub use sec_desc::{
    Ace, AceBuf, AceBuildError, AceBuildOptions, AceError, AceFlags, AceType, Aces, Acl, AclBuf,
    AclBuildError, AclError, AclKind, AclRevision, ObjectAceFlags, SecDesc, SecDescComponent,
    SecDescControl, SecDescError, SecDescRevision, SecDescUpdate, SecInfo,
};
pub use sid::{Sid, SidError, SidIdentifierAuthority, SidRevision, WellKnownSid};
#[cfg(any(windows, docsrs))]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub use win32_security::with_security_privilege;
