use serde::{Deserialize, Serialize};

bitflags::bitflags! {
    /// Permission bits used by NFSv4 ACL entries.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct Nfs4AceMask: u32 {
        /// Read the data of a file, or list the entries of a directory.
        const READ_DATA = 0x00000008;
        /// Write the data of a file, or add a new file to a directory.
        const WRITE_DATA = 0x00000010;
        /// Append data to a file, or add a new subdirectory to a directory.
        const APPEND_DATA = 0x00000020;
        /// Read the named attributes of a file or directory.
        const READ_NAMED_ATTRS = 0x00000040;
        /// Write the named attributes of a file or directory.
        const WRITE_NAMED_ATTRS = 0x00000080;
        /// Execute a file, or search a directory.
        const EXECUTE = 0x00000001;
        /// Delete a file or directory within a directory.
        const DELETE_CHILD = 0x00000100;
        /// Read the (non-ACL) attributes of a file or directory.
        const READ_ATTRIBUTES = 0x00000200;
        /// Write the (non-ACL) attributes of a file or directory.
        const WRITE_ATTRIBUTES = 0x00000400;
        /// Delete the file or directory.
        const DELETE = 0x00000800;
        /// Read the ACL.
        const READ_ACL = 0x00001000;
        /// Write the ACL.
        const WRITE_ACL = 0x00002000;
        /// Change the owner.
        const WRITE_OWNER = 0x00004000;
        /// Synchronize I/O.
        const SYNCHRONIZE = 0x00008000;
    }
}

bitflags::bitflags! {
    /// Inheritance and audit/alarm flags used by NFSv4 ACL entries.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct Nfs4AceFlags: u32 {
        /// Inherited by files created within a directory.
        const FILE_INHERIT = 0x0001;
        /// Inherited by subdirectories created within a directory.
        const DIRECTORY_INHERIT = 0x0002;
        /// Inherited only by direct children, not further descendants.
        const NO_PROPAGATE_INHERIT = 0x0004;
        /// Present only to be inherited; does not apply to the entry's own object.
        const INHERIT_ONLY = 0x0008;
        /// Log successful accesses (audit/alarm entries).
        const SUCCESSFUL_ACCESS = 0x0010;
        /// Log failed accesses (audit/alarm entries).
        const FAILED_ACCESS = 0x0020;
        /// The entry was created by inheritance from a parent directory.
        const INHERITED = 0x0080;
    }
}

/// The kind of an NFSv4 ACL entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Nfs4AceType {
    /// Grants the permissions in the entry's mask.
    Allow,
    /// Denies the permissions in the entry's mask.
    Deny,
    /// Generates an audit log entry when accessed.
    Audit,
    /// Generates an alarm when accessed.
    Alarm,
}

/// The principal selected by an NFSv4 ACL entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Nfs4AceQualifier {
    /// The `OWNER@` special principal: the file's owning user.
    Owner,
    /// The `GROUP@` special principal: the file's owning group.
    OwningGroup,
    /// The `EVERYONE@` special principal.
    Everyone,
    /// A named user.
    User(u32),
    /// A named group.
    Group(u32),
}

/// A portable NFSv4 ACL entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nfs4Ace {
    /// Whether this entry allows, denies, audits, or alarms.
    pub(crate) ace_type: Nfs4AceType,
    /// Principal controlled by this entry.
    pub(crate) qualifier: Nfs4AceQualifier,
    /// Permissions named by this entry.
    pub(crate) mask: Nfs4AceMask,
    /// Inheritance and audit/alarm flags.
    pub(crate) flags: Nfs4AceFlags,
}

impl Nfs4Ace {
    /// Creates an NFSv4 ACL entry.
    pub const fn new(
        ace_type: Nfs4AceType,
        qualifier: Nfs4AceQualifier,
        mask: Nfs4AceMask,
        flags: Nfs4AceFlags,
    ) -> Self {
        Self {
            ace_type,
            qualifier,
            mask,
            flags,
        }
    }
    /// Returns whether this entry allows or denies access.
    pub const fn ace_type(self) -> Nfs4AceType {
        self.ace_type
    }
    /// Returns the principal to which this entry applies.
    pub const fn qualifier(self) -> Nfs4AceQualifier {
        self.qualifier
    }
    /// Returns the access-rights mask.
    pub const fn mask(self) -> Nfs4AceMask {
        self.mask
    }
    /// Returns the inheritance and qualifier flags.
    pub const fn flags(self) -> Nfs4AceFlags {
        self.flags
    }
}

/// A portable NFSv4 access-control list.
///
/// Unlike [`PosixAcl`](crate::security::PosixAcl), NFSv4 ACLs are an ordered,
/// first-match list with no completeness requirement, so there is nothing to
/// validate beyond the shape of the individual entries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nfs4Acl {
    entries: Vec<Nfs4Ace>,
}

impl Nfs4Acl {
    /// Constructs an access-control list from `entries`, in evaluation order.
    pub fn new(entries: Vec<Nfs4Ace>) -> Self {
        Self { entries }
    }

    /// Returns the ACL entries in their stored (evaluation) order.
    pub fn entries(&self) -> &[Nfs4Ace] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip_preserves_entries() {
        let acl = Nfs4Acl::new(vec![
            Nfs4Ace {
                ace_type: Nfs4AceType::Allow,
                qualifier: Nfs4AceQualifier::Owner,
                mask: Nfs4AceMask::READ_DATA | Nfs4AceMask::WRITE_DATA,
                flags: Nfs4AceFlags::empty(),
            },
            Nfs4Ace {
                ace_type: Nfs4AceType::Deny,
                qualifier: Nfs4AceQualifier::User(1000),
                mask: Nfs4AceMask::WRITE_DATA,
                flags: Nfs4AceFlags::FILE_INHERIT | Nfs4AceFlags::DIRECTORY_INHERIT,
            },
        ]);
        let bytes = postcard::to_stdvec(&acl).unwrap();
        assert_eq!(postcard::from_bytes::<Nfs4Acl>(&bytes).unwrap(), acl);
    }
}
