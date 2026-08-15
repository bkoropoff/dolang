use serde::{Deserialize, Serialize};
use uuid::Uuid;

bitflags::bitflags! {
    /// Permission bits used by macOS extended ACL entries.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct MacosAceMask: u32 {
        /// Read the data of a file, or list the entries of a directory.
        const READ_DATA = 0x00000002;
        /// Write the data of a file, or add a new file to a directory.
        const WRITE_DATA = 0x00000004;
        /// Execute a file, or search a directory.
        const EXECUTE = 0x00000008;
        /// Delete the file or directory.
        const DELETE = 0x00000010;
        /// Append data to a file, or add a new subdirectory to a directory.
        const APPEND_DATA = 0x00000020;
        /// Delete a file or directory within a directory.
        const DELETE_CHILD = 0x00000040;
        /// Read the (non-ACL) attributes of a file or directory.
        const READ_ATTRIBUTES = 0x00000080;
        /// Write the (non-ACL) attributes of a file or directory.
        const WRITE_ATTRIBUTES = 0x00000100;
        /// Read the extended attributes of a file or directory.
        const READ_EXTATTRIBUTES = 0x00000200;
        /// Write the extended attributes of a file or directory.
        const WRITE_EXTATTRIBUTES = 0x00000400;
        /// Read the ACL/security information.
        const READ_SECURITY = 0x00000800;
        /// Write the ACL/security information.
        const WRITE_SECURITY = 0x00001000;
        /// Change the owner.
        const CHANGE_OWNER = 0x00002000;
        /// Synchronize I/O.
        const SYNCHRONIZE = 0x00100000;
    }
}

bitflags::bitflags! {
    /// Inheritance flags used by macOS extended ACL entries.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct MacosAceFlags: u32 {
        /// The entry was created by inheritance from a parent directory.
        const INHERITED = 0x0010;
        /// Inherited by files created within a directory.
        const FILE_INHERIT = 0x0020;
        /// Inherited by subdirectories created within a directory.
        const DIRECTORY_INHERIT = 0x0040;
        /// Inherited only by direct children, not further descendants.
        const LIMIT_INHERIT = 0x0080;
        /// Present only to be inherited; does not apply to the entry's own object.
        const ONLY_INHERIT = 0x0100;
    }
}

/// The kind of a macOS extended ACL entry.
///
/// Unlike NFSv4 ACLs, macOS extended ACLs have no audit/alarm entry kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MacosAceType {
    /// Grants the permissions in the entry's mask.
    Allow,
    /// Denies the permissions in the entry's mask.
    Deny,
}

/// A portable macOS extended ACL entry.
///
/// macOS resolves every principal (owning user, owning group, well-known
/// accounts, or an arbitrary user/group) to a `guid_t` before it reaches the
/// kernel ACL, so unlike [`Nfs4Ace`](crate::security::Nfs4Ace)'s qualifier,
/// there is no separate enum here: the qualifier is simply the UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MacosAce {
    /// Whether this entry allows or denies.
    pub ace_type: MacosAceType,
    /// Principal controlled by this entry, as a macOS `guid_t`.
    pub qualifier: Uuid,
    /// Permissions named by this entry.
    pub mask: MacosAceMask,
    /// Inheritance flags.
    pub flags: MacosAceFlags,
}

/// A portable macOS extended access-control list.
///
/// Like [`Nfs4Acl`](crate::security::Nfs4Acl), this is an ordered,
/// first-match list with no completeness requirement, so there is nothing to
/// validate beyond the shape of the individual entries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacosAcl {
    entries: Vec<MacosAce>,
}

impl MacosAcl {
    /// Constructs an access-control list from `entries`, in evaluation order.
    pub fn new(entries: Vec<MacosAce>) -> Self {
        Self { entries }
    }

    /// Returns the ACL entries in their stored (evaluation) order.
    pub fn entries(&self) -> &[MacosAce] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip_preserves_entries() {
        let acl = MacosAcl::new(vec![
            MacosAce {
                ace_type: MacosAceType::Allow,
                qualifier: Uuid::nil(),
                mask: MacosAceMask::READ_DATA | MacosAceMask::WRITE_DATA,
                flags: MacosAceFlags::empty(),
            },
            MacosAce {
                ace_type: MacosAceType::Deny,
                qualifier: Uuid::max(),
                mask: MacosAceMask::WRITE_DATA,
                flags: MacosAceFlags::FILE_INHERIT | MacosAceFlags::DIRECTORY_INHERIT,
            },
        ]);
        let bytes = postcard::to_stdvec(&acl).unwrap();
        assert_eq!(postcard::from_bytes::<MacosAcl>(&bytes).unwrap(), acl);
    }
}
