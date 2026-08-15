use std::io;

use crate::security::Nfs4Acl;

#[cfg(target_os = "freebsd")]
mod freebsd {
    use super::*;
    use crate::security::{Nfs4Ace, Nfs4AceFlags, Nfs4AceMask, Nfs4AceQualifier, Nfs4AceType};
    use std::{ffi::c_void, ptr};

    type Acl = *mut c_void;
    type Entry = *mut c_void;
    type Permset = *mut u32;
    type Flagset = *mut u16;

    const ACL_TYPE_NFS4: u32 = 0x00000004;
    const ACL_USER_OBJ: u32 = 0x01;
    const ACL_USER: u32 = 0x02;
    const ACL_GROUP_OBJ: u32 = 0x04;
    const ACL_GROUP: u32 = 0x08;
    const ACL_EVERYONE: u32 = 0x40;
    const ACL_FIRST_ENTRY: libc::c_int = 0;
    const ACL_NEXT_ENTRY: libc::c_int = 1;

    const ACL_ENTRY_TYPE_ALLOW: u16 = 0x0100;
    const ACL_ENTRY_TYPE_DENY: u16 = 0x0200;
    const ACL_ENTRY_TYPE_AUDIT: u16 = 0x0400;
    const ACL_ENTRY_TYPE_ALARM: u16 = 0x0800;

    // NFSv4 ae_perm bits.
    const ACL_READ_DATA: u32 = 0x00000008;
    const ACL_WRITE_DATA: u32 = 0x00000010;
    const ACL_APPEND_DATA: u32 = 0x00000020;
    const ACL_READ_NAMED_ATTRS: u32 = 0x00000040;
    const ACL_WRITE_NAMED_ATTRS: u32 = 0x00000080;
    const ACL_EXECUTE: u32 = 0x00000001;
    const ACL_DELETE_CHILD: u32 = 0x00000100;
    const ACL_READ_ATTRIBUTES: u32 = 0x00000200;
    const ACL_WRITE_ATTRIBUTES: u32 = 0x00000400;
    const ACL_DELETE: u32 = 0x00000800;
    const ACL_READ_ACL: u32 = 0x00001000;
    const ACL_WRITE_ACL: u32 = 0x00002000;
    const ACL_WRITE_OWNER: u32 = 0x00004000;
    const ACL_SYNCHRONIZE: u32 = 0x00008000;

    const MASK_BITS: &[(Nfs4AceMask, u32)] = &[
        (Nfs4AceMask::READ_DATA, ACL_READ_DATA),
        (Nfs4AceMask::WRITE_DATA, ACL_WRITE_DATA),
        (Nfs4AceMask::APPEND_DATA, ACL_APPEND_DATA),
        (Nfs4AceMask::READ_NAMED_ATTRS, ACL_READ_NAMED_ATTRS),
        (Nfs4AceMask::WRITE_NAMED_ATTRS, ACL_WRITE_NAMED_ATTRS),
        (Nfs4AceMask::EXECUTE, ACL_EXECUTE),
        (Nfs4AceMask::DELETE_CHILD, ACL_DELETE_CHILD),
        (Nfs4AceMask::READ_ATTRIBUTES, ACL_READ_ATTRIBUTES),
        (Nfs4AceMask::WRITE_ATTRIBUTES, ACL_WRITE_ATTRIBUTES),
        (Nfs4AceMask::DELETE, ACL_DELETE),
        (Nfs4AceMask::READ_ACL, ACL_READ_ACL),
        (Nfs4AceMask::WRITE_ACL, ACL_WRITE_ACL),
        (Nfs4AceMask::WRITE_OWNER, ACL_WRITE_OWNER),
        (Nfs4AceMask::SYNCHRONIZE, ACL_SYNCHRONIZE),
    ];

    // ACL_ENTRY_IDENTIFIER_GROUP (0x0040) is intentionally excluded: it is
    // absorbed into Nfs4AceQualifier::User vs. Group instead of being exposed
    // as a raw flag bit.
    const FLAG_BITS: &[(Nfs4AceFlags, u16)] = &[
        (Nfs4AceFlags::FILE_INHERIT, 0x0001),
        (Nfs4AceFlags::DIRECTORY_INHERIT, 0x0002),
        (Nfs4AceFlags::NO_PROPAGATE_INHERIT, 0x0004),
        (Nfs4AceFlags::INHERIT_ONLY, 0x0008),
        (Nfs4AceFlags::SUCCESSFUL_ACCESS, 0x0010),
        (Nfs4AceFlags::FAILED_ACCESS, 0x0020),
        (Nfs4AceFlags::INHERITED, 0x0080),
    ];

    unsafe extern "C" {
        fn acl_add_flag_np(flagset: Flagset, flag: u16) -> libc::c_int;
        fn acl_add_perm(permset: Permset, permission: u32) -> libc::c_int;
        fn acl_clear_flags_np(flagset: Flagset) -> libc::c_int;
        fn acl_clear_perms(permset: Permset) -> libc::c_int;
        fn acl_create_entry(acl: *mut Acl, entry: *mut Entry) -> libc::c_int;
        fn acl_free(object: *mut c_void) -> libc::c_int;
        fn acl_get_entry(acl: Acl, entry_id: libc::c_int, entry: *mut Entry) -> libc::c_int;
        fn acl_get_entry_type_np(entry: Entry, entry_type: *mut u16) -> libc::c_int;
        fn acl_get_fd_np(fd: libc::c_int, acl_type: u32) -> Acl;
        fn acl_get_file(path: *const libc::c_char, acl_type: u32) -> Acl;
        fn acl_get_flag_np(flagset: Flagset, flag: u16) -> libc::c_int;
        fn acl_get_flagset_np(entry: Entry, flagset: *mut Flagset) -> libc::c_int;
        fn acl_get_link_np(path: *const libc::c_char, acl_type: u32) -> Acl;
        fn acl_get_perm_np(permset: Permset, permission: u32) -> libc::c_int;
        fn acl_get_permset(entry: Entry, permset: *mut Permset) -> libc::c_int;
        fn acl_get_qualifier(entry: Entry) -> *mut c_void;
        fn acl_get_tag_type(entry: Entry, tag: *mut u32) -> libc::c_int;
        fn acl_init(count: libc::c_int) -> Acl;
        fn acl_is_trivial_np(acl: Acl, trivial: *mut libc::c_int) -> libc::c_int;
        fn acl_set_entry_type_np(entry: Entry, entry_type: u16) -> libc::c_int;
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

    fn call(result: libc::c_int) -> io::Result<()> {
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    unsafe fn get_native(path: Option<&std::ffi::CStr>, fd: libc::c_int, follow: bool) -> Acl {
        if fd >= 0 {
            unsafe { acl_get_fd_np(fd, ACL_TYPE_NFS4) }
        } else if follow {
            unsafe { acl_get_file(path.unwrap().as_ptr(), ACL_TYPE_NFS4) }
        } else {
            unsafe { acl_get_link_np(path.unwrap().as_ptr(), ACL_TYPE_NFS4) }
        }
    }

    unsafe fn set_native(
        path: Option<&std::ffi::CStr>,
        fd: libc::c_int,
        follow: bool,
        acl: Acl,
    ) -> libc::c_int {
        if fd >= 0 {
            unsafe { acl_set_fd_np(fd, acl, ACL_TYPE_NFS4) }
        } else if follow {
            unsafe { acl_set_file(path.unwrap().as_ptr(), ACL_TYPE_NFS4, acl) }
        } else {
            unsafe { acl_set_link_np(path.unwrap().as_ptr(), ACL_TYPE_NFS4, acl) }
        }
    }

    pub(super) fn get(
        path: Option<&std::ffi::CStr>,
        fd: libc::c_int,
        follow: bool,
    ) -> io::Result<Option<Nfs4Acl>> {
        let acl = OwnedAcl(unsafe { get_native(path, fd, follow) });
        if acl.0.is_null() {
            let error = io::Error::last_os_error();
            // The filesystem/object does not carry an NFSv4-branded ACL
            // (e.g. it has a POSIX.1e ACL, or no extended ACL at all).
            if error.raw_os_error() == Some(libc::EINVAL) {
                return Ok(None);
            }
            return Err(error);
        }

        // A newly created file/directory on an NFSv4-ACL-enabled filesystem
        // carries a "trivial" ACL synthesized from its mode bits, same as
        // the POSIX.1e case — report that as no ACL rather than decoding it.
        let mut trivial = 0;
        call(unsafe { acl_is_trivial_np(acl.0, &mut trivial) })?;
        if trivial != 0 {
            return Ok(None);
        }

        let mut entries = Vec::new();
        let mut entry = ptr::null_mut();
        let mut result = unsafe { acl_get_entry(acl.0, ACL_FIRST_ENTRY, &mut entry) };
        while result == 1 {
            let mut tag = 0;
            let mut permset = ptr::null_mut();
            let mut flagset = ptr::null_mut();
            let mut entry_type = 0u16;
            call(unsafe { acl_get_tag_type(entry, &mut tag) })?;
            call(unsafe { acl_get_permset(entry, &mut permset) })?;
            call(unsafe { acl_get_flagset_np(entry, &mut flagset) })?;
            call(unsafe { acl_get_entry_type_np(entry, &mut entry_type) })?;

            let qualifier = match tag {
                ACL_USER_OBJ => Nfs4AceQualifier::Owner,
                ACL_GROUP_OBJ => Nfs4AceQualifier::OwningGroup,
                ACL_EVERYONE => Nfs4AceQualifier::Everyone,
                ACL_USER | ACL_GROUP => {
                    let value = unsafe { acl_get_qualifier(entry) };
                    if value.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let id = unsafe { *(value.cast::<u32>()) };
                    unsafe {
                        acl_free(value);
                    }
                    if tag == ACL_USER {
                        Nfs4AceQualifier::User(id)
                    } else {
                        Nfs4AceQualifier::Group(id)
                    }
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "FreeBSD NFSv4 ACL contains an unrecognized entry tag",
                    ));
                }
            };

            let ace_type = match entry_type {
                ACL_ENTRY_TYPE_ALLOW => Nfs4AceType::Allow,
                ACL_ENTRY_TYPE_DENY => Nfs4AceType::Deny,
                ACL_ENTRY_TYPE_AUDIT => Nfs4AceType::Audit,
                ACL_ENTRY_TYPE_ALARM => Nfs4AceType::Alarm,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "FreeBSD NFSv4 ACL entry has an unrecognized entry type",
                    ));
                }
            };

            let mut mask = Nfs4AceMask::empty();
            for (bit, native) in MASK_BITS {
                let value = unsafe { acl_get_perm_np(permset, *native) };
                if value < 0 {
                    return Err(io::Error::last_os_error());
                }
                mask.set(*bit, value != 0);
            }

            let mut flags = Nfs4AceFlags::empty();
            for (bit, native) in FLAG_BITS {
                let value = unsafe { acl_get_flag_np(flagset, *native) };
                if value < 0 {
                    return Err(io::Error::last_os_error());
                }
                flags.set(*bit, value != 0);
            }

            entries.push(Nfs4Ace {
                ace_type,
                qualifier,
                mask,
                flags,
            });
            result = unsafe { acl_get_entry(acl.0, ACL_NEXT_ENTRY, &mut entry) };
        }
        call(result)?;
        if entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Nfs4Acl::new(entries)))
        }
    }

    pub(super) fn set(
        path: Option<&std::ffi::CStr>,
        fd: libc::c_int,
        follow: bool,
        acl: Option<&Nfs4Acl>,
    ) -> io::Result<()> {
        // Unlike a POSIX.1e ACL (an optional extended attribute), an NFSv4
        // ACL is a file's native security descriptor: FreeBSD implements
        // ACL removal as `VOP_SETACL(vp, type, NULL, ...)`, and UFS's NFSv4
        // branch (`ufs_setacl_nfs4`) does not support a null ACL the way its
        // POSIX.1e branch does — there is no operation that clears an
        // NFSv4 ACL back to "none", only ways to replace it with another
        // (possibly mode-equivalent) one.
        let Some(acl) = acl else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "NFSv4 ACLs cannot be removed, only replaced",
            ));
        };

        let mut native = OwnedAcl(unsafe {
            acl_init(
                acl.entries()
                    .len()
                    .try_into()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "ACL is too large"))?,
            )
        });
        if native.0.is_null() {
            return Err(io::Error::last_os_error());
        }
        for ace in acl.entries() {
            let (tag, id) = match ace.qualifier {
                Nfs4AceQualifier::Owner => (ACL_USER_OBJ, None),
                Nfs4AceQualifier::OwningGroup => (ACL_GROUP_OBJ, None),
                Nfs4AceQualifier::Everyone => (ACL_EVERYONE, None),
                Nfs4AceQualifier::User(id) => (ACL_USER, Some(id)),
                Nfs4AceQualifier::Group(id) => (ACL_GROUP, Some(id)),
            };
            let entry_type = match ace.ace_type {
                Nfs4AceType::Allow => ACL_ENTRY_TYPE_ALLOW,
                Nfs4AceType::Deny => ACL_ENTRY_TYPE_DENY,
                Nfs4AceType::Audit => ACL_ENTRY_TYPE_AUDIT,
                Nfs4AceType::Alarm => ACL_ENTRY_TYPE_ALARM,
            };

            let mut entry = ptr::null_mut();
            call(unsafe { acl_create_entry(&mut native.0, &mut entry) })?;
            call(unsafe { acl_set_tag_type(entry, tag) })?;
            if let Some(id) = id {
                call(unsafe { acl_set_qualifier(entry, (&id as *const u32).cast()) })?;
            }
            call(unsafe { acl_set_entry_type_np(entry, entry_type) })?;

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

#[cfg(target_os = "freebsd")]
pub(super) fn get(
    path: Option<&std::ffi::CStr>,
    fd: libc::c_int,
    follow: bool,
) -> io::Result<Option<Nfs4Acl>> {
    freebsd::get(path, fd, follow)
}

#[cfg(target_os = "freebsd")]
pub(super) fn set(
    path: Option<&std::ffi::CStr>,
    fd: libc::c_int,
    follow: bool,
    acl: Option<&Nfs4Acl>,
) -> io::Result<()> {
    freebsd::set(path, fd, follow, acl)
}

#[cfg(not(target_os = "freebsd"))]
pub(super) fn set(
    _path: Option<&std::ffi::CStr>,
    _fd: libc::c_int,
    _follow: bool,
    _acl: Option<&Nfs4Acl>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NFSv4 ACLs are not supported on this platform",
    ))
}
