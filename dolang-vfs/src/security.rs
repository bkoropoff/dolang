//! Ownership identities and POSIX access-control lists.

use dolang_winterop::security::{Sid, TokenGroupAttributes};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::io;

pub use crate::metadata::{Mode, Permission};
pub use crate::nfs4_acl::{
    Nfs4Ace, Nfs4AceFlags, Nfs4AceMask, Nfs4AceQualifier, Nfs4AceType, Nfs4Acl,
};
pub use crate::posix_acl::{PosixAce, PosixAcl, PosixAclError, PosixAclQualifier};

/// Selects which kind of access-control list a `get_acl`-style operation
/// should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AclKind {
    /// POSIX.1e ACL.
    Posix,
    /// NFSv4 ACL.
    Nfs4,
}

/// A portable access-control list of any supported kind.
///
/// Returned by `get_acl`-style operations and accepted by `set_acl`-style
/// ones, which infer the kind from the variant rather than taking a separate
/// selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Acl {
    /// POSIX.1e ACL.
    Posix(PosixAcl),
    /// NFSv4 ACL.
    Nfs4(Nfs4Acl),
}

impl Acl {
    /// Returns the [`AclKind`] of this ACL.
    pub fn kind(&self) -> AclKind {
        match self {
            Self::Posix(_) => AclKind::Posix,
            Self::Nfs4(_) => AclKind::Nfs4,
        }
    }
}

/// An owner or group selected by numeric ID, account name, or Windows SID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub enum SecurityInfo {
    /// Unix identity information.
    Unix(UnixSecurityInfo),
    /// Windows token information.
    Windows(WindowsTokenInfo),
}

/// Unix identity information for a VFS target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnixSecurityInfo {
    /// Real user ID.
    pub uid: u32,
    /// Real group ID.
    pub gid: u32,
    /// Effective user ID.
    pub euid: u32,
    /// Effective group ID.
    pub egid: u32,
    /// Supplementary group IDs.
    pub group_ids: Vec<u32>,
}

/// Windows token information for a VFS target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsTokenInfo {
    /// Whether the token has elevated administrator privileges.
    pub is_elevated: bool,
    /// Token user SID.
    pub user_sid: Sid,
    /// Token owner SID.
    pub owner_sid: Sid,
    /// Token primary group SID.
    pub primary_group_sid: Sid,
    /// Token group memberships.
    pub groups: Vec<TokenGroup>,
}

/// A Windows token group SID and its attribute mask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenGroup {
    /// Group security identifier.
    pub sid: Sid,
    /// Native group attributes bitmask.
    pub attributes: TokenGroupAttributes,
}

/// Classification returned by Windows account-name lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub sid: Sid,
    /// Account name.
    pub name: String,
    /// Account domain.
    pub domain: String,
    /// Account classification.
    pub kind: SidNameUse,
}

impl SecurityInfo {
    /// Captures the security context of the current process.
    pub fn current() -> crate::Result<Self> {
        #[cfg(unix)]
        return Ok(Self::Unix(UnixSecurityInfo::current()?));
        #[cfg(windows)]
        return Ok(Self::Windows(WindowsTokenInfo::current()?));
    }
}

#[cfg(unix)]
impl UnixSecurityInfo {
    fn current() -> crate::Result<Self> {
        use nix::unistd::{getegid, geteuid, getgid, getuid};

        let euid = geteuid();
        let egid = getegid();

        Ok(Self {
            uid: getuid().as_raw(),
            gid: getgid().as_raw(),
            euid: euid.as_raw(),
            egid: egid.as_raw(),
            group_ids: current_group_ids(euid, egid)?,
        })
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn current_group_ids(_euid: nix::unistd::Uid, _egid: nix::unistd::Gid) -> crate::Result<Vec<u32>> {
    Ok(nix::unistd::getgroups()
        .map_err(io::Error::from)?
        .into_iter()
        .map(|gid| gid.as_raw())
        .collect())
}

#[cfg(target_os = "macos")]
fn current_group_ids(euid: nix::unistd::Uid, egid: nix::unistd::Gid) -> crate::Result<Vec<u32>> {
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
    fn current() -> crate::Result<Self> {
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
            System::Threading::{GetCurrentProcess, OpenProcessToken},
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
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
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
