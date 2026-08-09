/// Generic Windows `ACCESS_MASK` bits that apply to any securable object
/// type (registry keys, services, files, ...), as opposed to bits whose
/// meaning is specific to one object type (e.g. `KEY_QUERY_VALUE`,
/// `SERVICE_START`).
///
/// Plain data: extension crates build their own local bitflag types from
/// these constants rather than this type implementing any Do-runtime trait
/// directly, since this crate has no `dolang-runtime` dependency (it stays
/// portable for use by wire-protocol crates that may run in a lightweight
/// remote agent).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AccessMask(pub u32);

impl AccessMask {
    /// Grants the right to delete the object.
    pub const DELETE: AccessMask = AccessMask(0x0001_0000);
    /// Grants the right to read the object's security descriptor.
    pub const READ_CONTROL: AccessMask = AccessMask(0x0002_0000);
    /// Grants the right to modify the discretionary ACL.
    pub const WRITE_DAC: AccessMask = AccessMask(0x0004_0000);
    /// Grants the right to change the owner or primary group.
    pub const WRITE_OWNER: AccessMask = AccessMask(0x0008_0000);
    /// Grants the synchronization right.
    pub const SYNCHRONIZE: AccessMask = AccessMask(0x0010_0000);
    /// Combines the standard rights required by an object type.
    pub const STANDARD_RIGHTS_REQUIRED: AccessMask = AccessMask(0x000F_0000);
    /// Combines all standard rights.
    pub const STANDARD_RIGHTS_ALL: AccessMask = AccessMask(0x001F_0000);
    /// Requests access to the system ACL; enabling `SeSecurityPrivilege` may be required.
    pub const ACCESS_SYSTEM_SECURITY: AccessMask = AccessMask(0x0100_0000);
    /// Asks the system to grant the maximum permitted access.
    pub const MAXIMUM_ALLOWED: AccessMask = AccessMask(0x0200_0000);
    /// Generic all-access mapping bit.
    pub const GENERIC_ALL: AccessMask = AccessMask(0x1000_0000);
    /// Generic execute-access mapping bit.
    pub const GENERIC_EXECUTE: AccessMask = AccessMask(0x2000_0000);
    /// Generic write-access mapping bit.
    pub const GENERIC_WRITE: AccessMask = AccessMask(0x4000_0000);
    /// Generic read-access mapping bit.
    pub const GENERIC_READ: AccessMask = AccessMask(0x8000_0000);
}
