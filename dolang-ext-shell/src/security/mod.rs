use std::{
    hash::Hash,
    ops::{BitAnd, BitOr, BitXor, Not},
};

use dolang::{
    compile::Compiler,
    runtime::{
        Args, Error, Format, Instance, Object, Output, Result, Slot, State, Strand, Sym, Type,
        Value,
        object::{
            ArrayLike, ArrayView, FlagLike, Flags, FlagsInstanceExt, FlagsTypeExt, Mut, Ref,
            TypeBuilder, fmt,
        },
        unpack,
        value::{AsTuple, Dict, Nil},
        vm::Builder,
    },
};

use dolang_vfs::{
    security::{
        Acl as VfsAnyAcl, AclKind as VfsAclKind, MacosAce as VfsMacosAce,
        MacosAceFlags as VfsMacosAceFlags, MacosAceMask as VfsMacosAceMask,
        MacosAceType as VfsMacosAceType, MacosAcl as VfsMacosAcl, Nfs4Ace as VfsNfs4Ace,
        Nfs4AceFlags as VfsNfs4AceFlags, Nfs4AceMask as VfsNfs4AceMask,
        Nfs4AceQualifier as VfsNfs4AceQualifier, Nfs4AceType as VfsNfs4AceType,
        Nfs4Acl as VfsNfs4Acl, Permission as VfsPermission, PosixAce as VfsPosixAce,
        PosixAcl as VfsPosixAcl, PosixAclQualifier as VfsPosixAclQualifier,
        PrincipalId as VfsPrincipalId, PrincipalIdKind as VfsPrincipalIdKind, SecurityInfo,
        SidName as VfsSidName, SidNameUse, TokenGroup as VfsTokenGroup, UnixSecurityInfo,
        WindowsTokenInfo,
    },
    target::{OperatingSystem, OperatingSystemFamily},
};

use dolang_winterop::{
    guid::Guid,
    security::{
        AccessMask as WinAccessMask, Ace as VfsAce, AceBuf as VfsAceBuf, AceBuildOptions,
        AceFlags as WinAceFlags, AceType as VfsAceType, Acl as VfsAcl, AclBuf as VfsAclBuf,
        AclRevision, SecDesc as VfsSecDesc, SecDescControl as WinSecDescControl,
        SecDescUpdate as VfsSecDescUpdate, SecInfo as WinSecInfo, Sid as VfsSid,
        SidIdentifierAuthority, TokenGroupAttributes as WinTokenGroupAttributes, WellKnownSid,
    },
};

use crate::{error, global::Global, util};

pub(crate) fn configure_compiler<'a>(_compiler: &mut Compiler<'a>) {}

macro_rules! flags_ops {
    ($name:ident) => {
        impl BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self {
                Self(self.0 | rhs.0)
            }
        }
        impl BitAnd for $name {
            type Output = Self;
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }
        impl BitXor for $name {
            type Output = Self;
            fn bitxor(self, rhs: Self) -> Self {
                Self(self.0 ^ rhs.0)
            }
        }
        impl Not for $name {
            type Output = Self;
            fn not(self) -> Self {
                Self(!self.0)
            }
        }
    };
}

mod macos;
mod nfs4;
mod unix;
mod windows;

pub(crate) use macos::{
    MacosAceFlags, MacosAceMask, MacosAceObject, MacosAclObject, create_macos_acl,
};
pub(crate) use nfs4::{Nfs4AceFlags, Nfs4AceMask, Nfs4AceObject, Nfs4AclObject, create_nfs4_acl};
pub(crate) use unix::{Identity, Permission, PosixAceObject, PosixAclObject, create_posix_acl};
pub use windows::AccessMask;
pub(crate) use windows::{
    Ace, AceFlags, Acl, SecDesc, SecDescControl, SecInfo, Sid, SidName, SpecPath, TokenGroup,
    TokenGroupAttributes, TokenInfo, WellKnownSids, create_sec_desc, create_sid,
    sec_desc_from_args,
};

/// Converts a portable [`dolang_vfs::security::Acl`] into the appropriate
/// Do-facing ACL value (`security.unix.Acl`, `security.nfs4.Acl`, or
/// `security.macos.Acl`).
pub(crate) fn create_any_acl<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    acl: Option<VfsAnyAcl>,
    out: &mut Slot<'v, '_>,
) {
    match acl {
        None => Output::set(strand, out, Nil),
        Some(VfsAnyAcl::Posix(acl)) => create_posix_acl(strand, global, Some(acl), out),
        Some(VfsAnyAcl::Nfs4(acl)) => create_nfs4_acl(strand, global, Some(acl), out),
        Some(VfsAnyAcl::Macos(acl)) => create_macos_acl(strand, global, Some(acl), out),
        Some(_) => Output::set(strand, out, Nil),
    }
}

/// Infers the ACL kind from `value`'s dynamic type: a `security.unix.Acl`
/// yields [`VfsAclKind::Posix`], a `security.nfs4.Acl` yields
/// [`VfsAclKind::Nfs4`], a `security.macos.Acl` yields [`VfsAclKind::Macos`].
/// `nil` yields `None`, since there is nothing to infer from; callers fall
/// back to a `kind:` keyword argument in that case.
pub(crate) fn acl_from_value<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    value: &Value<'v>,
) -> Result<'v, 's, Option<VfsAnyAcl>> {
    if value.is_nil() {
        return Ok(None);
    }
    if let Some(acl) = global.types.posix_acl.cast(value) {
        return Ok(Some(VfsAnyAcl::Posix(
            acl.enter_sync(strand, |_strand, acl| acl.annex().clone()),
        )));
    }
    if let Some(acl) = global.types.nfs4_acl.cast(value) {
        return Ok(Some(VfsAnyAcl::Nfs4(
            acl.enter_sync(strand, |_strand, acl| acl.annex().clone()),
        )));
    }
    if let Some(acl) = global.types.macos_acl.cast(value) {
        return Ok(Some(VfsAnyAcl::Macos(
            acl.enter_sync(strand, |_strand, acl| acl.annex().clone()),
        )));
    }
    Err(Error::type_error(
        strand,
        "expected security.unix.Acl, security.nfs4.Acl, security.macos.Acl, or nil",
    ))
}

/// Parses the `kind:` keyword argument (`:POSIX:`/`:NFS4:`/`:MACOS:`),
/// defaulting to [`VfsAclKind::Posix`] when absent.
pub(crate) fn acl_kind_sym<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    slot: Option<Slot<'v, '_>>,
) -> Result<'v, 's, VfsAclKind> {
    let Some(slot) = slot else {
        return Ok(VfsAclKind::Posix);
    };
    let sym = slot
        .as_sym(strand)
        .ok_or_else(|| Error::type_error(strand, "kind: expected :POSIX:, :NFS4:, or :MACOS:"))?;
    if sym == global.syms.posix {
        Ok(VfsAclKind::Posix)
    } else if sym == global.syms.nfs4 {
        Ok(VfsAclKind::Nfs4)
    } else if sym == global.syms.macos {
        Ok(VfsAclKind::Macos)
    } else {
        Err(Error::value(
            strand,
            "kind: expected :POSIX:, :NFS4:, or :MACOS:",
        ))
    }
}

fn security_info<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
) -> Result<'v, 's, SecurityInfo> {
    Ok(global.local.get(strand).security())
}

pub(crate) fn configure_vm<'v>(builder: &mut Builder<'v>, global: State<'v, Global<'v>>) {
    builder
        .module("security")
        .function("user_name", async move |strand, args, out| {
            let ([], []) = unpack!(strand, args, 0, 0)?;
            let family = global.local.get(strand).target().os().family();
            let vfs = global.local.get(strand).vfs();
            let name = match family {
                OperatingSystemFamily::Unix => {
                    let security = security_info(strand, global)?;
                    let Some(info) = security.unix() else {
                        unreachable!("Unix target returned Windows security information")
                    };
                    error::io_result(strand, vfs.user_name(info.uid()).await)?
                }
                OperatingSystemFamily::Windows => {
                    let security = security_info(strand, global)?;
                    let Some(info) = security.windows() else {
                        unreachable!("Windows target returned Unix security information")
                    };
                    error::io_result(strand, vfs.sid_name(info.user_sid()).await)?
                        .name()
                        .to_owned()
                }
            };
            Output::set(strand, out, name.as_str());
            Ok(())
        })
        .commit();

    unix::configure_vm(builder, global);
    macos::configure_vm(builder, global);
    nfs4::configure_vm(builder, global);
    windows::configure_vm(builder, global);
}
