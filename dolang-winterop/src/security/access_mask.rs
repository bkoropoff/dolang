bitflags::bitflags! {
/// Generic `ACCESS_MASK` flags.
///
/// These apply to any securable object type (registry keys, services, files, ...), as opposed to
/// bits whose meaning is specific to one object type (e.g. `KEY_QUERY_VALUE`, `SERVICE_START`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AccessMask: u32 {
    /// All object-specific rights in the low 16 bits.
    const SPECIFIC_RIGHTS_ALL = 0x0000_FFFF;
    /// Grants the right to delete the object.
    const DELETE = 0x0001_0000;
    /// Grants the right to read the object's security descriptor.
    const READ_CONTROL = 0x0002_0000;
    /// Grants the right to modify the discretionary ACL.
    const WRITE_DAC = 0x0004_0000;
    /// Grants the right to change the owner or primary group.
    const WRITE_OWNER = 0x0008_0000;
    /// Grants the synchronization right.
    const SYNCHRONIZE = 0x0010_0000;
    /// Combines the standard rights required by an object type.
    const STANDARD_RIGHTS_REQUIRED = 0x000F_0000;
    /// Combines all standard rights.
    const STANDARD_RIGHTS_ALL = 0x001F_0000;
    /// Requests access to the system ACL; enabling `SeSecurityPrivilege` may be required.
    const ACCESS_SYSTEM_SECURITY = 0x0100_0000;
    /// Asks the system to grant the maximum permitted access.
    const MAXIMUM_ALLOWED = 0x0200_0000;
    /// Generic all-access mapping bit.
    const GENERIC_ALL = 0x1000_0000;
    /// Generic execute-access mapping bit.
    const GENERIC_EXECUTE = 0x2000_0000;
    /// Generic write-access mapping bit.
    const GENERIC_WRITE = 0x4000_0000;
    /// Generic read-access mapping bit.
    const GENERIC_READ = 0x8000_0000;
}
}

impl AccessMask {
    /// Returns the object-specific low 16 bits.
    pub const fn specific_rights(self) -> u16 {
        self.bits() as u16
    }

    /// Creates a mask from object-specific low 16 bits.
    pub const fn from_specific_rights(rights: u16) -> Self {
        Self::from_bits_retain(rights as u32)
    }

    /// Returns the standard and system-access rights.
    pub const fn standard_rights(self) -> Self {
        self.intersection(Self::from_bits_retain(0x0FFF_0000))
    }

    /// Returns the generic mapping rights.
    pub const fn generic_rights(self) -> Self {
        self.intersection(Self::from_bits_retain(0xF000_0000))
    }
}

bitflags::bitflags! {
    /// Native `SE_GROUP_*` attributes attached to token groups.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub struct TokenGroupAttributes: u32 {
        const MANDATORY = 0x0000_0001;
        const ENABLED_BY_DEFAULT = 0x0000_0002;
        const ENABLED = 0x0000_0004;
        const OWNER = 0x0000_0008;
        const USE_FOR_DENY_ONLY = 0x0000_0010;
        const INTEGRITY = 0x0000_0020;
        const INTEGRITY_ENABLED = 0x0000_0040;
        const RESOURCE = 0x2000_0000;
        const LOGON_ID = 0xC000_0000;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_preserves_bits() {
        let mask = AccessMask::from_specific_rights(0x8123)
            | AccessMask::READ_CONTROL
            | AccessMask::GENERIC_READ
            | AccessMask::from_bits_retain(0x0080_0000);
        let encoded = postcard::to_stdvec(&mask).unwrap();
        assert_eq!(encoded, postcard::to_stdvec(&mask.bits()).unwrap());
        assert_eq!(postcard::from_bytes::<AccessMask>(&encoded).unwrap(), mask);
        assert_eq!(mask.specific_rights(), 0x8123);
        assert_eq!(
            mask.standard_rights(),
            AccessMask::READ_CONTROL | AccessMask::from_bits_retain(0x0080_0000)
        );
        assert_eq!(mask.generic_rights(), AccessMask::GENERIC_READ);
    }

    #[test]
    fn token_group_attributes_preserve_unknown_bits() {
        let attributes = TokenGroupAttributes::LOGON_ID
            | TokenGroupAttributes::ENABLED
            | TokenGroupAttributes::from_bits_retain(0x1000_0000);
        let encoded = postcard::to_stdvec(&attributes).unwrap();
        assert_eq!(encoded, postcard::to_stdvec(&attributes.bits()).unwrap());
        assert_eq!(
            postcard::from_bytes::<TokenGroupAttributes>(&encoded).unwrap(),
            attributes
        );
    }
}
