//! Ownership identities and POSIX access-control lists.

use dolang_winterop::security::{Sid, TokenGroupAttributes};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::io;

use crate::error::Result;
pub use crate::{
    macos_acl::{MacosAce, MacosAceFlags, MacosAceMask, MacosAceType, MacosAcl},
    nfs4_acl::{Nfs4Ace, Nfs4AceFlags, Nfs4AceMask, Nfs4AceQualifier, Nfs4AceType, Nfs4Acl},
    posix_acl::{PosixAce, PosixAcl, PosixAclError, PosixAclQualifier},
};

bitflags::bitflags! {
    /// Unix read/write/execute permission bits, as granted by one class of a
    /// [`crate::metadata::Mode`] or one entry of a POSIX ACL.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct Permission: u8 {
        /// Grants read access.
        const READ = 0o4;
        /// Grants write access.
        const WRITE = 0o2;
        /// Grants execute or directory-search access.
        const EXECUTE = 0o1;
    }
}

/// Selects which kind of access-control list a `get_acl`-style operation
/// should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AclKind {
    /// POSIX.1e ACL.
    Posix,
    /// NFSv4 ACL.
    Nfs4,
    /// macOS extended ACL.
    Macos,
}

/// A portable access-control list of any supported kind.
///
/// Returned by `get_acl`-style operations and accepted by `set_acl`-style
/// ones, which infer the kind from the variant rather than taking a separate
/// selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Acl {
    /// POSIX.1e ACL.
    Posix(PosixAcl),
    /// NFSv4 ACL.
    Nfs4(Nfs4Acl),
    /// macOS extended ACL.
    Macos(MacosAcl),
}

impl Acl {
    /// Returns the [`AclKind`] of this ACL.
    pub fn kind(&self) -> AclKind {
        match self {
            Self::Posix(_) => AclKind::Posix,
            Self::Nfs4(_) => AclKind::Nfs4,
            Self::Macos(_) => AclKind::Macos,
        }
    }
}

/// A principal identifier of a specific kind, used with
/// [`Vfs::resolve_principal_id`](crate::Vfs::resolve_principal_id) to
/// convert between them (e.g. a Unix uid/gid and a macOS `guid_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PrincipalId {
    /// A Unix user ID.
    Uid(u32),
    /// A Unix group ID.
    Gid(u32),
    /// A macOS principal UUID (`guid_t`).
    Uuid(uuid::Uuid),
}

/// Selects which [`PrincipalId`] variant a `resolve_principal_id`-style
/// operation should produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PrincipalIdKind {
    /// Resolve to a Unix user ID.
    Uid,
    /// Resolve to a Unix group ID.
    Gid,
    /// Resolve to a macOS principal UUID (`guid_t`).
    Uuid,
}

/// An owner or group selected by numeric ID, account name, or Windows SID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OwnershipIdentity {
    /// A platform numeric user or group ID.
    Id(u32),
    /// An account name resolved by the target.
    Name(String),
    /// A Windows security identifier.
    Sid(Sid),
}

/// Snapshot of a VFS target's process security context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecurityInfo(SecurityInfoInner);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum SecurityInfoInner {
    /// Unix identity information.
    Unix(UnixSecurityInfo),
    /// Windows token information.
    Windows(WindowsTokenInfo),
}

/// Unix identity information for a VFS target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnixSecurityInfo {
    /// Real user ID.
    pub(crate) uid: u32,
    /// Real group ID.
    pub(crate) gid: u32,
    /// Effective user ID.
    pub(crate) euid: u32,
    /// Effective group ID.
    pub(crate) egid: u32,
    /// Supplementary group IDs.
    pub(crate) groups: Vec<u32>,
}

/// Windows token information for a VFS target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsTokenInfo {
    /// Whether the token has elevated administrator privileges.
    pub(crate) is_elevated: bool,
    /// Token user SID.
    pub(crate) user_sid: Sid,
    /// Token owner SID.
    pub(crate) owner_sid: Sid,
    /// Token primary group SID.
    pub(crate) primary_group_sid: Sid,
    /// Token group memberships.
    pub(crate) groups: Vec<TokenGroup>,
}

/// A Windows token group SID and its attribute mask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenGroup {
    /// Group security identifier.
    pub(crate) sid: Sid,
    /// Native group attributes bitmask.
    pub(crate) attributes: TokenGroupAttributes,
}

/// Classification returned by Windows account-name lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SidNameUse {
    /// User account.
    User,
    /// Group account.
    Group,
    /// Domain account.
    Domain,
    /// Local alias.
    Alias,
    /// Well-known group.
    WellKnownGroup,
    /// Deleted account.
    DeletedAccount,
    /// Invalid SID type.
    Invalid,
    /// Unrecognized SID type.
    Unknown,
    /// Computer account.
    Computer,
    /// Integrity label.
    Label,
    /// Logon session.
    LogonSession,
}

/// A Windows SID together with its resolved account name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidName {
    /// Resolved security identifier.
    pub(crate) sid: Sid,
    /// Account name.
    pub(crate) name: String,
    /// Account domain.
    pub(crate) domain: String,
    /// Account classification.
    pub(crate) kind: SidNameUse,
}

impl SecurityInfo {
    /// Returns the Unix security information, if this is a Unix target.
    pub fn unix(&self) -> Option<&UnixSecurityInfo> {
        match &self.0 {
            SecurityInfoInner::Unix(info) => Some(info),
            _ => None,
        }
    }
    /// Returns the Windows token information, if this is a Windows target.
    pub fn windows(&self) -> Option<&WindowsTokenInfo> {
        match &self.0 {
            SecurityInfoInner::Windows(info) => Some(info),
            _ => None,
        }
    }
    /// Captures the security context of the current process.
    pub fn current() -> Result<Self> {
        #[cfg(unix)]
        return Ok(Self(SecurityInfoInner::Unix(UnixSecurityInfo::current()?)));
        #[cfg(windows)]
        return Ok(Self(SecurityInfoInner::Windows(
            WindowsTokenInfo::current()?
        )));
    }
}

impl UnixSecurityInfo {
    /// Returns the real user ID.
    pub const fn uid(&self) -> u32 {
        self.uid
    }
    /// Returns the real group ID.
    pub const fn gid(&self) -> u32 {
        self.gid
    }
    /// Returns the effective user ID.
    pub const fn effective_uid(&self) -> u32 {
        self.euid
    }
    /// Returns the effective group ID.
    pub const fn effective_gid(&self) -> u32 {
        self.egid
    }
    /// Returns the supplementary group IDs.
    pub fn groups(&self) -> &[u32] {
        &self.groups
    }
}

impl TokenGroup {
    /// Returns the group security identifier.
    pub fn sid(&self) -> &Sid {
        &self.sid
    }
    /// Returns the native token-group attributes.
    pub const fn attributes(&self) -> TokenGroupAttributes {
        self.attributes
    }
}

impl SidName {
    /// Returns the resolved security identifier.
    pub fn sid(&self) -> &Sid {
        &self.sid
    }
    /// Returns the account name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the account domain.
    pub fn domain(&self) -> &str {
        &self.domain
    }
    /// Returns the account classification.
    pub const fn kind(&self) -> SidNameUse {
        self.kind
    }
}

#[cfg(unix)]
impl UnixSecurityInfo {
    fn current() -> Result<Self> {
        use nix::unistd::{getegid, geteuid, getgid, getuid};

        let euid = geteuid();
        let egid = getegid();

        Ok(Self {
            uid: getuid().as_raw(),
            gid: getgid().as_raw(),
            euid: euid.as_raw(),
            egid: egid.as_raw(),
            groups: current_groups(euid, egid)?,
        })
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn current_groups(_euid: nix::unistd::Uid, _egid: nix::unistd::Gid) -> Result<Vec<u32>> {
    Ok(nix::unistd::getgroups()
        .map_err(io::Error::from)?
        .into_iter()
        .map(|gid| gid.as_raw())
        .collect())
}

#[cfg(target_os = "macos")]
fn current_groups(euid: nix::unistd::Uid, egid: nix::unistd::Gid) -> Result<Vec<u32>> {
    use std::{ffi::CString, ptr, slice};

    // macOS limits the public getgroups/getgrouplist interfaces and resolves
    // extended memberships through opendirectoryd. This SPI returns the full
    // list in a libc-allocated buffer owned by the caller.
    unsafe extern "C" {
        fn getgrouplist_2(
            name: *const libc::c_char,
            base_gid: libc::gid_t,
            groups: *mut *mut libc::gid_t,
        ) -> i32;
    }

    let user = nix::unistd::User::from_uid(euid)
        .map_err(io::Error::from)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "effective user not found"))?;
    let name = CString::new(user.name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "user name contains NUL"))?;
    let mut groups = ptr::null_mut();
    let count = unsafe { getgrouplist_2(name.as_ptr(), egid.as_raw(), &mut groups) };
    if count < 0 {
        if !groups.is_null() {
            unsafe { libc::free(groups.cast()) };
        }
        return Err(io::Error::other("getgrouplist_2 failed").into());
    }
    if count == 0 {
        if !groups.is_null() {
            unsafe { libc::free(groups.cast()) };
        }
        return Ok(Vec::new());
    }
    if count > 0 && groups.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "getgrouplist_2 returned a null group list",
        )
        .into());
    }
    let result = unsafe { slice::from_raw_parts(groups, count as usize) }.to_vec();
    unsafe { libc::free(groups.cast()) };
    Ok(result)
}

impl WindowsTokenInfo {
    /// Returns whether the token has elevated administrator privileges.
    pub const fn is_elevated(&self) -> bool {
        self.is_elevated
    }
    /// Returns the token user SID.
    pub fn user_sid(&self) -> &Sid {
        &self.user_sid
    }
    /// Returns the token owner SID.
    pub fn owner_sid(&self) -> &Sid {
        &self.owner_sid
    }
    /// Returns the token primary group SID.
    pub fn primary_group_sid(&self) -> &Sid {
        &self.primary_group_sid
    }
    /// Returns the token group memberships.
    pub fn groups(&self) -> &[TokenGroup] {
        &self.groups
    }
    /// Returns the logon SID identified by the token group attributes.
    pub fn logon_sid(&self) -> Option<&Sid> {
        self.groups
            .iter()
            .find(|group| group.attributes.contains(TokenGroupAttributes::LOGON_ID))
            .map(|group| &group.sid)
    }
}

#[cfg(windows)]
impl WindowsTokenInfo {
    fn current() -> Result<Self> {
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        // SAFETY: the pseudo-handle from `GetCurrentProcess` is valid for the
        // life of the process and needs no closing.
        unsafe { Self::from_process_handle(GetCurrentProcess()) }
    }

    /// Reads the token of the process `handle` refers to.
    ///
    /// # Safety
    ///
    /// `handle` must be a live process handle carrying at least
    /// `PROCESS_QUERY_INFORMATION` (or `PROCESS_QUERY_LIMITED_INFORMATION`).
    pub(crate) unsafe fn from_process_handle(
        handle: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<Self> {
        use std::{
            io, mem,
            os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
            ptr, slice,
        };
        use windows_sys::Win32::{
            Foundation::HANDLE,
            Security::{
                GetLengthSid, GetTokenInformation, IsValidSid, PSID, TOKEN_ELEVATION, TOKEN_GROUPS,
                TOKEN_INFORMATION_CLASS, TOKEN_OWNER, TOKEN_PRIMARY_GROUP, TOKEN_QUERY, TOKEN_USER,
                TokenElevation, TokenGroups, TokenOwner, TokenPrimaryGroup, TokenUser,
            },
            System::Threading::OpenProcessToken,
        };

        fn query(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
            let mut required = 0;
            unsafe {
                GetTokenInformation(token, class, ptr::null_mut(), 0, &mut required);
            }
            if required == 0 {
                return Err(io::Error::last_os_error());
            }
            let word_size = mem::size_of::<usize>();
            let mut buffer = vec![0usize; (required as usize).div_ceil(word_size)];
            if unsafe {
                GetTokenInformation(
                    token,
                    class,
                    buffer.as_mut_ptr().cast(),
                    required,
                    &mut required,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(buffer)
        }

        unsafe fn copy_sid(sid: PSID) -> io::Result<Sid> {
            if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid token SID",
                ));
            }
            let length = unsafe { GetLengthSid(sid) } as usize;
            let bytes = unsafe { slice::from_raw_parts(sid.cast::<u8>(), length) };
            Sid::from_bytes(bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        }

        unsafe fn view<T>(buffer: &[usize]) -> &T {
            unsafe { &*buffer.as_ptr().cast::<T>() }
        }

        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(handle, TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let token = token.as_raw_handle();

        let elevation = query(token, TokenElevation)?;
        let user = query(token, TokenUser)?;
        let owner = query(token, TokenOwner)?;
        let primary_group = query(token, TokenPrimaryGroup)?;
        let groups = query(token, TokenGroups)?;

        let elevation = unsafe { view::<TOKEN_ELEVATION>(&elevation) };
        let user = unsafe { copy_sid(view::<TOKEN_USER>(&user).User.Sid) }?;
        let owner = unsafe { copy_sid(view::<TOKEN_OWNER>(&owner).Owner) }?;
        let primary_group =
            unsafe { copy_sid(view::<TOKEN_PRIMARY_GROUP>(&primary_group).PrimaryGroup) }?;
        let groups_info = unsafe { view::<TOKEN_GROUPS>(&groups) };
        let native_groups = unsafe {
            slice::from_raw_parts(
                groups_info.Groups.as_ptr(),
                usize::try_from(groups_info.GroupCount).unwrap(),
            )
        };
        let groups = native_groups
            .iter()
            .map(|group| {
                Ok(TokenGroup {
                    sid: unsafe { copy_sid(group.Sid) }?,
                    attributes: TokenGroupAttributes::from_bits_retain(group.Attributes),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;

        Ok(Self {
            is_elevated: elevation.TokenIsElevated != 0,
            user_sid: user,
            owner_sid: owner,
            primary_group_sid: primary_group,
            groups,
        })
    }
}
