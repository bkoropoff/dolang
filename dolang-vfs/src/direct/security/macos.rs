use std::{
    ffi::CStr,
    os::fd::{AsFd, AsRawFd},
    path::Path,
    ptr,
};

use std::fs::File;
use uuid::Uuid;

use crate::error::{Error, ErrorKind, Result};
use crate::security::{
    Acl, AclKind, MacosAce, MacosAceFlags, MacosAceMask, MacosAceType, MacosAcl,
};

use super::{cpath, unsupported_kind};

fn call(result: libc::c_int) -> Result<()> {
    if result < 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

// --- macOS extended ACLs (ACL_TYPE_EXTENDED) ---

mod acl {
    use super::*;
    use std::{ffi::c_void, os::unix::ffi::OsStrExt};

    type Acl = *mut c_void;
    type Entry = *mut c_void;
    type Permset = *mut c_void;
    type Flagset = *mut c_void;

    const ACL_TYPE_EXTENDED: u32 = 0x00000100;
    const ACL_FIRST_ENTRY: libc::c_int = 0;
    const ACL_NEXT_ENTRY: libc::c_int = -1;

    // acl_tag_t: allow/deny is the entry's tag, not a separate entry-type
    // field the way FreeBSD's NFSv4 ACLs work.
    const ACL_EXTENDED_ALLOW: u32 = 1;
    const ACL_EXTENDED_DENY: u32 = 2;

    // acl_perm_t bits.
    const ACL_READ_DATA: u32 = 1 << 1;
    const ACL_WRITE_DATA: u32 = 1 << 2;
    const ACL_EXECUTE: u32 = 1 << 3;
    const ACL_DELETE: u32 = 1 << 4;
    const ACL_APPEND_DATA: u32 = 1 << 5;
    const ACL_DELETE_CHILD: u32 = 1 << 6;
    const ACL_READ_ATTRIBUTES: u32 = 1 << 7;
    const ACL_WRITE_ATTRIBUTES: u32 = 1 << 8;
    const ACL_READ_EXTATTRIBUTES: u32 = 1 << 9;
    const ACL_WRITE_EXTATTRIBUTES: u32 = 1 << 10;
    const ACL_READ_SECURITY: u32 = 1 << 11;
    const ACL_WRITE_SECURITY: u32 = 1 << 12;
    const ACL_CHANGE_OWNER: u32 = 1 << 13;
    const ACL_SYNCHRONIZE: u32 = 1 << 20;

    // acl_flag_t bits (entry-scoped subset; ACL_FLAG_DEFER_INHERIT and
    // ACL_FLAG_NO_INHERIT are ACL-scoped, not entry-scoped, and have no
    // equivalent in the portable representation).
    const ACL_ENTRY_INHERITED: u32 = 1 << 4;
    const ACL_ENTRY_FILE_INHERIT: u32 = 1 << 5;
    const ACL_ENTRY_DIRECTORY_INHERIT: u32 = 1 << 6;
    const ACL_ENTRY_LIMIT_INHERIT: u32 = 1 << 7;
    const ACL_ENTRY_ONLY_INHERIT: u32 = 1 << 8;

    const MASK_BITS: &[(MacosAceMask, u32)] = &[
        (MacosAceMask::READ_DATA, ACL_READ_DATA),
        (MacosAceMask::WRITE_DATA, ACL_WRITE_DATA),
        (MacosAceMask::EXECUTE, ACL_EXECUTE),
        (MacosAceMask::DELETE, ACL_DELETE),
        (MacosAceMask::APPEND_DATA, ACL_APPEND_DATA),
        (MacosAceMask::DELETE_CHILD, ACL_DELETE_CHILD),
        (MacosAceMask::READ_ATTRIBUTES, ACL_READ_ATTRIBUTES),
        (MacosAceMask::WRITE_ATTRIBUTES, ACL_WRITE_ATTRIBUTES),
        (MacosAceMask::READ_EXTATTRIBUTES, ACL_READ_EXTATTRIBUTES),
        (MacosAceMask::WRITE_EXTATTRIBUTES, ACL_WRITE_EXTATTRIBUTES),
        (MacosAceMask::READ_SECURITY, ACL_READ_SECURITY),
        (MacosAceMask::WRITE_SECURITY, ACL_WRITE_SECURITY),
        (MacosAceMask::CHANGE_OWNER, ACL_CHANGE_OWNER),
        (MacosAceMask::SYNCHRONIZE, ACL_SYNCHRONIZE),
    ];

    const FLAG_BITS: &[(MacosAceFlags, u32)] = &[
        (MacosAceFlags::INHERITED, ACL_ENTRY_INHERITED),
        (MacosAceFlags::FILE_INHERIT, ACL_ENTRY_FILE_INHERIT),
        (
            MacosAceFlags::DIRECTORY_INHERIT,
            ACL_ENTRY_DIRECTORY_INHERIT,
        ),
        (MacosAceFlags::LIMIT_INHERIT, ACL_ENTRY_LIMIT_INHERIT),
        (MacosAceFlags::ONLY_INHERIT, ACL_ENTRY_ONLY_INHERIT),
    ];

    unsafe extern "C" {
        fn acl_add_flag_np(flagset: Flagset, flag: u32) -> libc::c_int;
        fn acl_add_perm(permset: Permset, permission: u32) -> libc::c_int;
        fn acl_clear_flags_np(flagset: Flagset) -> libc::c_int;
        fn acl_clear_perms(permset: Permset) -> libc::c_int;
        fn acl_create_entry(acl: *mut Acl, entry: *mut Entry) -> libc::c_int;
        fn acl_free(object: *mut c_void) -> libc::c_int;
        fn acl_get_entry(acl: Acl, entry_id: libc::c_int, entry: *mut Entry) -> libc::c_int;
        fn acl_get_fd_np(fd: libc::c_int, acl_type: u32) -> Acl;
        fn acl_get_file(path: *const libc::c_char, acl_type: u32) -> Acl;
        fn acl_get_flag_np(flagset: Flagset, flag: u32) -> libc::c_int;
        fn acl_get_flagset_np(entry: Entry, flagset: *mut Flagset) -> libc::c_int;
        fn acl_get_link_np(path: *const libc::c_char, acl_type: u32) -> Acl;
        fn acl_get_perm_np(permset: Permset, permission: u32) -> libc::c_int;
        fn acl_get_permset(entry: Entry, permset: *mut Permset) -> libc::c_int;
        fn acl_get_qualifier(entry: Entry) -> *mut c_void;
        fn acl_get_tag_type(entry: Entry, tag: *mut u32) -> libc::c_int;
        fn acl_init(count: libc::c_int) -> Acl;
        fn acl_set_fd_np(fd: libc::c_int, acl: Acl, acl_type: u32) -> libc::c_int;
        fn acl_set_file(path: *const libc::c_char, acl_type: u32, acl: Acl) -> libc::c_int;
        fn acl_set_flagset_np(entry: Entry, flagset: Flagset) -> libc::c_int;
        fn acl_set_link_np(path: *const libc::c_char, acl_type: u32, acl: Acl) -> libc::c_int;
        fn acl_set_permset(entry: Entry, permset: Permset) -> libc::c_int;
        fn acl_set_qualifier(entry: Entry, qualifier: *const c_void) -> libc::c_int;
        fn acl_set_tag_type(entry: Entry, tag: u32) -> libc::c_int;
    }

    struct OwnedAcl(Acl);

    impl Drop for OwnedAcl {
        fn drop(&mut self) {
            unsafe {
                acl_free(self.0);
            }
        }
    }

    // Unlike FreeBSD's NFSv4 ACL API (EINVAL for "not applicable at all"),
    // Darwin's acl_get_{file,link_np,fd_np} report a target with no
    // extended ACL by returning NULL with errno set to ENOENT -- the same
    // errno a genuinely-missing path produces. Disambiguate by checking
    // whether the target actually exists before treating ENOENT as "no
    // ACL"; an fd is already open, so ENOENT on that path can only mean
    // "no ACL".
    fn target_exists(path: Option<&CStr>, fd: libc::c_int, follow: bool) -> bool {
        if fd >= 0 {
            return true;
        }
        let Some(path) = path else { return false };
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
        if follow {
            path.exists()
        } else {
            path.symlink_metadata().is_ok()
        }
    }

    unsafe fn get_native(path: Option<&CStr>, fd: libc::c_int, follow: bool) -> Acl {
        if fd >= 0 {
            unsafe { acl_get_fd_np(fd, ACL_TYPE_EXTENDED) }
        } else if follow {
            unsafe { acl_get_file(path.unwrap().as_ptr(), ACL_TYPE_EXTENDED) }
        } else {
            unsafe { acl_get_link_np(path.unwrap().as_ptr(), ACL_TYPE_EXTENDED) }
        }
    }

    unsafe fn set_native(
        path: Option<&CStr>,
        fd: libc::c_int,
        follow: bool,
        acl: Acl,
    ) -> libc::c_int {
        if fd >= 0 {
            unsafe { acl_set_fd_np(fd, acl, ACL_TYPE_EXTENDED) }
        } else if follow {
            unsafe { acl_set_file(path.unwrap().as_ptr(), ACL_TYPE_EXTENDED, acl) }
        } else {
            unsafe { acl_set_link_np(path.unwrap().as_ptr(), ACL_TYPE_EXTENDED, acl) }
        }
    }

    pub(super) fn get(
        path: Option<&CStr>,
        fd: libc::c_int,
        follow: bool,
    ) -> Result<Option<MacosAcl>> {
        let acl = OwnedAcl(unsafe { get_native(path, fd, follow) });
        if acl.0.is_null() {
            let error = Error::last_os_error();
            // The target has no extended ACL at all.
            let no_acl = match error.raw_os_error() {
                Some(libc::EINVAL) => true,
                Some(libc::ENOENT) => target_exists(path, fd, follow),
                _ => false,
            };
            if no_acl {
                return Ok(None);
            }
            return Err(error);
        }

        let mut entries = Vec::new();
        let mut entry = ptr::null_mut();
        let mut entry_id = ACL_FIRST_ENTRY;
        loop {
            // Unlike FreeBSD's POSIX-draft convention (1 = entry returned, 0
            // = no more entries, -1 = error), Darwin's acl_get_entry()
            // returns 0 on success and -1 with errno EINVAL when the list is
            // exhausted (per the reference implementation in
            // apple-oss-distributions/Libc's posix1e/acl_entry.c, which only
            // ever sets EINVAL here for an out-of-range index -- and we only
            // ever pass ACL_FIRST_ENTRY/ACL_NEXT_ENTRY, so EINVAL from this
            // call can only mean "no more entries" in our usage).
            if unsafe { acl_get_entry(acl.0, entry_id, &mut entry) } != 0 {
                let error = Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINVAL) {
                    break;
                }
                return Err(error);
            }
            entry_id = ACL_NEXT_ENTRY;

            let mut tag = 0;
            let mut permset = ptr::null_mut();
            let mut flagset = ptr::null_mut();
            call(unsafe { acl_get_tag_type(entry, &mut tag) })?;
            call(unsafe { acl_get_permset(entry, &mut permset) })?;
            call(unsafe { acl_get_flagset_np(entry, &mut flagset) })?;

            let ace_type = match tag {
                ACL_EXTENDED_ALLOW => MacosAceType::Allow,
                ACL_EXTENDED_DENY => MacosAceType::Deny,
                _ => {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        "macOS extended ACL entry has an unrecognized tag",
                    ));
                }
            };

            let value = unsafe { acl_get_qualifier(entry) };
            if value.is_null() {
                return Err(Error::last_os_error());
            }
            // SAFETY: for ACL_EXTENDED_ALLOW/ACL_EXTENDED_DENY entries the
            // qualifier is always a 16-byte guid_t.
            let qualifier = Uuid::from_bytes(unsafe { *(value.cast::<[u8; 16]>()) });
            unsafe {
                acl_free(value);
            }

            let mut mask = MacosAceMask::empty();
            for (bit, native) in MASK_BITS {
                let value = unsafe { acl_get_perm_np(permset, *native) };
                if value < 0 {
                    return Err(Error::last_os_error());
                }
                mask.set(*bit, value != 0);
            }

            let mut flags = MacosAceFlags::empty();
            for (bit, native) in FLAG_BITS {
                let value = unsafe { acl_get_flag_np(flagset, *native) };
                if value < 0 {
                    return Err(Error::last_os_error());
                }
                flags.set(*bit, value != 0);
            }

            entries.push(MacosAce {
                ace_type,
                qualifier,
                mask,
                flags,
            });
        }
        if entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(MacosAcl::new(entries)))
        }
    }

    pub(super) fn set(
        path: Option<&CStr>,
        fd: libc::c_int,
        follow: bool,
        acl: Option<&MacosAcl>,
    ) -> Result<()> {
        let entries: &[MacosAce] = acl.map(MacosAcl::entries).unwrap_or_default();

        let mut native = OwnedAcl(unsafe {
            acl_init(
                entries
                    .len()
                    .try_into()
                    .map_err(|_| Error::new(ErrorKind::InvalidInput, "ACL is too large"))?,
            )
        });
        if native.0.is_null() {
            return Err(Error::last_os_error());
        }
        for ace in entries {
            let tag = match ace.ace_type {
                MacosAceType::Allow => ACL_EXTENDED_ALLOW,
                MacosAceType::Deny => ACL_EXTENDED_DENY,
            };

            let mut entry = ptr::null_mut();
            call(unsafe { acl_create_entry(&mut native.0, &mut entry) })?;
            call(unsafe { acl_set_tag_type(entry, tag) })?;
            let qualifier = *ace.qualifier.as_bytes();
            call(unsafe { acl_set_qualifier(entry, qualifier.as_ptr().cast()) })?;

            let mut permset = ptr::null_mut();
            call(unsafe { acl_get_permset(entry, &mut permset) })?;
            call(unsafe { acl_clear_perms(permset) })?;
            for (bit, native_bit) in MASK_BITS {
                if ace.mask.contains(*bit) {
                    call(unsafe { acl_add_perm(permset, *native_bit) })?;
                }
            }
            call(unsafe { acl_set_permset(entry, permset) })?;

            let mut flagset = ptr::null_mut();
            call(unsafe { acl_get_flagset_np(entry, &mut flagset) })?;
            call(unsafe { acl_clear_flags_np(flagset) })?;
            for (bit, native_bit) in FLAG_BITS {
                if ace.flags.contains(*bit) {
                    call(unsafe { acl_add_flag_np(flagset, *native_bit) })?;
                }
            }
            call(unsafe { acl_set_flagset_np(entry, flagset) })?;
        }
        call(unsafe { set_native(path, fd, follow, native.0) })
    }
}

pub(super) fn get(path: &Path, kind: AclKind, follow: bool) -> Result<Option<Acl>> {
    match kind {
        AclKind::Macos => {
            let cpath = cpath(path)?;
            Ok(acl::get(Some(&cpath), -1, follow)?.map(Acl::Macos))
        }
        AclKind::Posix | AclKind::Nfs4 => Err(unsupported_kind(kind)),
    }
}

pub(super) fn set(path: &Path, kind: AclKind, acl: Option<&Acl>, follow: bool) -> Result<()> {
    match kind {
        AclKind::Macos => {
            let cpath = cpath(path)?;
            acl::set(Some(&cpath), -1, follow, super::split_macos(kind, acl)?)
        }
        AclKind::Posix | AclKind::Nfs4 => Err(unsupported_kind(kind)),
    }
}

pub(super) fn get_fd(file: &File, kind: AclKind) -> Result<Option<Acl>> {
    match kind {
        AclKind::Macos => Ok(acl::get(None, file.as_fd().as_raw_fd(), true)?.map(Acl::Macos)),
        AclKind::Posix | AclKind::Nfs4 => Err(unsupported_kind(kind)),
    }
}

pub(super) fn set_fd(file: &File, kind: AclKind, acl: Option<&Acl>) -> Result<()> {
    match kind {
        AclKind::Macos => acl::set(
            None,
            file.as_fd().as_raw_fd(),
            true,
            super::split_macos(kind, acl)?,
        ),
        AclKind::Posix | AclKind::Nfs4 => Err(unsupported_kind(kind)),
    }
}

// --- Principal ID resolution (Membership framework: uid/gid <-> guid_t) ---
//
// Signatures below are transcribed from Apple's public `membership.h` and
// have not been compiled/verified on this (Linux) machine -- confirm
// against a real macOS SDK/CI build before relying on them.

mod mbr {
    use super::*;

    const ID_TYPE_UID: libc::c_int = 0;
    const ID_TYPE_GID: libc::c_int = 1;

    unsafe extern "C" {
        fn mbr_uid_to_uuid(uid: libc::uid_t, uu: *mut u8) -> libc::c_int;
        fn mbr_gid_to_uuid(gid: libc::gid_t, uu: *mut u8) -> libc::c_int;
        fn mbr_uuid_to_id(uu: *const u8, id: *mut u32, id_type: *mut libc::c_int) -> libc::c_int;
    }

    fn call(result: libc::c_int) -> Result<()> {
        // mbr_*() functions return an error number directly on failure,
        // rather than -1 with errno set.
        if result != 0 {
            Err(Error::from_raw_os_error(result))
        } else {
            Ok(())
        }
    }

    pub(super) fn uid_to_uuid(uid: u32) -> Result<Uuid> {
        let mut uu = [0u8; 16];
        call(unsafe { mbr_uid_to_uuid(uid, uu.as_mut_ptr()) })?;
        Ok(Uuid::from_bytes(uu))
    }

    pub(super) fn gid_to_uuid(gid: u32) -> Result<Uuid> {
        let mut uu = [0u8; 16];
        call(unsafe { mbr_gid_to_uuid(gid, uu.as_mut_ptr()) })?;
        Ok(Uuid::from_bytes(uu))
    }

    pub(super) enum ResolvedId {
        Uid(u32),
        Gid(u32),
    }

    pub(super) fn uuid_to_id(uuid: Uuid) -> Result<ResolvedId> {
        let mut id = 0u32;
        let mut id_type = 0;
        call(unsafe { mbr_uuid_to_id(uuid.as_bytes().as_ptr(), &mut id, &mut id_type) })?;
        match id_type {
            ID_TYPE_UID => Ok(ResolvedId::Uid(id)),
            ID_TYPE_GID => Ok(ResolvedId::Gid(id)),
            _ => Err(Error::other(
                "mbr_uuid_to_id returned an unrecognized id type",
            )),
        }
    }
}

fn invalid_combo() -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        "unsupported principal ID conversion",
    )
}

pub(super) fn resolve_principal_id(
    input: crate::security::PrincipalId,
    want: crate::security::PrincipalIdKind,
) -> Result<crate::security::PrincipalId> {
    use crate::security::{PrincipalId, PrincipalIdKind};

    match (input, want) {
        (PrincipalId::Uid(uid), PrincipalIdKind::Uuid) => {
            mbr::uid_to_uuid(uid).map(PrincipalId::Uuid)
        }
        (PrincipalId::Gid(gid), PrincipalIdKind::Uuid) => {
            mbr::gid_to_uuid(gid).map(PrincipalId::Uuid)
        }
        (PrincipalId::Uuid(uuid), PrincipalIdKind::Uid | PrincipalIdKind::Gid) => {
            match mbr::uuid_to_id(uuid)? {
                mbr::ResolvedId::Uid(uid) => Ok(PrincipalId::Uid(uid)),
                mbr::ResolvedId::Gid(gid) => Ok(PrincipalId::Gid(gid)),
            }
        }
        _ => Err(invalid_combo()),
    }
}
