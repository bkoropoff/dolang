use std::{ffi::CStr, io, os::fd::AsFd, path::Path};

use std::fs::File;

use crate::{
    direct::{Direct, unix::UnixXattrTarget},
    security::{Acl, AclKind, Permission, PosixAce, PosixAcl, PosixAclQualifier},
};

use super::{canonical_entries, cpath, unsupported_kind};

const ACCESS_XATTR: &[u8] = b"system.posix_acl_access\0";
const DEFAULT_XATTR: &[u8] = b"system.posix_acl_default\0";

fn xattr_name(default: bool) -> &'static CStr {
    CStr::from_bytes_with_nul(if default { DEFAULT_XATTR } else { ACCESS_XATTR }).unwrap()
}

fn missing_xattr(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ENODATA)
}

fn decode(bytes: &[u8]) -> io::Result<PosixAcl> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "POSIX ACL xattr has invalid length",
        ));
    }
    let version = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if version != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported POSIX ACL xattr version {version}"),
        ));
    }
    let mut entries = Vec::with_capacity((bytes.len() - 4) / 8);
    for raw in bytes[4..].as_chunks::<8>().0 {
        let tag = u16::from_le_bytes(raw[0..2].try_into().unwrap());
        let perm = u16::from_le_bytes(raw[2..4].try_into().unwrap());
        let id = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        if perm & !7 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "POSIX ACL entry has invalid permissions",
            ));
        }
        let qualifier = match tag {
            0x01 if id == u32::MAX => PosixAclQualifier::UserObj,
            0x02 if id != u32::MAX => PosixAclQualifier::User(id),
            0x04 if id == u32::MAX => PosixAclQualifier::GroupObj,
            0x08 if id != u32::MAX => PosixAclQualifier::Group(id),
            0x10 if id == u32::MAX => PosixAclQualifier::Mask,
            0x20 if id == u32::MAX => PosixAclQualifier::Other,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "POSIX ACL entry has invalid tag or qualifier",
                ));
            }
        };
        entries.push(PosixAce {
            qualifier,
            permissions: Permission::from_bits_truncate(perm as u8),
        });
    }
    PosixAcl::new(entries).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn encode(acl: &PosixAcl) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + acl.entries().len() * 8);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for entry in canonical_entries(acl) {
        let (tag, id) = match entry.qualifier {
            PosixAclQualifier::UserObj => (0x01u16, u32::MAX),
            PosixAclQualifier::User(id) => (0x02, id),
            PosixAclQualifier::GroupObj => (0x04, u32::MAX),
            PosixAclQualifier::Group(id) => (0x08, id),
            PosixAclQualifier::Mask => (0x10, u32::MAX),
            PosixAclQualifier::Other => (0x20, u32::MAX),
        };
        let perm = u16::from(entry.permissions.bits());
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&perm.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes
}

fn get_xattr(target: UnixXattrTarget<'_>, default: bool) -> io::Result<Option<PosixAcl>> {
    match Direct::unix_get_xattr(target, xattr_name(default)) {
        Ok(bytes) => decode(&bytes).map(Some),
        Err(error) if missing_xattr(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn set_xattr(target: UnixXattrTarget<'_>, acl: Option<&PosixAcl>, default: bool) -> io::Result<()> {
    match acl {
        Some(acl) => Direct::unix_set_xattr(target, xattr_name(default), &encode(acl)),
        None => match Direct::unix_remove_xattr(target, xattr_name(default)) {
            Err(error) if missing_xattr(&error) => Ok(()),
            result => result,
        },
    }
}

pub(super) fn get(
    path: &Path,
    kind: AclKind,
    default: bool,
    follow: bool,
) -> io::Result<Option<Acl>> {
    match kind {
        AclKind::Posix => {
            let cpath = cpath(path)?;
            Ok(get_xattr(UnixXattrTarget::Path(&cpath, follow), default)?.map(Acl::Posix))
        }
        AclKind::Nfs4 | AclKind::Macos => Err(unsupported_kind(kind)),
    }
}

pub(super) fn set(
    path: &Path,
    kind: AclKind,
    acl: Option<&Acl>,
    default: bool,
    follow: bool,
) -> io::Result<()> {
    match kind {
        AclKind::Posix => {
            let acl = super::split_posix(kind, acl)?;
            let cpath = cpath(path)?;
            set_xattr(UnixXattrTarget::Path(&cpath, follow), acl, default)
        }
        AclKind::Nfs4 | AclKind::Macos => Err(unsupported_kind(kind)),
    }
}

pub(super) fn get_fd(file: &File, kind: AclKind, default: bool) -> io::Result<Option<Acl>> {
    match kind {
        AclKind::Posix => {
            Ok(get_xattr(UnixXattrTarget::Fd(file.as_fd()), default)?.map(Acl::Posix))
        }
        AclKind::Nfs4 | AclKind::Macos => Err(unsupported_kind(kind)),
    }
}

pub(super) fn set_fd(
    file: &File,
    kind: AclKind,
    acl: Option<&Acl>,
    default: bool,
) -> io::Result<()> {
    match kind {
        AclKind::Posix => {
            let acl = super::split_posix(kind, acl)?;
            set_xattr(UnixXattrTarget::Fd(file.as_fd()), acl, default)
        }
        AclKind::Nfs4 | AclKind::Macos => Err(unsupported_kind(kind)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_acl_packet_round_trip() {
        let acl = PosixAcl::new(vec![
            PosixAce {
                qualifier: PosixAclQualifier::UserObj,
                permissions: Permission::READ | Permission::WRITE,
            },
            PosixAce {
                qualifier: PosixAclQualifier::GroupObj,
                permissions: Permission::READ,
            },
            PosixAce {
                qualifier: PosixAclQualifier::Other,
                permissions: Permission::empty(),
            },
        ])
        .unwrap();
        assert_eq!(decode(&encode(&acl)).unwrap(), acl);
    }
}
