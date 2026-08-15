use std::{
    hash::Hash,
    ops::{BitAnd, BitOr, BitXor, Not},
};

use dolang::runtime::object::fmt;

use dolang::{
    compile::Compiler,
    runtime::{
        Args, Error, Instance, Object, Output, Result, Slot, State, Strand, Type, Value,
        object::{
            ArrayLike, ArrayView, FlagLike, FlagsInstanceExt, FlagsTypeExt, Mut, Ref, TypeBuilder,
        },
        unpack,
        value::{AsTuple, Nil},
        vm::Builder,
    },
};
use dolang_vfs::{
    Vfs as _,
    security::{
        Acl as VfsAnyAcl, AclKind as VfsAclKind, MacosAce as VfsMacosAce,
        MacosAceFlags as VfsMacosAceFlags, MacosAceMask as VfsMacosAceMask,
        MacosAceType as VfsMacosAceType, MacosAcl as VfsMacosAcl, Nfs4Ace as VfsNfs4Ace,
        Nfs4AceFlags as VfsNfs4AceFlags, Nfs4AceMask as VfsNfs4AceMask,
        Nfs4AceQualifier as VfsNfs4AceQualifier, Nfs4AceType as VfsNfs4AceType,
        Nfs4Acl as VfsNfs4Acl, Permission as VfsPermission, PosixAce as VfsPosixAce,
        PosixAcl as VfsPosixAcl, PosixAclQualifier as VfsPosixAclQualifier, SecurityInfo,
        SidName as VfsSidName, SidNameUse, TokenGroup as VfsTokenGroup, UnixSecurityInfo,
        WindowsTokenInfo,
    },
    target::OperatingSystemFamily,
};
use dolang_winterop::security::{
    AccessMask as WinAccessMask, SecDescControl as WinSecDescControl, SecInfo as WinSecInfo,
    TokenGroupAttributes as WinTokenGroupAttributes,
};
use dolang_winterop::security::{
    Ace as VfsAce, AceBuf as VfsAceBuf, AceBuildOptions, AceFlags, AceType as VfsAceType,
    Acl as VfsAcl, AclBuf as VfsAclBuf, AclRevision, SecDesc as VfsSecDesc,
    SecDescUpdate as VfsSecDescUpdate, Sid as VfsSid, SidIdentifierAuthority,
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecInfo(pub WinSecInfo);
flags_ops!(SecInfo);
impl FlagLike for SecInfo {
    const ZERO: Self = Self(WinSecInfo::empty());
    const MODULE: &'static str = "security.windows";
    const NAME: &'static str = "SecInfo";
    const BITS: &'static [(&'static str, Self)] = &[
        ("OWNER", Self(WinSecInfo::OWNER)),
        ("GROUP", Self(WinSecInfo::GROUP)),
        ("DACL", Self(WinSecInfo::DACL)),
        ("SACL", Self(WinSecInfo::SACL)),
        ("ALL", Self(WinSecInfo::ALL)),
    ];
    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenGroupAttributes(pub WinTokenGroupAttributes);
flags_ops!(TokenGroupAttributes);
impl FlagLike for TokenGroupAttributes {
    const ZERO: Self = Self(WinTokenGroupAttributes::empty());
    const MODULE: &'static str = "security.windows";
    const NAME: &'static str = "TokenGroupAttributes";
    const BITS: &'static [(&'static str, Self)] = &[
        ("MANDATORY", Self(WinTokenGroupAttributes::MANDATORY)),
        (
            "ENABLED_BY_DEFAULT",
            Self(WinTokenGroupAttributes::ENABLED_BY_DEFAULT),
        ),
        ("ENABLED", Self(WinTokenGroupAttributes::ENABLED)),
        ("OWNER", Self(WinTokenGroupAttributes::OWNER)),
        (
            "USE_FOR_DENY_ONLY",
            Self(WinTokenGroupAttributes::USE_FOR_DENY_ONLY),
        ),
        ("INTEGRITY", Self(WinTokenGroupAttributes::INTEGRITY)),
        (
            "INTEGRITY_ENABLED",
            Self(WinTokenGroupAttributes::INTEGRITY_ENABLED),
        ),
        ("RESOURCE", Self(WinTokenGroupAttributes::RESOURCE)),
        ("LOGON_ID", Self(WinTokenGroupAttributes::LOGON_ID)),
    ];
    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

/// Generic Windows `ACCESS_MASK` bits (`security.windows.AccessMask`), a
/// local newtype over [`dolang_winterop::security::AccessMask`]'s bit values so
/// [`FlagLike`] can be implemented here (both the trait and
/// `dolang_winterop::security::AccessMask` are foreign to this crate).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccessMask(pub WinAccessMask);

impl AccessMask {
    pub const DELETE: AccessMask = AccessMask(WinAccessMask::DELETE);
    pub const READ_CONTROL: AccessMask = AccessMask(WinAccessMask::READ_CONTROL);
    pub const WRITE_DAC: AccessMask = AccessMask(WinAccessMask::WRITE_DAC);
    pub const WRITE_OWNER: AccessMask = AccessMask(WinAccessMask::WRITE_OWNER);
    pub const SYNCHRONIZE: AccessMask = AccessMask(WinAccessMask::SYNCHRONIZE);
    pub const STANDARD_RIGHTS_REQUIRED: AccessMask =
        AccessMask(WinAccessMask::STANDARD_RIGHTS_REQUIRED);
    pub const STANDARD_RIGHTS_ALL: AccessMask = AccessMask(WinAccessMask::STANDARD_RIGHTS_ALL);
    pub const ACCESS_SYSTEM_SECURITY: AccessMask =
        AccessMask(WinAccessMask::ACCESS_SYSTEM_SECURITY);
    pub const MAXIMUM_ALLOWED: AccessMask = AccessMask(WinAccessMask::MAXIMUM_ALLOWED);
    pub const GENERIC_ALL: AccessMask = AccessMask(WinAccessMask::GENERIC_ALL);
    pub const GENERIC_EXECUTE: AccessMask = AccessMask(WinAccessMask::GENERIC_EXECUTE);
    pub const GENERIC_WRITE: AccessMask = AccessMask(WinAccessMask::GENERIC_WRITE);
    pub const GENERIC_READ: AccessMask = AccessMask(WinAccessMask::GENERIC_READ);
}

impl BitOr for AccessMask {
    type Output = AccessMask;
    fn bitor(self, rhs: AccessMask) -> AccessMask {
        AccessMask(self.0 | rhs.0)
    }
}

impl BitAnd for AccessMask {
    type Output = AccessMask;
    fn bitand(self, rhs: AccessMask) -> AccessMask {
        AccessMask(self.0 & rhs.0)
    }
}

impl BitXor for AccessMask {
    type Output = AccessMask;
    fn bitxor(self, rhs: AccessMask) -> AccessMask {
        AccessMask(self.0 ^ rhs.0)
    }
}

impl Not for AccessMask {
    type Output = AccessMask;
    fn not(self) -> AccessMask {
        AccessMask(!self.0)
    }
}

impl FlagLike for AccessMask {
    const ZERO: AccessMask = AccessMask(WinAccessMask::empty());
    const MODULE: &'static str = "security.windows";
    const NAME: &'static str = "AccessMask";
    const BITS: &'static [(&'static str, AccessMask)] = &[
        ("DELETE", AccessMask::DELETE),
        ("READ_CONTROL", AccessMask::READ_CONTROL),
        ("WRITE_DAC", AccessMask::WRITE_DAC),
        ("WRITE_OWNER", AccessMask::WRITE_OWNER),
        ("SYNCHRONIZE", AccessMask::SYNCHRONIZE),
        (
            "STANDARD_RIGHTS_REQUIRED",
            AccessMask::STANDARD_RIGHTS_REQUIRED,
        ),
        ("STANDARD_RIGHTS_ALL", AccessMask::STANDARD_RIGHTS_ALL),
        ("ACCESS_SYSTEM_SECURITY", AccessMask::ACCESS_SYSTEM_SECURITY),
        ("MAXIMUM_ALLOWED", AccessMask::MAXIMUM_ALLOWED),
        ("GENERIC_READ", AccessMask::GENERIC_READ),
        ("GENERIC_WRITE", AccessMask::GENERIC_WRITE),
        ("GENERIC_EXECUTE", AccessMask::GENERIC_EXECUTE),
        ("GENERIC_ALL", AccessMask::GENERIC_ALL),
    ];

    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }

    fn build<'v, 'a>(
        builder: TypeBuilder<'v, 'a, dolang::runtime::object::Flags<Self>>,
    ) -> TypeBuilder<'v, 'a, dolang::runtime::object::Flags<Self>> {
        builder
            .get("specific_rights", |this, strand, out| {
                Output::set(strand, out, this.flags().0.specific_rights());
                Ok(())
            })
            .get("standard_rights", |this, strand, out| {
                let ty = this.ty(strand.vm());
                ty.create_flags(strand, Self(this.flags().0.standard_rights()), out);
                Ok(())
            })
            .get("generic_rights", |this, strand, out| {
                let ty = this.ty(strand.vm());
                ty.create_flags(strand, Self(this.flags().0.generic_rights()), out);
                Ok(())
            })
            .get("int", |this, strand, out| {
                Output::set(strand, out, this.flags().0.bits());
                Ok(())
            })
            .type_method("from_int", async move |this, strand, args, out| {
                let ([value], []) = unpack!(strand, args, 1, 0)?;
                let value = ace_u32(strand, &value, "value")?;
                this.create_flags(strand, Self(WinAccessMask::from_bits_retain(value)), out);
                Ok(())
            })
    }
}

/// Security descriptor control flags (`security.windows.SecDescControl`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecDescControl(WinSecDescControl);

impl BitOr for SecDescControl {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitAnd for SecDescControl {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitXor for SecDescControl {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

impl Not for SecDescControl {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl FlagLike for SecDescControl {
    const ZERO: Self = Self(WinSecDescControl::empty());
    const MODULE: &'static str = "security.windows";
    const NAME: &'static str = "SecDescControl";
    const BITS: &'static [(&'static str, Self)] = &[
        ("OWNER_DEFAULTED", Self(WinSecDescControl::OWNER_DEFAULTED)),
        ("GROUP_DEFAULTED", Self(WinSecDescControl::GROUP_DEFAULTED)),
        ("DACL_PRESENT", Self(WinSecDescControl::DACL_PRESENT)),
        ("DACL_DEFAULTED", Self(WinSecDescControl::DACL_DEFAULTED)),
        ("SACL_PRESENT", Self(WinSecDescControl::SACL_PRESENT)),
        ("SACL_DEFAULTED", Self(WinSecDescControl::SACL_DEFAULTED)),
        (
            "DACL_AUTO_INHERIT_REQUIRED",
            Self(WinSecDescControl::DACL_AUTO_INHERIT_REQUIRED),
        ),
        (
            "SACL_AUTO_INHERIT_REQUIRED",
            Self(WinSecDescControl::SACL_AUTO_INHERIT_REQUIRED),
        ),
        (
            "DACL_AUTO_INHERITED",
            Self(WinSecDescControl::DACL_AUTO_INHERITED),
        ),
        (
            "SACL_AUTO_INHERITED",
            Self(WinSecDescControl::SACL_AUTO_INHERITED),
        ),
        ("DACL_PROTECTED", Self(WinSecDescControl::DACL_PROTECTED)),
        ("SACL_PROTECTED", Self(WinSecDescControl::SACL_PROTECTED)),
        (
            "RM_CONTROL_VALID",
            Self(WinSecDescControl::RM_CONTROL_VALID),
        ),
        ("SELF_RELATIVE", Self(WinSecDescControl::SELF_RELATIVE)),
    ];

    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

/// Unix read/write/execute permission bits (`security.unix.Permission`), a
/// local newtype over [`dolang_vfs::security::Permission`] so [`FlagLike`]
/// can be implemented here. Shared by [`Mode`]'s owner/group/other
/// projections and by [`PosixAceObject`]'s permission set.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Permission(pub VfsPermission);
flags_ops!(Permission);
impl FlagLike for Permission {
    const ZERO: Self = Self(VfsPermission::empty());
    const MODULE: &'static str = "security.unix";
    const NAME: &'static str = "Permission";
    const BITS: &'static [(&'static str, Self)] = &[
        ("READ", Self(VfsPermission::READ)),
        ("WRITE", Self(VfsPermission::WRITE)),
        ("EXECUTE", Self(VfsPermission::EXECUTE)),
    ];

    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

pub(crate) struct Identity;

impl<'v> Object<'v> for Identity {
    const NAME: &'v str = "Identity";
    const MODULE: &'v str = "security.unix";
    const SLOTS: usize = 1;
    type Annex = UnixSecurityInfo;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("uid", |this, strand, out| {
                Output::set(strand, out, this.annex().uid);
                Ok(())
            })
            .get("gid", |this, strand, out| {
                Output::set(strand, out, this.annex().gid);
                Ok(())
            })
            .get("euid", |this, strand, out| {
                Output::set(strand, out, this.annex().euid);
                Ok(())
            })
            .get("egid", |this, strand, out| {
                Output::set(strand, out, this.annex().egid);
                Ok(())
            })
            .get("group_ids", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                Output::set(strand, out, Ref::slot::<0>(&borrow));
                Ok(())
            })
    }
}

pub(crate) struct PosixAclObject;

struct PosixAclAces;

impl<'v> ArrayLike<'v> for PosixAclAces {
    type Object = PosixAclObject;

    const MODULE: &'v str = "security.unix";
    const NAME: &'v str = "AclAces";

    fn len(&self, this: Instance<'v, '_, PosixAclObject>, _strand: &mut Strand<'v, '_>) -> usize {
        this.annex().entries().len()
    }

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, PosixAclObject>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ace = this.annex().entries()[index];
        strand
            .state::<Global<'v>>()
            .types
            .posix_ace
            .create_with_annex(strand, PosixAceObject, ace, out);
        Ok(())
    }
}

impl<'v> Object<'v> for PosixAclObject {
    const NAME: &'v str = "Acl";
    const MODULE: &'v str = "security.unix";
    type Annex = VfsPosixAcl;
    type Type = ();
    type TypeAnnex = ();

    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ([mut iterable], []) = unpack!(strand, args, 1, 0)?;
        let global = strand.state::<Global<'v>>();
        iterable.iter(strand, &mut out).await?;
        let mut entries = Vec::new();
        while out.next(strand, &mut iterable).await? {
            let ace = global.types.posix_ace.cast(&iterable).ok_or_else(|| {
                Error::type_error(strand, "Acl: iterable must contain security.unix.Ace")
            })?;
            entries.push(ace.enter_sync(strand, |_strand, ace| *ace.annex()));
        }
        let acl =
            VfsPosixAcl::new(entries).map_err(|error| Error::value(strand, error.to_string()))?;
        this.create_with_annex(strand, PosixAclObject, acl, out);
        Ok(())
    }

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder.get("aces", |this, strand, out| {
            Output::set(strand, out, ArrayView::new(this, PosixAclAces));
            Ok(())
        })
    }
}

pub(crate) fn create_posix_acl<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    acl: Option<VfsPosixAcl>,
    out: &mut Slot<'v, '_>,
) {
    match acl {
        Some(acl) => {
            global
                .types
                .posix_acl
                .create_with_annex(strand, PosixAclObject, acl, out);
        }
        None => Output::set(strand, out, Nil),
    }
}

pub(crate) struct PosixAceObject;

async fn posix_permissions<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    permissions: Option<&Value<'v>>,
) -> Result<'v, 's, VfsPermission> {
    let Some(value) = permissions else {
        return Ok(VfsPermission::empty());
    };
    Ok(global.types.permission.coerce(strand, value).await?.0)
}

fn posix_id<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &'static str,
) -> Result<'v, 's, u32> {
    let value = value
        .to_i64(strand)
        .map_err(|_| Error::type_error(strand, format!("{name}: expected Int")))?;
    u32::try_from(value).map_err(|_| Error::value(strand, format!("{name}: out of range")))
}

impl<'v> Object<'v> for PosixAceObject {
    const NAME: &'v str = "Ace";
    const MODULE: &'v str = "security.unix";
    type Annex = VfsPosixAce;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let id_field = builder.sym("id");
        let user_obj = builder.sym("USER_OBJ");
        let user = builder.sym("USER");
        let group_obj = builder.sym("GROUP_OBJ");
        let group = builder.sym("GROUP");
        let mask = builder.sym("MASK");
        let other = builder.sym("OTHER");

        macro_rules! constructor {
            ($builder:expr, $name:literal, $qualifier:expr) => {
                $builder.type_method($name, async move |this, strand, args, out| {
                    let ([], [permissions]) = unpack!(strand, args, 0, 1)?;
                    let global = strand.state::<Global<'v>>();
                    let permissions =
                        posix_permissions(strand, global, permissions.as_deref()).await?;
                    this.create_with_annex(
                        strand,
                        PosixAceObject,
                        VfsPosixAce {
                            qualifier: $qualifier,
                            permissions,
                        },
                        out,
                    );
                    Ok(())
                })
            };
        }

        let builder = constructor!(builder, "user_obj", VfsPosixAclQualifier::UserObj);
        let builder = constructor!(builder, "group_obj", VfsPosixAclQualifier::GroupObj);
        let builder = constructor!(builder, "mask", VfsPosixAclQualifier::Mask);
        let builder = constructor!(builder, "other", VfsPosixAclQualifier::Other);

        builder
            .type_method("user", async move |this, strand, args, out| {
                let ([id], [permissions]) = unpack!(strand, args, 1, 1)?;
                let global = strand.state::<Global<'v>>();
                let permissions = posix_permissions(strand, global, permissions.as_deref()).await?;
                let id = posix_id(strand, &id, "uid")?;
                this.create_with_annex(
                    strand,
                    PosixAceObject,
                    VfsPosixAce {
                        qualifier: VfsPosixAclQualifier::User(id),
                        permissions,
                    },
                    out,
                );
                Ok(())
            })
            .type_method("group", async move |this, strand, args, out| {
                let ([id], [permissions]) = unpack!(strand, args, 1, 1)?;
                let global = strand.state::<Global<'v>>();
                let permissions = posix_permissions(strand, global, permissions.as_deref()).await?;
                let id = posix_id(strand, &id, "gid")?;
                this.create_with_annex(
                    strand,
                    PosixAceObject,
                    VfsPosixAce {
                        qualifier: VfsPosixAclQualifier::Group(id),
                        permissions,
                    },
                    out,
                );
                Ok(())
            })
            .get("type", move |this, strand, out| {
                let value = match this.annex().qualifier {
                    VfsPosixAclQualifier::UserObj => user_obj,
                    VfsPosixAclQualifier::User(_) => user,
                    VfsPosixAclQualifier::GroupObj => group_obj,
                    VfsPosixAclQualifier::Group(_) => group,
                    VfsPosixAclQualifier::Mask => mask,
                    VfsPosixAclQualifier::Other => other,
                };
                Output::set(strand, out, value);
                Ok(())
            })
            .get("id", move |this, strand, out| {
                let id = match this.annex().qualifier {
                    VfsPosixAclQualifier::User(id) | VfsPosixAclQualifier::Group(id) => id,
                    _ => return Err(Error::field(strand, id_field)),
                };
                Output::set(strand, out, id);
                Ok(())
            })
            .get("permissions", |this, strand, out| {
                let permission = strand.state::<Global<'v>>().types.permission;
                permission.create_flags(strand, Permission(this.annex().permissions), out);
                Ok(())
            })
    }
}

/// NFSv4 ACE permission mask (`security.nfs4.Mask`), a local newtype over
/// [`dolang_vfs::security::Nfs4AceMask`] so [`FlagLike`] can be implemented
/// here.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Nfs4AceMask(pub VfsNfs4AceMask);
flags_ops!(Nfs4AceMask);
impl FlagLike for Nfs4AceMask {
    const ZERO: Self = Self(VfsNfs4AceMask::empty());
    const MODULE: &'static str = "security.nfs4";
    const NAME: &'static str = "Mask";
    const BITS: &'static [(&'static str, Self)] = &[
        ("READ_DATA", Self(VfsNfs4AceMask::READ_DATA)),
        ("WRITE_DATA", Self(VfsNfs4AceMask::WRITE_DATA)),
        ("APPEND_DATA", Self(VfsNfs4AceMask::APPEND_DATA)),
        ("READ_NAMED_ATTRS", Self(VfsNfs4AceMask::READ_NAMED_ATTRS)),
        ("WRITE_NAMED_ATTRS", Self(VfsNfs4AceMask::WRITE_NAMED_ATTRS)),
        ("EXECUTE", Self(VfsNfs4AceMask::EXECUTE)),
        ("DELETE_CHILD", Self(VfsNfs4AceMask::DELETE_CHILD)),
        ("READ_ATTRIBUTES", Self(VfsNfs4AceMask::READ_ATTRIBUTES)),
        ("WRITE_ATTRIBUTES", Self(VfsNfs4AceMask::WRITE_ATTRIBUTES)),
        ("DELETE", Self(VfsNfs4AceMask::DELETE)),
        ("READ_ACL", Self(VfsNfs4AceMask::READ_ACL)),
        ("WRITE_ACL", Self(VfsNfs4AceMask::WRITE_ACL)),
        ("WRITE_OWNER", Self(VfsNfs4AceMask::WRITE_OWNER)),
        ("SYNCHRONIZE", Self(VfsNfs4AceMask::SYNCHRONIZE)),
    ];

    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

/// NFSv4 ACE inheritance/audit flags (`security.nfs4.Flags`), a local
/// newtype over [`dolang_vfs::security::Nfs4AceFlags`] so [`FlagLike`] can be
/// implemented here.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Nfs4AceFlags(pub VfsNfs4AceFlags);
flags_ops!(Nfs4AceFlags);
impl FlagLike for Nfs4AceFlags {
    const ZERO: Self = Self(VfsNfs4AceFlags::empty());
    const MODULE: &'static str = "security.nfs4";
    const NAME: &'static str = "Flags";
    const BITS: &'static [(&'static str, Self)] = &[
        ("FILE_INHERIT", Self(VfsNfs4AceFlags::FILE_INHERIT)),
        (
            "DIRECTORY_INHERIT",
            Self(VfsNfs4AceFlags::DIRECTORY_INHERIT),
        ),
        (
            "NO_PROPAGATE_INHERIT",
            Self(VfsNfs4AceFlags::NO_PROPAGATE_INHERIT),
        ),
        ("INHERIT_ONLY", Self(VfsNfs4AceFlags::INHERIT_ONLY)),
        (
            "SUCCESSFUL_ACCESS",
            Self(VfsNfs4AceFlags::SUCCESSFUL_ACCESS),
        ),
        ("FAILED_ACCESS", Self(VfsNfs4AceFlags::FAILED_ACCESS)),
        ("INHERITED", Self(VfsNfs4AceFlags::INHERITED)),
    ];

    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

pub(crate) struct Nfs4AclObject;

struct Nfs4AclAces;

impl<'v> ArrayLike<'v> for Nfs4AclAces {
    type Object = Nfs4AclObject;

    const MODULE: &'v str = "security.nfs4";
    const NAME: &'v str = "AclAces";

    fn len(&self, this: Instance<'v, '_, Nfs4AclObject>, _strand: &mut Strand<'v, '_>) -> usize {
        this.annex().entries().len()
    }

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, Nfs4AclObject>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ace = this.annex().entries()[index];
        strand
            .state::<Global<'v>>()
            .types
            .nfs4_ace
            .create_with_annex(strand, Nfs4AceObject, ace, out);
        Ok(())
    }
}

impl<'v> Object<'v> for Nfs4AclObject {
    const NAME: &'v str = "Acl";
    const MODULE: &'v str = "security.nfs4";
    type Annex = VfsNfs4Acl;
    type Type = ();
    type TypeAnnex = ();

    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ([mut iterable], []) = unpack!(strand, args, 1, 0)?;
        let global = strand.state::<Global<'v>>();
        iterable.iter(strand, &mut out).await?;
        let mut entries = Vec::new();
        while out.next(strand, &mut iterable).await? {
            let ace = global.types.nfs4_ace.cast(&iterable).ok_or_else(|| {
                Error::type_error(strand, "Acl: iterable must contain security.nfs4.Ace")
            })?;
            entries.push(ace.enter_sync(strand, |_strand, ace| *ace.annex()));
        }
        let acl = VfsNfs4Acl::new(entries);
        this.create_with_annex(strand, Nfs4AclObject, acl, out);
        Ok(())
    }

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder.get("aces", |this, strand, out| {
            Output::set(strand, out, ArrayView::new(this, Nfs4AclAces));
            Ok(())
        })
    }
}

pub(crate) fn create_nfs4_acl<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    acl: Option<VfsNfs4Acl>,
    out: &mut Slot<'v, '_>,
) {
    match acl {
        Some(acl) => {
            global
                .types
                .nfs4_acl
                .create_with_annex(strand, Nfs4AclObject, acl, out);
        }
        None => Output::set(strand, out, Nil),
    }
}

pub(crate) struct Nfs4AceObject;

async fn nfs4_ace_mask<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    mask: Option<&Value<'v>>,
) -> Result<'v, 's, VfsNfs4AceMask> {
    let Some(value) = mask else {
        return Err(Error::type_error(
            strand,
            "mask: expected security.nfs4.Mask",
        ));
    };
    Ok(global.types.nfs4_ace_mask.coerce(strand, value).await?.0)
}

async fn nfs4_ace_flags<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    flags: Option<&Value<'v>>,
) -> Result<'v, 's, VfsNfs4AceFlags> {
    let Some(value) = flags else {
        return Ok(VfsNfs4AceFlags::empty());
    };
    Ok(global.types.nfs4_ace_flags.coerce(strand, value).await?.0)
}

fn nfs4_ace_type<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: Option<&Value<'v>>,
    allow: dolang::runtime::Sym<'v, 'v>,
    deny: dolang::runtime::Sym<'v, 'v>,
    audit: dolang::runtime::Sym<'v, 'v>,
    alarm: dolang::runtime::Sym<'v, 'v>,
) -> Result<'v, 's, VfsNfs4AceType> {
    let Some(value) = value else {
        return Err(Error::type_error(
            strand,
            "type: expected :ALLOW:, :DENY:, :AUDIT:, or :ALARM:",
        ));
    };
    let Some(sym) = value.as_sym(strand) else {
        return Err(Error::type_error(
            strand,
            "type: expected :ALLOW:, :DENY:, :AUDIT:, or :ALARM:",
        ));
    };
    if sym == allow {
        Ok(VfsNfs4AceType::Allow)
    } else if sym == deny {
        Ok(VfsNfs4AceType::Deny)
    } else if sym == audit {
        Ok(VfsNfs4AceType::Audit)
    } else if sym == alarm {
        Ok(VfsNfs4AceType::Alarm)
    } else {
        Err(Error::value(
            strand,
            "type: expected :ALLOW:, :DENY:, :AUDIT:, or :ALARM:",
        ))
    }
}

impl<'v> Object<'v> for Nfs4AceObject {
    const NAME: &'v str = "Ace";
    const MODULE: &'v str = "security.nfs4";
    type Annex = VfsNfs4Ace;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let type_sym = builder.sym("type");
        let mask_sym = builder.sym("mask");
        let flags_sym = builder.sym("flags");
        let id_field = builder.sym("id");
        let allow = builder.sym("ALLOW");
        let deny = builder.sym("DENY");
        let audit = builder.sym("AUDIT");
        let alarm = builder.sym("ALARM");
        let owner = builder.sym("OWNER");
        let owning_group = builder.sym("OWNING_GROUP");
        let everyone = builder.sym("EVERYONE");
        let user = builder.sym("USER");
        let group = builder.sym("GROUP");

        macro_rules! constructor {
            ($builder:expr, $name:literal, $qualifier:expr) => {
                $builder.type_method($name, async move |this, strand, args, out| {
                    let ([], [ace_type, mask, flags]) = unpack!(
                        strand,
                        args,
                        0,
                        0,
                        type_sym = None,
                        mask_sym = None,
                        flags_sym = None
                    )?;
                    let global = strand.state::<Global<'v>>();
                    let ace_type =
                        nfs4_ace_type(strand, ace_type.as_deref(), allow, deny, audit, alarm)?;
                    let mask = nfs4_ace_mask(strand, global, mask.as_deref()).await?;
                    let flags = nfs4_ace_flags(strand, global, flags.as_deref()).await?;
                    this.create_with_annex(
                        strand,
                        Nfs4AceObject,
                        VfsNfs4Ace {
                            ace_type,
                            qualifier: $qualifier,
                            mask,
                            flags,
                        },
                        out,
                    );
                    Ok(())
                })
            };
        }

        let builder = constructor!(builder, "owner", VfsNfs4AceQualifier::Owner);
        let builder = constructor!(builder, "owning_group", VfsNfs4AceQualifier::OwningGroup);
        let builder = constructor!(builder, "everyone", VfsNfs4AceQualifier::Everyone);

        builder
            .type_method("user", async move |this, strand, args, out| {
                let ([id], [ace_type, mask, flags]) = unpack!(
                    strand,
                    args,
                    1,
                    0,
                    type_sym = None,
                    mask_sym = None,
                    flags_sym = None
                )?;
                let global = strand.state::<Global<'v>>();
                let ace_type =
                    nfs4_ace_type(strand, ace_type.as_deref(), allow, deny, audit, alarm)?;
                let mask = nfs4_ace_mask(strand, global, mask.as_deref()).await?;
                let flags = nfs4_ace_flags(strand, global, flags.as_deref()).await?;
                let id = posix_id(strand, &id, "uid")?;
                this.create_with_annex(
                    strand,
                    Nfs4AceObject,
                    VfsNfs4Ace {
                        ace_type,
                        qualifier: VfsNfs4AceQualifier::User(id),
                        mask,
                        flags,
                    },
                    out,
                );
                Ok(())
            })
            .type_method("group", async move |this, strand, args, out| {
                let ([id], [ace_type, mask, flags]) = unpack!(
                    strand,
                    args,
                    1,
                    0,
                    type_sym = None,
                    mask_sym = None,
                    flags_sym = None
                )?;
                let global = strand.state::<Global<'v>>();
                let ace_type =
                    nfs4_ace_type(strand, ace_type.as_deref(), allow, deny, audit, alarm)?;
                let mask = nfs4_ace_mask(strand, global, mask.as_deref()).await?;
                let flags = nfs4_ace_flags(strand, global, flags.as_deref()).await?;
                let id = posix_id(strand, &id, "gid")?;
                this.create_with_annex(
                    strand,
                    Nfs4AceObject,
                    VfsNfs4Ace {
                        ace_type,
                        qualifier: VfsNfs4AceQualifier::Group(id),
                        mask,
                        flags,
                    },
                    out,
                );
                Ok(())
            })
            .get("type", move |this, strand, out| {
                let value = match this.annex().ace_type {
                    VfsNfs4AceType::Allow => allow,
                    VfsNfs4AceType::Deny => deny,
                    VfsNfs4AceType::Audit => audit,
                    VfsNfs4AceType::Alarm => alarm,
                };
                Output::set(strand, out, value);
                Ok(())
            })
            .get("principal", move |this, strand, out| {
                let value = match this.annex().qualifier {
                    VfsNfs4AceQualifier::Owner => owner,
                    VfsNfs4AceQualifier::OwningGroup => owning_group,
                    VfsNfs4AceQualifier::Everyone => everyone,
                    VfsNfs4AceQualifier::User(_) => user,
                    VfsNfs4AceQualifier::Group(_) => group,
                };
                Output::set(strand, out, value);
                Ok(())
            })
            .get("id", move |this, strand, out| {
                let id = match this.annex().qualifier {
                    VfsNfs4AceQualifier::User(id) | VfsNfs4AceQualifier::Group(id) => id,
                    _ => return Err(Error::field(strand, id_field)),
                };
                Output::set(strand, out, id);
                Ok(())
            })
            .get("mask", |this, strand, out| {
                let mask = strand.state::<Global<'v>>().types.nfs4_ace_mask;
                mask.create_flags(strand, Nfs4AceMask(this.annex().mask), out);
                Ok(())
            })
            .get("flags", |this, strand, out| {
                let flags = strand.state::<Global<'v>>().types.nfs4_ace_flags;
                flags.create_flags(strand, Nfs4AceFlags(this.annex().flags), out);
                Ok(())
            })
    }
}

/// macOS extended ACE permission mask (`security.macos.Mask`), a local
/// newtype over [`dolang_vfs::security::MacosAceMask`] so [`FlagLike`] can be
/// implemented here.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacosAceMask(pub VfsMacosAceMask);
flags_ops!(MacosAceMask);
impl FlagLike for MacosAceMask {
    const ZERO: Self = Self(VfsMacosAceMask::empty());
    const MODULE: &'static str = "security.macos";
    const NAME: &'static str = "Mask";
    const BITS: &'static [(&'static str, Self)] = &[
        ("READ_DATA", Self(VfsMacosAceMask::READ_DATA)),
        ("WRITE_DATA", Self(VfsMacosAceMask::WRITE_DATA)),
        ("EXECUTE", Self(VfsMacosAceMask::EXECUTE)),
        ("DELETE", Self(VfsMacosAceMask::DELETE)),
        ("APPEND_DATA", Self(VfsMacosAceMask::APPEND_DATA)),
        ("DELETE_CHILD", Self(VfsMacosAceMask::DELETE_CHILD)),
        ("READ_ATTRIBUTES", Self(VfsMacosAceMask::READ_ATTRIBUTES)),
        ("WRITE_ATTRIBUTES", Self(VfsMacosAceMask::WRITE_ATTRIBUTES)),
        (
            "READ_EXTATTRIBUTES",
            Self(VfsMacosAceMask::READ_EXTATTRIBUTES),
        ),
        (
            "WRITE_EXTATTRIBUTES",
            Self(VfsMacosAceMask::WRITE_EXTATTRIBUTES),
        ),
        ("READ_SECURITY", Self(VfsMacosAceMask::READ_SECURITY)),
        ("WRITE_SECURITY", Self(VfsMacosAceMask::WRITE_SECURITY)),
        ("CHANGE_OWNER", Self(VfsMacosAceMask::CHANGE_OWNER)),
        ("SYNCHRONIZE", Self(VfsMacosAceMask::SYNCHRONIZE)),
    ];

    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

/// macOS extended ACE inheritance flags (`security.macos.Flags`), a local
/// newtype over [`dolang_vfs::security::MacosAceFlags`] so [`FlagLike`] can
/// be implemented here.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacosAceFlags(pub VfsMacosAceFlags);
flags_ops!(MacosAceFlags);
impl FlagLike for MacosAceFlags {
    const ZERO: Self = Self(VfsMacosAceFlags::empty());
    const MODULE: &'static str = "security.macos";
    const NAME: &'static str = "Flags";
    const BITS: &'static [(&'static str, Self)] = &[
        ("FILE_INHERIT", Self(VfsMacosAceFlags::FILE_INHERIT)),
        (
            "DIRECTORY_INHERIT",
            Self(VfsMacosAceFlags::DIRECTORY_INHERIT),
        ),
        ("LIMIT_INHERIT", Self(VfsMacosAceFlags::LIMIT_INHERIT)),
        ("ONLY_INHERIT", Self(VfsMacosAceFlags::ONLY_INHERIT)),
        ("INHERITED", Self(VfsMacosAceFlags::INHERITED)),
    ];

    fn rank(self) -> usize {
        self.0.bits().count_ones() as usize
    }
}

pub(crate) struct MacosAclObject;

struct MacosAclAces;

impl<'v> ArrayLike<'v> for MacosAclAces {
    type Object = MacosAclObject;

    const MODULE: &'v str = "security.macos";
    const NAME: &'v str = "AclAces";

    fn len(&self, this: Instance<'v, '_, MacosAclObject>, _strand: &mut Strand<'v, '_>) -> usize {
        this.annex().entries().len()
    }

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, MacosAclObject>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ace = this.annex().entries()[index];
        strand
            .state::<Global<'v>>()
            .types
            .macos_ace
            .create_with_annex(strand, MacosAceObject, ace, out);
        Ok(())
    }
}

impl<'v> Object<'v> for MacosAclObject {
    const NAME: &'v str = "Acl";
    const MODULE: &'v str = "security.macos";
    type Annex = VfsMacosAcl;
    type Type = ();
    type TypeAnnex = ();

    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ([mut iterable], []) = unpack!(strand, args, 1, 0)?;
        let global = strand.state::<Global<'v>>();
        iterable.iter(strand, &mut out).await?;
        let mut entries = Vec::new();
        while out.next(strand, &mut iterable).await? {
            let ace = global.types.macos_ace.cast(&iterable).ok_or_else(|| {
                Error::type_error(strand, "Acl: iterable must contain security.macos.Ace")
            })?;
            entries.push(ace.enter_sync(strand, |_strand, ace| *ace.annex()));
        }
        let acl = VfsMacosAcl::new(entries);
        this.create_with_annex(strand, MacosAclObject, acl, out);
        Ok(())
    }

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder.get("aces", |this, strand, out| {
            Output::set(strand, out, ArrayView::new(this, MacosAclAces));
            Ok(())
        })
    }
}

pub(crate) fn create_macos_acl<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    acl: Option<VfsMacosAcl>,
    out: &mut Slot<'v, '_>,
) {
    match acl {
        Some(acl) => {
            global
                .types
                .macos_acl
                .create_with_annex(strand, MacosAclObject, acl, out);
        }
        None => Output::set(strand, out, Nil),
    }
}

pub(crate) struct MacosAceObject;

async fn macos_ace_mask<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    mask: Option<&Value<'v>>,
) -> Result<'v, 's, VfsMacosAceMask> {
    let Some(value) = mask else {
        return Err(Error::type_error(
            strand,
            "mask: expected security.macos.Mask",
        ));
    };
    Ok(global.types.macos_ace_mask.coerce(strand, value).await?.0)
}

async fn macos_ace_flags<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    flags: Option<&Value<'v>>,
) -> Result<'v, 's, VfsMacosAceFlags> {
    let Some(value) = flags else {
        return Ok(VfsMacosAceFlags::empty());
    };
    Ok(global.types.macos_ace_flags.coerce(strand, value).await?.0)
}

impl<'v> Object<'v> for MacosAceObject {
    const NAME: &'v str = "Ace";
    const MODULE: &'v str = "security.macos";
    type Annex = VfsMacosAce;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let mask_sym = builder.sym("mask");
        let flags_sym = builder.sym("flags");
        let allow = builder.sym("ALLOW");
        let deny = builder.sym("DENY");

        macro_rules! constructor {
            ($builder:expr, $name:literal, $ace_type:expr) => {
                $builder.type_method($name, async move |this, strand, args, out| {
                    let ([principal], [mask, flags]) =
                        unpack!(strand, args, 1, 0, mask_sym = None, flags_sym = None)?;
                    let global = strand.state::<Global<'v>>();
                    let qualifier = dolang_ext_uuid::value_to_uuid(strand, &principal)?;
                    let mask = macos_ace_mask(strand, global, mask.as_deref()).await?;
                    let flags = macos_ace_flags(strand, global, flags.as_deref()).await?;
                    this.create_with_annex(
                        strand,
                        MacosAceObject,
                        VfsMacosAce {
                            ace_type: $ace_type,
                            qualifier,
                            mask,
                            flags,
                        },
                        out,
                    );
                    Ok(())
                })
            };
        }

        let builder = constructor!(builder, "allow", VfsMacosAceType::Allow);
        let builder = constructor!(builder, "deny", VfsMacosAceType::Deny);

        builder
            .get("type", move |this, strand, out| {
                let value = match this.annex().ace_type {
                    VfsMacosAceType::Allow => allow,
                    VfsMacosAceType::Deny => deny,
                };
                Output::set(strand, out, value);
                Ok(())
            })
            .get("principal", move |this, strand, out| {
                dolang_ext_uuid::create_uuid(strand, this.annex().qualifier, out);
                Ok(())
            })
            .get("mask", |this, strand, out| {
                let mask = strand.state::<Global<'v>>().types.macos_ace_mask;
                mask.create_flags(strand, MacosAceMask(this.annex().mask), out);
                Ok(())
            })
            .get("flags", |this, strand, out| {
                let flags = strand.state::<Global<'v>>().types.macos_ace_flags;
                flags.create_flags(strand, MacosAceFlags(this.annex().flags), out);
                Ok(())
            })
    }
}

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

pub(crate) struct Sid;

pub(crate) fn create_sid<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    sid: VfsSid,
    out: &mut Slot<'v, '_>,
) {
    global
        .types
        .sid
        .create_with_annex(strand, Sid, sid, &mut *out);
    global
        .types
        .sid
        .cast(&*out)
        .unwrap()
        .enter_sync(strand, |strand, this| {
            let annex = this.annex();
            let sub_authorities = annex.sub_authorities();
            Output::set(
                strand,
                Mut::slot_mut::<0>(&mut this.borrow_mut_unwrap()),
                AsTuple::new(sub_authorities.iter().copied()),
            );
        });
}

fn sid_from_value<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
) -> Result<'v, 's, VfsSid> {
    if let Some(value) = value.as_str(strand) {
        value
            .to_string()
            .parse::<VfsSid>()
            .map_err(|error| Error::value(strand, error.to_string()))
    } else if let Some(value) = value.as_bin(strand) {
        let bytes = value.to_vec();
        VfsSid::from_bytes(&bytes).map_err(|error| Error::value(strand, error.to_string()))
    } else {
        Err(Error::type_error(strand, "Sid: expected Str or Bin"))
    }
}

impl<'v> Object<'v> for Sid {
    const NAME: &'v str = "Sid";
    const MODULE: &'v str = "security.windows";
    const SLOTS: usize = 1;
    type Annex = VfsSid;
    type Type = ();
    type TypeAnnex = ();

    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ([value], []) = unpack!(strand, args, 1, 0)?;
        let sid = sid_from_value(strand, &value)?;
        this.create_with_annex(strand, Sid, sid, &mut out);
        this.cast(&out).unwrap().enter_sync(strand, |strand, this| {
            let annex = this.annex();
            Output::set(
                strand,
                Mut::slot_mut::<0>(&mut this.borrow_mut_unwrap()),
                AsTuple::new(annex.sub_authorities().iter().copied()),
            );
        });
        Ok(())
    }

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let null = builder.sym("NULL");
        let world = builder.sym("WORLD");
        let local = builder.sym("LOCAL");
        let creator = builder.sym("CREATOR");
        let non_unique = builder.sym("NON_UNIQUE");
        let nt = builder.sym("NT");
        let resource_manager = builder.sym("RESOURCE_MANAGER");
        let app_package = builder.sym("APP_PACKAGE");
        let mandatory_label = builder.sym("MANDATORY_LABEL");
        let scoped_policy = builder.sym("SCOPED_POLICY");
        let authentication = builder.sym("AUTHENTICATION");
        let process_trust = builder.sym("PROCESS_TRUST");

        builder
            .get("revision", |this, strand, out| {
                Output::set(strand, out, this.annex().revision() as u8);
                Ok(())
            })
            .get("sub_authority_count", |this, strand, out| {
                Output::set(strand, out, this.annex().sub_authorities().len());
                Ok(())
            })
            .get("identifier_authority", move |this, strand, out| {
                match this.annex().identifier_authority() {
                    SidIdentifierAuthority::Null => Output::set(strand, out, null),
                    SidIdentifierAuthority::World => Output::set(strand, out, world),
                    SidIdentifierAuthority::Local => Output::set(strand, out, local),
                    SidIdentifierAuthority::Creator => Output::set(strand, out, creator),
                    SidIdentifierAuthority::NonUnique => Output::set(strand, out, non_unique),
                    SidIdentifierAuthority::Nt => Output::set(strand, out, nt),
                    SidIdentifierAuthority::ResourceManager => {
                        Output::set(strand, out, resource_manager)
                    }
                    SidIdentifierAuthority::AppPackage => Output::set(strand, out, app_package),
                    SidIdentifierAuthority::MandatoryLabel => {
                        Output::set(strand, out, mandatory_label)
                    }
                    SidIdentifierAuthority::ScopedPolicy => Output::set(strand, out, scoped_policy),
                    SidIdentifierAuthority::Authentication => {
                        Output::set(strand, out, authentication)
                    }
                    SidIdentifierAuthority::ProcessTrust => Output::set(strand, out, process_trust),
                    SidIdentifierAuthority::Unknown(value) => Output::set(strand, out, value),
                    authority => Output::set(strand, out, u64::from(authority)),
                }
                Ok(())
            })
            .get("sub_authorities", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                Output::set(strand, out, Ref::slot::<0>(&borrow));
                Ok(())
            })
            .method("to_bin", async move |this, strand, args, out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let bytes = this.annex().to_bytes();
                Output::set(strand, out, bytes.as_slice());
                Ok(())
            })
            .method("lookup", async move |this, strand, args, mut out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let sid = this.annex().clone();
                let global = strand.state::<Global<'v>>();
                if global.local.get(strand).target().operating_system.family()
                    != OperatingSystemFamily::Windows
                {
                    return Err(Error::not_supported(strand));
                }
                let vfs = global.local.get(strand).vfs();
                let name = error::io_result(strand, vfs.sid_name(&sid).await)?;
                create_sid_name(strand, global, name, &mut out);
                Ok(())
            })
    }

    fn display<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn dolang::runtime::Format<'v>,
    ) -> Result<'v, 's, ()> {
        fmt!(strand, w, "{}", this.annex().as_ref())
    }

    fn debug<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn dolang::runtime::Format<'v>,
    ) -> Result<'v, 's, ()> {
        fmt!(
            strand,
            w,
            "<security.windows.Sid {}>",
            this.annex().as_ref()
        )
    }
}

pub(crate) enum AclComponent {
    Dacl,
    Sacl,
}

pub(crate) enum AclAnnex {
    Component(AclComponent),
    Owned(VfsAclBuf),
}

pub(crate) struct Acl;

fn create_acl<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    descriptor: Instance<'v, '_, SecDesc>,
    component: AclComponent,
    out: &mut Slot<'v, '_>,
) {
    global
        .types
        .acl
        .create_with_annex(strand, Acl, AclAnnex::Component(component), &mut *out);
    global
        .types
        .acl
        .cast(&*out)
        .unwrap()
        .enter_sync(strand, |strand, acl| {
            Output::set(
                strand,
                Mut::slot_mut::<0>(&mut acl.borrow_mut_unwrap()),
                descriptor,
            );
        });
}

fn with_acl<'v, 's, T>(
    this: Instance<'v, '_, Acl>,
    strand: &mut Strand<'v, 's>,
    f: impl FnOnce(&VfsAcl) -> T,
) -> Result<'v, 's, T> {
    if let AclAnnex::Owned(acl) = &*this.annex() {
        return Ok(f(acl));
    }
    let global = strand.state::<Global<'v>>();
    let borrow = this.borrow(strand)?;
    let descriptor = global
        .types
        .sec_desc
        .cast(Ref::slot::<0>(&borrow))
        .expect("Acl root is a SecDesc");
    let value = descriptor.enter_sync(strand, |_strand, descriptor| {
        let descriptor = descriptor.annex();
        let acl = match &*this.annex() {
            AclAnnex::Component(AclComponent::Dacl) => descriptor.dacl(),
            AclAnnex::Component(AclComponent::Sacl) => descriptor.sacl(),
            AclAnnex::Owned(_) => unreachable!(),
        }
        .expect("Acl component is non-null");
        f(acl)
    });
    Ok(value)
}

struct AclAces;

impl<'v> ArrayLike<'v> for AclAces {
    type Object = Acl;

    const MODULE: &'v str = "security.windows";
    const NAME: &'v str = "AclAces";

    fn len(&self, this: Instance<'v, '_, Acl>, strand: &mut Strand<'v, '_>) -> usize {
        with_acl(this, strand, |acl| usize::from(acl.ace_count())).unwrap()
    }

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, Acl>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let global = strand.state::<Global<'v>>();
        global
            .types
            .ace
            .create_with_annex(strand, Ace, AceAnnex::InAcl(index), &mut out);
        global
            .types
            .ace
            .cast(&out)
            .unwrap()
            .enter_sync(strand, |strand, ace| {
                Output::set(
                    strand,
                    Mut::slot_mut::<0>(&mut ace.borrow_mut_unwrap()),
                    this,
                );
            });
        Ok(())
    }
}

impl<'v> Object<'v> for Acl {
    const NAME: &'v str = "Acl";
    const MODULE: &'v str = "security.windows";
    const SLOTS: usize = 1;
    type Annex = AclAnnex;
    type Type = ();
    type TypeAnnex = ();

    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let revision_sym = strand.state::<Global<'v>>().syms.revision;
        let ([mut iterable], [revision]) = unpack!(strand, args, 1, 0, revision_sym = None)?;
        let revision = revision
            .map(|value| {
                if let Some(sym) = value.as_sym(strand.vm()) {
                    return match sym.as_str(strand.vm()) {
                        "BASIC" => Ok(AclRevision::Basic),
                        "DIRECTORY_SERVICE" => Ok(AclRevision::DirectoryService),
                        _ => Err(Error::value(
                            strand,
                            "revision: expected BASIC or DIRECTORY_SERVICE",
                        )),
                    };
                }
                let value = ace_u8(strand, &value, "revision")?;
                Ok(AclRevision::from(value))
            })
            .transpose()?;

        let global = strand.state::<Global<'v>>();
        iterable.iter(strand, &mut out).await?;
        let mut aces = Vec::new();
        while out.next(strand, &mut iterable).await? {
            let ace = global.types.ace.cast(&iterable).ok_or_else(|| {
                Error::type_error(strand, "Acl: iterable must contain security.windows.Ace")
            })?;
            let value = ace.enter_sync(strand, |strand, ace| {
                with_ace(ace, strand, VfsAce::to_owned)
            })?;
            aces.push(value);
        }
        let acl = VfsAclBuf::from_aces(&aces, revision)
            .map_err(|error| Error::value(strand, error.to_string()))?;
        this.create_with_annex(strand, Acl, AclAnnex::Owned(acl), out);
        Ok(())
    }

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let basic = builder.sym("BASIC");
        let directory_service = builder.sym("DIRECTORY_SERVICE");
        builder
            .get("revision", move |this, strand, out| {
                let revision = with_acl(this, strand, |acl| acl.revision())?;
                match revision {
                    AclRevision::Basic => Output::set(strand, out, basic),
                    AclRevision::DirectoryService => Output::set(strand, out, directory_service),
                    AclRevision::Unknown(value) => Output::set(strand, out, value),
                }
                Ok(())
            })
            .get("size", |this, strand, out| {
                let size = with_acl(this, strand, |acl| acl.size())?;
                Output::set(strand, out, size);
                Ok(())
            })
            .get("aces", |this, strand, out| {
                Output::set(strand, out, ArrayView::new(this, AclAces));
                Ok(())
            })
            .method("to_bin", async move |this, strand, args, out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let bytes = with_acl(this, strand, |acl| acl.as_bytes().to_vec())?;
                Output::set(strand, out, bytes.as_slice());
                Ok(())
            })
    }
}

pub(crate) enum AceAnnex {
    InAcl(usize),
    Owned(VfsAceBuf),
}

pub(crate) struct Ace;

fn ace_u32<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &'static str,
) -> Result<'v, 's, u32> {
    let value = value
        .to_i64(strand)
        .map_err(|_| Error::type_error(strand, format!("{name}: expected Int")))?;
    u32::try_from(value).map_err(|_| Error::value(strand, format!("{name}: out of range")))
}

fn ace_u8<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &'static str,
) -> Result<'v, 's, u8> {
    let value = ace_u32(strand, value, name)?;
    u8::try_from(value).map_err(|_| Error::value(strand, format!("{name}: out of range")))
}

fn ace_mask<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    value: &Value<'v>,
) -> Result<'v, 's, WinAccessMask> {
    if let Some(mask) = global.types.access_mask.cast_flags(value) {
        Ok(mask.0)
    } else {
        Ok(WinAccessMask::from_bits_retain(ace_u32(
            strand, value, "mask",
        )?))
    }
}

fn ace_bool<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &'static str,
) -> Result<'v, 's, bool> {
    value
        .as_bool(strand)
        .ok_or_else(|| Error::type_error(strand, format!("{name}: expected Bool")))
}

fn ace_options<'v, 's>(
    strand: &mut Strand<'v, 's>,
    flags: Option<&Value<'v>>,
    object_type: Option<&Value<'v>>,
    inherited_object_type: Option<&Value<'v>>,
    callback: Option<&Value<'v>>,
    application_data: Option<&Value<'v>>,
) -> Result<'v, 's, AceBuildOptions> {
    let mut options = AceBuildOptions::new();
    if let Some(value) = flags {
        options = options.flags(AceFlags::from_bits_retain(ace_u8(strand, value, "flags")?));
    }
    if let Some(value) = object_type {
        options = options.object_type(dolang_ext_uuid::value_to_guid(strand, value)?);
    }
    if let Some(value) = inherited_object_type {
        options = options.inherited_object_type(dolang_ext_uuid::value_to_guid(strand, value)?);
    }
    if callback
        .map(|value| ace_bool(strand, value, "callback"))
        .transpose()?
        .unwrap_or(false)
    {
        options = options.callback();
    }
    if let Some(value) = application_data {
        let value = value
            .as_bin(strand)
            .map(|value| value.to_vec())
            .ok_or_else(|| Error::type_error(strand, "application_data: expected Bin"))?;
        options = options.application_data(value);
    }
    Ok(options)
}

fn with_ace<'v, 's, T>(
    this: Instance<'v, '_, Ace>,
    strand: &mut Strand<'v, 's>,
    f: impl FnOnce(&VfsAce) -> T,
) -> Result<'v, 's, T> {
    if let AceAnnex::Owned(ace) = &*this.annex() {
        return Ok(f(ace));
    }
    let global = strand.state::<Global<'v>>();
    let borrow = this.borrow(strand)?;
    let acl = global
        .types
        .acl
        .cast(Ref::slot::<0>(&borrow))
        .expect("Ace root is an Acl");
    let index = match &*this.annex() {
        AceAnnex::InAcl(index) => *index,
        AceAnnex::Owned(_) => unreachable!(),
    };
    acl.enter_sync(strand, |strand, acl| {
        with_acl(acl, strand, |acl| {
            let ace = acl
                .aces()
                .nth(index)
                .expect("Ace array index was normalized");
            f(ace)
        })
    })
}

impl<'v> Object<'v> for Ace {
    const NAME: &'v str = "Ace";
    const MODULE: &'v str = "security.windows";
    const SLOTS: usize = 1;
    type Annex = AceAnnex;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let flags = builder.sym("flags");
        let object_type = builder.sym("object_type");
        let inherited_object_type = builder.sym("inherited_object_type");
        let callback = builder.sym("callback");
        let application_data = builder.sym("application_data");
        let successful = builder.sym("successful");
        let failed = builder.sym("failed");
        let mask_field = builder.sym("mask");
        let sid_field = builder.sym("sid");
        let object_flags_field = builder.sym("object_flags");
        let object_type_field = builder.sym("object_type");
        let inherited_object_type_field = builder.sym("inherited_object_type");
        let application_data_field = builder.sym("application_data");
        let successful_access_field = builder.sym("successful_access");
        let failed_access_field = builder.sym("failed_access");
        let trust_protected_filter_field = builder.sym("trust_protected_filter");

        let access_allowed = builder.sym("ACCESS_ALLOWED");
        let access_denied = builder.sym("ACCESS_DENIED");
        let system_audit = builder.sym("SYSTEM_AUDIT");
        let system_alarm = builder.sym("SYSTEM_ALARM");
        let access_allowed_compound = builder.sym("ACCESS_ALLOWED_COMPOUND");
        let access_allowed_object = builder.sym("ACCESS_ALLOWED_OBJECT");
        let access_denied_object = builder.sym("ACCESS_DENIED_OBJECT");
        let system_audit_object = builder.sym("SYSTEM_AUDIT_OBJECT");
        let system_alarm_object = builder.sym("SYSTEM_ALARM_OBJECT");
        let access_allowed_callback = builder.sym("ACCESS_ALLOWED_CALLBACK");
        let access_denied_callback = builder.sym("ACCESS_DENIED_CALLBACK");
        let access_allowed_callback_object = builder.sym("ACCESS_ALLOWED_CALLBACK_OBJECT");
        let access_denied_callback_object = builder.sym("ACCESS_DENIED_CALLBACK_OBJECT");
        let system_audit_callback = builder.sym("SYSTEM_AUDIT_CALLBACK");
        let system_alarm_callback = builder.sym("SYSTEM_ALARM_CALLBACK");
        let system_audit_callback_object = builder.sym("SYSTEM_AUDIT_CALLBACK_OBJECT");
        let system_alarm_callback_object = builder.sym("SYSTEM_ALARM_CALLBACK_OBJECT");
        let system_mandatory_label = builder.sym("SYSTEM_MANDATORY_LABEL");
        let system_resource_attribute = builder.sym("SYSTEM_RESOURCE_ATTRIBUTE");
        let system_scoped_policy_id = builder.sym("SYSTEM_SCOPED_POLICY_ID");
        let system_process_trust_label = builder.sym("SYSTEM_PROCESS_TRUST_LABEL");
        let system_access_filter = builder.sym("SYSTEM_ACCESS_FILTER");
        let unknown = builder.sym("UNKNOWN");

        builder
            .type_method("allow", async move |this, strand, args, out| {
                let (
                    [sid, mask],
                    [
                        flags_value,
                        object_type_value,
                        inherited_value,
                        callback_value,
                        application_value,
                    ],
                ) = unpack!(
                    strand,
                    args,
                    2,
                    0,
                    flags = None,
                    object_type = None,
                    inherited_object_type = None,
                    callback = None,
                    application_data = None
                )?;
                let global = strand.state::<Global<'v>>();
                let sid = global.types.sid.cast(&sid).ok_or_else(|| {
                    Error::type_error(strand, "sid: expected security.windows.Sid")
                })?;
                let mask = ace_mask(strand, global, &mask)?;
                let options = ace_options(
                    strand,
                    flags_value.as_deref(),
                    object_type_value.as_deref(),
                    inherited_value.as_deref(),
                    callback_value.as_deref(),
                    application_value.as_deref(),
                )?;
                let ace = sid.enter_sync(strand, |strand, sid| {
                    VfsAceBuf::allow(&sid.annex(), mask, options)
                        .map_err(|error| Error::value(strand, error.to_string()))
                })?;
                this.create_with_annex(strand, Ace, AceAnnex::Owned(ace), out);
                Ok(())
            })
            .type_method("deny", async move |this, strand, args, out| {
                let (
                    [sid, mask],
                    [
                        flags_value,
                        object_type_value,
                        inherited_value,
                        callback_value,
                        application_value,
                    ],
                ) = unpack!(
                    strand,
                    args,
                    2,
                    0,
                    flags = None,
                    object_type = None,
                    inherited_object_type = None,
                    callback = None,
                    application_data = None
                )?;
                let global = strand.state::<Global<'v>>();
                let sid = global.types.sid.cast(&sid).ok_or_else(|| {
                    Error::type_error(strand, "sid: expected security.windows.Sid")
                })?;
                let mask = ace_mask(strand, global, &mask)?;
                let options = ace_options(
                    strand,
                    flags_value.as_deref(),
                    object_type_value.as_deref(),
                    inherited_value.as_deref(),
                    callback_value.as_deref(),
                    application_value.as_deref(),
                )?;
                let ace = sid.enter_sync(strand, |strand, sid| {
                    VfsAceBuf::deny(&sid.annex(), mask, options)
                        .map_err(|error| Error::value(strand, error.to_string()))
                })?;
                this.create_with_annex(strand, Ace, AceAnnex::Owned(ace), out);
                Ok(())
            })
            .type_method("audit", async move |this, strand, args, out| {
                let (
                    [sid, mask, successful_value, failed_value],
                    [
                        flags_value,
                        object_type_value,
                        inherited_value,
                        callback_value,
                        application_value,
                    ],
                ) = unpack!(
                    strand,
                    args,
                    2,
                    0,
                    successful,
                    failed,
                    flags = None,
                    object_type = None,
                    inherited_object_type = None,
                    callback = None,
                    application_data = None
                )?;
                let global = strand.state::<Global<'v>>();
                let sid = global.types.sid.cast(&sid).ok_or_else(|| {
                    Error::type_error(strand, "sid: expected security.windows.Sid")
                })?;
                let mask = ace_mask(strand, global, &mask)?;
                let successful = ace_bool(strand, &successful_value, "successful")?;
                let failed = ace_bool(strand, &failed_value, "failed")?;
                let options = ace_options(
                    strand,
                    flags_value.as_deref(),
                    object_type_value.as_deref(),
                    inherited_value.as_deref(),
                    callback_value.as_deref(),
                    application_value.as_deref(),
                )?;
                let ace = sid.enter_sync(strand, |strand, sid| {
                    VfsAceBuf::audit(&sid.annex(), mask, successful, failed, options)
                        .map_err(|error| Error::value(strand, error.to_string()))
                })?;
                this.create_with_annex(strand, Ace, AceAnnex::Owned(ace), out);
                Ok(())
            })
            .get("type", move |this, strand, out| {
                let ace_type = with_ace(this, strand, |ace| ace.ace_type())?;
                let value = match ace_type {
                    VfsAceType::AccessAllowed => access_allowed,
                    VfsAceType::AccessDenied => access_denied,
                    VfsAceType::SystemAudit => system_audit,
                    VfsAceType::SystemAlarm => system_alarm,
                    VfsAceType::AccessAllowedCompound => access_allowed_compound,
                    VfsAceType::AccessAllowedObject => access_allowed_object,
                    VfsAceType::AccessDeniedObject => access_denied_object,
                    VfsAceType::SystemAuditObject => system_audit_object,
                    VfsAceType::SystemAlarmObject => system_alarm_object,
                    VfsAceType::AccessAllowedCallback => access_allowed_callback,
                    VfsAceType::AccessDeniedCallback => access_denied_callback,
                    VfsAceType::AccessAllowedCallbackObject => access_allowed_callback_object,
                    VfsAceType::AccessDeniedCallbackObject => access_denied_callback_object,
                    VfsAceType::SystemAuditCallback => system_audit_callback,
                    VfsAceType::SystemAlarmCallback => system_alarm_callback,
                    VfsAceType::SystemAuditCallbackObject => system_audit_callback_object,
                    VfsAceType::SystemAlarmCallbackObject => system_alarm_callback_object,
                    VfsAceType::SystemMandatoryLabel => system_mandatory_label,
                    VfsAceType::SystemResourceAttribute => system_resource_attribute,
                    VfsAceType::SystemScopedPolicyId => system_scoped_policy_id,
                    VfsAceType::SystemProcessTrustLabel => system_process_trust_label,
                    VfsAceType::SystemAccessFilter => system_access_filter,
                    VfsAceType::Unknown(_) => unknown,
                    _ => unknown,
                };
                Output::set(strand, out, value);
                Ok(())
            })
            .get("type_code", |this, strand, out| {
                let value = with_ace(this, strand, |ace| ace.type_code())?;
                Output::set(strand, out, value);
                Ok(())
            })
            .get("flags", |this, strand, out| {
                let value = with_ace(this, strand, |ace| ace.flags())?;
                Output::set(strand, out, value.bits());
                Ok(())
            })
            .get("size", |this, strand, out| {
                let value = with_ace(this, strand, |ace| ace.size())?;
                Output::set(strand, out, value);
                Ok(())
            })
            .get("mask", move |this, strand, out| {
                let Some(value) = with_ace(this, strand, |ace| ace.mask())? else {
                    return Err(Error::field(strand, mask_field));
                };
                let global = strand.state::<Global<'v>>();
                global
                    .types
                    .access_mask
                    .create_flags(strand, AccessMask(value), out);
                Ok(())
            })
            .get("sid", move |this, strand, mut out| {
                let Some(value) = with_ace(this, strand, |ace| ace.sid())? else {
                    return Err(Error::field(strand, sid_field));
                };
                let global = strand.state::<Global<'v>>();
                create_sid(strand, global, value, &mut out);
                Ok(())
            })
            .get("object_flags", move |this, strand, out| {
                let Some(value) = with_ace(this, strand, |ace| ace.object_flags())? else {
                    return Err(Error::field(strand, object_flags_field));
                };
                Output::set(strand, out, value.bits());
                Ok(())
            })
            .get("object_type", move |this, strand, out| {
                let (flags, value) =
                    with_ace(this, strand, |ace| (ace.object_flags(), ace.object_type()))?;
                if flags.is_none() {
                    return Err(Error::field(strand, object_type_field));
                }
                if let Some(value) = value {
                    dolang_ext_uuid::create_guid(strand, value, out);
                } else {
                    Output::set(strand, out, Nil);
                }
                Ok(())
            })
            .get("inherited_object_type", move |this, strand, out| {
                let (flags, value) = with_ace(this, strand, |ace| {
                    (ace.object_flags(), ace.inherited_object_type())
                })?;
                if flags.is_none() {
                    return Err(Error::field(strand, inherited_object_type_field));
                }
                if let Some(value) = value {
                    dolang_ext_uuid::create_guid(strand, value, out);
                } else {
                    Output::set(strand, out, Nil);
                }
                Ok(())
            })
            .get("application_data", move |this, strand, out| {
                let Some(value) = with_ace(this, strand, |ace| {
                    ace.application_data().map(<[u8]>::to_vec)
                })?
                else {
                    return Err(Error::field(strand, application_data_field));
                };
                Output::set(strand, out, value.as_slice());
                Ok(())
            })
            .get("object_inherit", |this, strand, out| {
                ace_flag(this, strand, out, 0x01)
            })
            .get("container_inherit", |this, strand, out| {
                ace_flag(this, strand, out, 0x02)
            })
            .get("no_propagate_inherit", |this, strand, out| {
                ace_flag(this, strand, out, 0x04)
            })
            .get("inherit_only", |this, strand, out| {
                ace_flag(this, strand, out, 0x08)
            })
            .get("inherited", |this, strand, out| {
                ace_flag(this, strand, out, 0x10)
            })
            .get("critical", |this, strand, out| {
                ace_flag(this, strand, out, 0x20)
            })
            .get("successful_access", move |this, strand, out| {
                let (kind, flags) = with_ace(this, strand, |ace| (ace.ace_type(), ace.flags()))?;
                if !ace_is_audit(kind) {
                    return Err(Error::field(strand, successful_access_field));
                }
                Output::set(strand, out, flags.contains(AceFlags::SUCCESSFUL_ACCESS));
                Ok(())
            })
            .get("failed_access", move |this, strand, out| {
                let (kind, flags) = with_ace(this, strand, |ace| (ace.ace_type(), ace.flags()))?;
                if !ace_is_audit(kind) {
                    return Err(Error::field(strand, failed_access_field));
                }
                Output::set(strand, out, flags.contains(AceFlags::FAILED_ACCESS));
                Ok(())
            })
            .get("trust_protected_filter", move |this, strand, out| {
                let (kind, flags) = with_ace(this, strand, |ace| (ace.ace_type(), ace.flags()))?;
                if kind != VfsAceType::SystemAccessFilter {
                    return Err(Error::field(strand, trust_protected_filter_field));
                }
                Output::set(
                    strand,
                    out,
                    flags.contains(AceFlags::TRUST_PROTECTED_FILTER),
                );
                Ok(())
            })
            .method("to_bin", async move |this, strand, args, out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let bytes = with_ace(this, strand, |ace| ace.as_bytes().to_vec())?;
                Output::set(strand, out, bytes.as_slice());
                Ok(())
            })
    }
}

fn ace_flag<'v, 's>(
    this: Instance<'v, '_, Ace>,
    strand: &mut Strand<'v, 's>,
    out: impl Output<'v>,
    flag: u8,
) -> Result<'v, 's, ()> {
    let flags = with_ace(this, strand, |ace| ace.flags())?;
    Output::set(
        strand,
        out,
        flags.contains(AceFlags::from_bits_retain(flag)),
    );
    Ok(())
}

const fn ace_is_audit(kind: VfsAceType) -> bool {
    matches!(
        kind,
        VfsAceType::SystemAudit
            | VfsAceType::SystemAlarm
            | VfsAceType::SystemAuditObject
            | VfsAceType::SystemAlarmObject
            | VfsAceType::SystemAuditCallback
            | VfsAceType::SystemAlarmCallback
            | VfsAceType::SystemAuditCallbackObject
            | VfsAceType::SystemAlarmCallbackObject
    )
}

pub(crate) struct SecDesc;

fn update_sid<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    value: Option<&Value<'v>>,
    name: &'static str,
) -> Result<'v, 's, Option<Option<VfsSid>>> {
    value
        .map(|value| {
            if value.is_nil() {
                Ok(None)
            } else {
                global
                    .types
                    .sid
                    .cast(value)
                    .map(|value| {
                        value.enter_sync(strand, |_strand, value| Some((*value.annex()).clone()))
                    })
                    .ok_or_else(|| {
                        Error::type_error(
                            strand,
                            format!("{name}: expected security.windows.Sid or nil"),
                        )
                    })
            }
        })
        .transpose()
}

fn update_acl<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    value: Option<&Value<'v>>,
    name: &'static str,
) -> Result<'v, 's, Option<Option<VfsAclBuf>>> {
    value
        .map(|value| {
            if value.is_nil() {
                Ok(None)
            } else {
                let value = global.types.acl.cast(value).ok_or_else(|| {
                    Error::type_error(
                        strand,
                        format!("{name}: expected security.windows.Acl or nil"),
                    )
                })?;
                value.enter_sync(strand, |strand, value| {
                    with_acl(value, strand, |acl| Some(acl.to_owned()))
                })
            }
        })
        .transpose()
}

fn update_bool<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: Option<&Value<'v>>,
    name: &'static str,
) -> Result<'v, 's, Option<bool>> {
    value.map(|value| ace_bool(strand, value, name)).transpose()
}

fn sec_desc_update<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    [
        owner,
        group,
        dacl,
        sacl,
        owner_defaulted,
        group_defaulted,
        dacl_present,
        dacl_defaulted,
        dacl_auto_inherit_required,
        dacl_auto_inherited,
        dacl_protected,
        sacl_present,
        sacl_defaulted,
        sacl_auto_inherit_required,
        sacl_auto_inherited,
        sacl_protected,
        rm_control,
    ]: [Option<&Value<'v>>; 17],
) -> Result<'v, 's, VfsSecDescUpdate> {
    let rm_control = rm_control
        .map(|value| {
            if value.is_nil() {
                Ok(None)
            } else {
                ace_u8(strand, value, "rm_control").map(Some)
            }
        })
        .transpose()?;
    let mut update = VfsSecDescUpdate::new();
    if let Some(owner) = update_sid(strand, global, owner, "owner")? {
        update = update.owner(owner);
    }
    if let Some(group) = update_sid(strand, global, group, "group")? {
        update = update.group(group);
    }
    if let Some(dacl) = update_acl(strand, global, dacl, "dacl")? {
        update = update.dacl(dacl);
    }
    if let Some(sacl) = update_acl(strand, global, sacl, "sacl")? {
        update = update.sacl(sacl);
    }

    macro_rules! update_flag {
        ($field:ident) => {
            if let Some(value) = update_bool(strand, $field, stringify!($field))? {
                update = update.$field(value);
            }
        };
    }
    update_flag!(owner_defaulted);
    update_flag!(group_defaulted);
    update_flag!(dacl_present);
    update_flag!(dacl_defaulted);
    update_flag!(dacl_auto_inherit_required);
    update_flag!(dacl_auto_inherited);
    update_flag!(dacl_protected);
    update_flag!(sacl_present);
    update_flag!(sacl_defaulted);
    update_flag!(sacl_auto_inherit_required);
    update_flag!(sacl_auto_inherited);
    update_flag!(sacl_protected);
    if let Some(rm_control) = rm_control {
        update = update.rm_control(rm_control);
    }
    Ok(update)
}

pub(crate) fn create_sec_desc<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    sec_desc: VfsSecDesc,
    out: &mut Slot<'v, '_>,
) {
    global
        .types
        .sec_desc
        .create_with_annex(strand, SecDesc, sec_desc, out);
}

pub(crate) fn sec_desc_from_value<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    value: &Value<'v>,
) -> Result<'v, 's, VfsSecDesc> {
    global
        .types
        .sec_desc
        .cast(value)
        .map(|value| value.enter_sync(strand, |_strand, value| value.annex().clone()))
        .ok_or_else(|| Error::type_error(strand, "expected security.windows.SecDesc"))
}

impl<'v> Object<'v> for SecDesc {
    const NAME: &'v str = "SecDesc";
    const MODULE: &'v str = "security.windows";
    type Annex = VfsSecDesc;
    type Type = ();
    type TypeAnnex = ();

    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let global = strand.state::<Global<'v>>();
        let owner = global.syms.owner;
        let group = global.syms.group;
        let dacl = global.syms.dacl;
        let sacl = global.syms.sacl;
        let owner_defaulted = global.syms.owner_defaulted;
        let group_defaulted = global.syms.group_defaulted;
        let dacl_present = global.syms.dacl_present;
        let dacl_defaulted = global.syms.dacl_defaulted;
        let dacl_auto_inherit_required = global.syms.dacl_auto_inherit_required;
        let dacl_auto_inherited = global.syms.dacl_auto_inherited;
        let dacl_protected = global.syms.dacl_protected;
        let sacl_present = global.syms.sacl_present;
        let sacl_defaulted = global.syms.sacl_defaulted;
        let sacl_auto_inherit_required = global.syms.sacl_auto_inherit_required;
        let sacl_auto_inherited = global.syms.sacl_auto_inherited;
        let sacl_protected = global.syms.sacl_protected;
        let rm_control = global.syms.rm_control;
        let (
            [],
            [
                value,
                owner_value,
                group_value,
                dacl_value,
                sacl_value,
                owner_defaulted_value,
                group_defaulted_value,
                dacl_present_value,
                dacl_defaulted_value,
                dacl_auto_inherit_required_value,
                dacl_auto_inherited_value,
                dacl_protected_value,
                sacl_present_value,
                sacl_defaulted_value,
                sacl_auto_inherit_required_value,
                sacl_auto_inherited_value,
                sacl_protected_value,
                rm_control_value,
            ],
        ) = unpack!(
            strand,
            args,
            0,
            1,
            owner = None,
            group = None,
            dacl = None,
            sacl = None,
            owner_defaulted = None,
            group_defaulted = None,
            dacl_present = None,
            dacl_defaulted = None,
            dacl_auto_inherit_required = None,
            dacl_auto_inherited = None,
            dacl_protected = None,
            sacl_present = None,
            sacl_defaulted = None,
            sacl_auto_inherit_required = None,
            sacl_auto_inherited = None,
            sacl_protected = None,
            rm_control = None
        )?;
        let values = [
            owner_value.as_deref(),
            group_value.as_deref(),
            dacl_value.as_deref(),
            sacl_value.as_deref(),
            owner_defaulted_value.as_deref(),
            group_defaulted_value.as_deref(),
            dacl_present_value.as_deref(),
            dacl_defaulted_value.as_deref(),
            dacl_auto_inherit_required_value.as_deref(),
            dacl_auto_inherited_value.as_deref(),
            dacl_protected_value.as_deref(),
            sacl_present_value.as_deref(),
            sacl_defaulted_value.as_deref(),
            sacl_auto_inherit_required_value.as_deref(),
            sacl_auto_inherited_value.as_deref(),
            sacl_protected_value.as_deref(),
            rm_control_value.as_deref(),
        ];
        let descriptor = if let Some(value) = value {
            if values.iter().any(|value| value.is_some()) {
                return Err(Error::value(
                    strand,
                    "SecDesc: packet form does not accept component options",
                ));
            }
            let value = value
                .as_bin(strand)
                .ok_or_else(|| Error::type_error(strand, "SecDesc: expected Bin"))?;
            VfsSecDesc::from_bytes(&value.to_vec())
                .map_err(|error| Error::value(strand, error.to_string()))?
        } else {
            let descriptor = VfsSecDesc::new(
                dolang_winterop::security::SecInfo::empty(),
                0,
                dolang_winterop::security::SecDescControl::empty(),
                None,
                None,
                None,
                None,
            )
            .expect("empty security descriptor is valid");
            descriptor
                .with(sec_desc_update(strand, global, values)?)
                .map_err(|error| Error::value(strand, error.to_string()))?
        };
        this.create_with_annex(strand, SecDesc, descriptor, out);
        Ok(())
    }

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        fn control_field<'v, 's>(
            _this: Instance<'v, '_, SecDesc>,
            strand: &mut Strand<'v, 's>,
            out: impl Output<'v>,
            field: dolang::runtime::Sym<'v, '_>,
            loaded: bool,
            value: bool,
        ) -> Result<'v, 's, ()> {
            if !loaded {
                return Err(Error::field(strand, field));
            }
            Output::set(strand, out, value);
            Ok(())
        }

        let rm_control = builder.sym("rm_control");
        let owner = builder.sym("owner");
        let group = builder.sym("group");
        let dacl = builder.sym("dacl");
        let sacl = builder.sym("sacl");
        let owner_defaulted = builder.sym("owner_defaulted");
        let group_defaulted = builder.sym("group_defaulted");
        let dacl_present = builder.sym("dacl_present");
        let dacl_defaulted = builder.sym("dacl_defaulted");
        let dacl_auto_inherit_required = builder.sym("dacl_auto_inherit_required");
        let dacl_auto_inherited = builder.sym("dacl_auto_inherited");
        let dacl_protected = builder.sym("dacl_protected");
        let sacl_present = builder.sym("sacl_present");
        let sacl_defaulted = builder.sym("sacl_defaulted");
        let sacl_auto_inherit_required = builder.sym("sacl_auto_inherit_required");
        let sacl_auto_inherited = builder.sym("sacl_auto_inherited");
        let sacl_protected = builder.sym("sacl_protected");

        builder
            .get("revision", |this, strand, out| {
                Output::set(strand, out, this.annex().revision() as u8);
                Ok(())
            })
            .get("control", |this, strand, out| {
                let global = strand.state::<Global<'v>>();
                global.types.sec_desc_control.create_flags(
                    strand,
                    SecDescControl(this.annex().control()),
                    out,
                );
                Ok(())
            })
            .get("mask", |this, strand, out| {
                let global = strand.state::<Global<'v>>();
                global
                    .types
                    .sec_info
                    .create_flags(strand, SecInfo(this.annex().mask()), out);
                Ok(())
            })
            .get("rm_control_valid", |this, strand, out| {
                Output::set(strand, out, this.annex().rm_control_valid());
                Ok(())
            })
            .get("rm_control", move |this, strand, out| {
                util::option_field(strand, this.annex().rm_control(), rm_control, out)
            })
            .get("owner", move |this, strand, mut out| {
                let descriptor = this.annex();
                let Some(value) = descriptor.owner().filter(|_| descriptor.owner_loaded()) else {
                    return Err(Error::field(strand, owner));
                };
                let global = strand.state::<Global<'v>>();
                create_sid(strand, global, value.clone(), &mut out);
                Ok(())
            })
            .get("group", move |this, strand, mut out| {
                let descriptor = this.annex();
                let Some(value) = descriptor.group().filter(|_| descriptor.group_loaded()) else {
                    return Err(Error::field(strand, group));
                };
                let global = strand.state::<Global<'v>>();
                create_sid(strand, global, value.clone(), &mut out);
                Ok(())
            })
            .get("dacl", move |this, strand, mut out| {
                let descriptor = this.annex();
                if !descriptor.dacl_loaded() || !descriptor.dacl_present() {
                    return Err(Error::field(strand, dacl));
                }
                if descriptor.dacl().is_none() {
                    Output::set(strand, out, Nil);
                } else {
                    let global = strand.state::<Global<'v>>();
                    create_acl(strand, global, this, AclComponent::Dacl, &mut out);
                }
                Ok(())
            })
            .get("sacl", move |this, strand, mut out| {
                let descriptor = this.annex();
                if !descriptor.sacl_loaded() || !descriptor.sacl_present() {
                    return Err(Error::field(strand, sacl));
                }
                if descriptor.sacl().is_none() {
                    Output::set(strand, out, Nil);
                } else {
                    let global = strand.state::<Global<'v>>();
                    create_acl(strand, global, this, AclComponent::Sacl, &mut out);
                }
                Ok(())
            })
            .get("owner_defaulted", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    owner_defaulted,
                    this.annex().owner_loaded(),
                    this.annex().owner_defaulted(),
                )
            })
            .get("group_defaulted", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    group_defaulted,
                    this.annex().group_loaded(),
                    this.annex().group_defaulted(),
                )
            })
            .get("dacl_present", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    dacl_present,
                    this.annex().dacl_loaded(),
                    this.annex().dacl_present(),
                )
            })
            .get("dacl_defaulted", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    dacl_defaulted,
                    this.annex().dacl_loaded(),
                    this.annex().dacl_defaulted(),
                )
            })
            .get("dacl_auto_inherit_required", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    dacl_auto_inherit_required,
                    this.annex().dacl_loaded(),
                    this.annex().dacl_auto_inherit_required(),
                )
            })
            .get("dacl_auto_inherited", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    dacl_auto_inherited,
                    this.annex().dacl_loaded(),
                    this.annex().dacl_auto_inherited(),
                )
            })
            .get("dacl_protected", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    dacl_protected,
                    this.annex().dacl_loaded(),
                    this.annex().dacl_protected(),
                )
            })
            .get("sacl_present", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    sacl_present,
                    this.annex().sacl_loaded(),
                    this.annex().sacl_present(),
                )
            })
            .get("sacl_defaulted", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    sacl_defaulted,
                    this.annex().sacl_loaded(),
                    this.annex().sacl_defaulted(),
                )
            })
            .get("sacl_auto_inherit_required", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    sacl_auto_inherit_required,
                    this.annex().sacl_loaded(),
                    this.annex().sacl_auto_inherit_required(),
                )
            })
            .get("sacl_auto_inherited", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    sacl_auto_inherited,
                    this.annex().sacl_loaded(),
                    this.annex().sacl_auto_inherited(),
                )
            })
            .get("sacl_protected", move |this, strand, out| {
                control_field(
                    this,
                    strand,
                    out,
                    sacl_protected,
                    this.annex().sacl_loaded(),
                    this.annex().sacl_protected(),
                )
            })
            .method("with", async move |this, strand, args, out| {
                let (
                    [],
                    [
                        owner_value,
                        group_value,
                        dacl_value,
                        sacl_value,
                        owner_defaulted_value,
                        group_defaulted_value,
                        dacl_present_value,
                        dacl_defaulted_value,
                        dacl_auto_inherit_required_value,
                        dacl_auto_inherited_value,
                        dacl_protected_value,
                        sacl_present_value,
                        sacl_defaulted_value,
                        sacl_auto_inherit_required_value,
                        sacl_auto_inherited_value,
                        sacl_protected_value,
                        rm_control_value,
                    ],
                ) = unpack!(
                    strand,
                    args,
                    0,
                    0,
                    owner = None,
                    group = None,
                    dacl = None,
                    sacl = None,
                    owner_defaulted = None,
                    group_defaulted = None,
                    dacl_present = None,
                    dacl_defaulted = None,
                    dacl_auto_inherit_required = None,
                    dacl_auto_inherited = None,
                    dacl_protected = None,
                    sacl_present = None,
                    sacl_defaulted = None,
                    sacl_auto_inherit_required = None,
                    sacl_auto_inherited = None,
                    sacl_protected = None,
                    rm_control = None
                )?;
                let global = strand.state::<Global<'v>>();
                let update = sec_desc_update(
                    strand,
                    global,
                    [
                        owner_value.as_deref(),
                        group_value.as_deref(),
                        dacl_value.as_deref(),
                        sacl_value.as_deref(),
                        owner_defaulted_value.as_deref(),
                        group_defaulted_value.as_deref(),
                        dacl_present_value.as_deref(),
                        dacl_defaulted_value.as_deref(),
                        dacl_auto_inherit_required_value.as_deref(),
                        dacl_auto_inherited_value.as_deref(),
                        dacl_protected_value.as_deref(),
                        sacl_present_value.as_deref(),
                        sacl_defaulted_value.as_deref(),
                        sacl_auto_inherit_required_value.as_deref(),
                        sacl_auto_inherited_value.as_deref(),
                        sacl_protected_value.as_deref(),
                        rm_control_value.as_deref(),
                    ],
                )?;
                let descriptor = this
                    .annex()
                    .with(update)
                    .map_err(|error| Error::value(strand, error.to_string()))?;
                global
                    .types
                    .sec_desc
                    .create_with_annex(strand, SecDesc, descriptor, out);
                Ok(())
            })
            .method("to_bin", async move |this, strand, args, out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let bytes = this.annex().to_bytes();
                Output::set(strand, out, bytes.as_slice());
                Ok(())
            })
    }
}

pub(crate) struct SidName;

fn create_sid_name<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    name: VfsSidName,
    out: &mut Slot<'v, '_>,
) {
    global
        .types
        .sid_name
        .create_with_annex(strand, SidName, name, out);
}

impl<'v> Object<'v> for SidName {
    const NAME: &'v str = "SidName";
    const MODULE: &'v str = "security.windows";
    type Annex = VfsSidName;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let user = builder.sym("USER");
        let group = builder.sym("GROUP");
        let domain = builder.sym("DOMAIN");
        let alias = builder.sym("ALIAS");
        let well_known_group = builder.sym("WELL_KNOWN_GROUP");
        let deleted_account = builder.sym("DELETED_ACCOUNT");
        let invalid = builder.sym("INVALID");
        let unknown = builder.sym("UNKNOWN");
        let computer = builder.sym("COMPUTER");
        let label = builder.sym("LABEL");
        let logon_session = builder.sym("LOGON_SESSION");
        builder
            .get("sid", |this, strand, mut out| {
                let global = strand.state::<Global<'v>>();
                create_sid(strand, global, this.annex().sid.clone(), &mut out);
                Ok(())
            })
            .get("name", |this, strand, out| {
                Output::set(strand, out, this.annex().name.as_str());
                Ok(())
            })
            .get("domain", |this, strand, out| {
                Output::set(strand, out, this.annex().domain.as_str());
                Ok(())
            })
            .get("qualified_name", |this, strand, out| {
                if this.annex().domain.is_empty() {
                    Output::set(strand, out, this.annex().name.as_str());
                } else {
                    let name = format!("{}\\{}", this.annex().domain, this.annex().name);
                    Output::set(strand, out, name.as_str());
                }
                Ok(())
            })
            .get("kind", move |this, strand, out| {
                let kind = match this.annex().kind {
                    SidNameUse::User => user,
                    SidNameUse::Group => group,
                    SidNameUse::Domain => domain,
                    SidNameUse::Alias => alias,
                    SidNameUse::WellKnownGroup => well_known_group,
                    SidNameUse::DeletedAccount => deleted_account,
                    SidNameUse::Invalid => invalid,
                    SidNameUse::Unknown => unknown,
                    SidNameUse::Computer => computer,
                    SidNameUse::Label => label,
                    SidNameUse::LogonSession => logon_session,
                };
                Output::set(strand, out, kind);
                Ok(())
            })
            .type_method("lookup", async move |_this, strand, args, mut out| {
                let ([value], []) = unpack!(strand, args, 1, 0)?;
                let global = strand.state::<Global<'v>>();
                if global.local.get(strand).target().operating_system.family()
                    != OperatingSystemFamily::Windows
                {
                    return Err(Error::not_supported(strand));
                }
                let vfs = global.local.get(strand).vfs();
                let name = if let Some(sid) = global.types.sid.cast(&value) {
                    let sid = sid.enter_sync(strand, |_strand, sid| sid.annex().clone());
                    error::io_result(strand, vfs.sid_name(&sid).await)?
                } else if let Some(value) = value.as_str(strand) {
                    let value = value.to_string();
                    error::io_result(strand, vfs.account_name(&value).await)?
                } else {
                    return Err(Error::type_error(
                        strand,
                        "SidName.lookup: expected Sid or Str",
                    ));
                };
                create_sid_name(strand, global, name, &mut out);
                Ok(())
            })
    }
}

pub(crate) struct TokenGroup;

impl<'v> Object<'v> for TokenGroup {
    const NAME: &'v str = "TokenGroup";
    const MODULE: &'v str = "security.windows";
    const SLOTS: usize = 1;
    type Annex = VfsTokenGroup;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        fn flag<'v, 's>(
            this: Instance<'v, '_, TokenGroup>,
            strand: &mut Strand<'v, 's>,
            out: impl Output<'v>,
            mask: WinTokenGroupAttributes,
        ) -> Result<'v, 's, ()> {
            Output::set(strand, out, this.annex().attributes.contains(mask));
            Ok(())
        }

        builder
            .get("sid", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                Output::set(strand, out, Ref::slot::<0>(&borrow));
                Ok(())
            })
            .get("attributes", |this, strand, out| {
                let global = strand.state::<Global<'v>>();
                global.types.token_group_attributes.create_flags(
                    strand,
                    TokenGroupAttributes(this.annex().attributes),
                    out,
                );
                Ok(())
            })
            .get("mandatory", |this, strand, out| {
                flag(this, strand, out, WinTokenGroupAttributes::MANDATORY)
            })
            .get("enabled_by_default", |this, strand, out| {
                flag(
                    this,
                    strand,
                    out,
                    WinTokenGroupAttributes::ENABLED_BY_DEFAULT,
                )
            })
            .get("enabled", |this, strand, out| {
                flag(this, strand, out, WinTokenGroupAttributes::ENABLED)
            })
            .get("owner", |this, strand, out| {
                flag(this, strand, out, WinTokenGroupAttributes::OWNER)
            })
            .get("use_for_deny_only", |this, strand, out| {
                flag(
                    this,
                    strand,
                    out,
                    WinTokenGroupAttributes::USE_FOR_DENY_ONLY,
                )
            })
            .get("integrity", |this, strand, out| {
                flag(this, strand, out, WinTokenGroupAttributes::INTEGRITY)
            })
            .get("integrity_enabled", |this, strand, out| {
                flag(
                    this,
                    strand,
                    out,
                    WinTokenGroupAttributes::INTEGRITY_ENABLED,
                )
            })
            .get("resource", |this, strand, out| {
                flag(this, strand, out, WinTokenGroupAttributes::RESOURCE)
            })
            .get("logon_id", |this, strand, out| {
                Output::set(
                    strand,
                    out,
                    this.annex()
                        .attributes
                        .contains(WinTokenGroupAttributes::LOGON_ID),
                );
                Ok(())
            })
    }
}

pub(crate) struct TokenInfo;

struct TokenGroups;

impl<'v> ArrayLike<'v> for TokenGroups {
    type Object = TokenInfo;

    const MODULE: &'v str = "security.windows";
    const NAME: &'v str = "TokenGroups";

    fn len(&self, this: Instance<'v, '_, Self::Object>, _strand: &mut Strand<'v, '_>) -> usize {
        this.annex().groups.len()
    }

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, Self::Object>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let token_group = this
            .annex()
            .groups
            .get(index)
            .expect("array view index was normalized")
            .clone();
        let global = strand.state::<Global<'v>>();
        strand.with_slots_sync(|strand, [mut sid]| {
            create_sid(strand, global, token_group.sid.clone(), &mut sid);
            global
                .types
                .token_group
                .create_with_annex(strand, TokenGroup, token_group, &mut out);
            global
                .types
                .token_group
                .cast(&out)
                .unwrap()
                .enter_sync(strand, |strand, group| {
                    Output::set(
                        strand,
                        Mut::slot_mut::<0>(&mut group.borrow_mut_unwrap()),
                        &sid,
                    );
                });
            Ok(())
        })
    }
}

impl<'v> Object<'v> for TokenInfo {
    const NAME: &'v str = "TokenInfo";
    const MODULE: &'v str = "security.windows";
    const SLOTS: usize = 4;
    type Annex = WindowsTokenInfo;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("is_elevated", |this, strand, out| {
                Output::set(strand, out, this.annex().is_elevated);
                Ok(())
            })
            .get("user_sid", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                Output::set(strand, out, Ref::slot::<0>(&borrow));
                Ok(())
            })
            .get("owner_sid", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                Output::set(strand, out, Ref::slot::<1>(&borrow));
                Ok(())
            })
            .get("primary_group_sid", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                Output::set(strand, out, Ref::slot::<2>(&borrow));
                Ok(())
            })
            .get("groups", |this, strand, out| {
                Output::set(strand, out, ArrayView::new(this, TokenGroups));
                Ok(())
            })
            .get("logon_sid", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                let sid = Ref::slot::<3>(&borrow);
                if sid.is_nil() {
                    Output::set(strand, out, Nil);
                } else {
                    Output::set(strand, out, sid);
                }
                Ok(())
            })
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
            let family = global.local.get(strand).target().operating_system.family();
            let vfs = global.local.get(strand).vfs();
            let name = match family {
                OperatingSystemFamily::Unix => {
                    let SecurityInfo::Unix(info) = security_info(strand, global)? else {
                        unreachable!("Unix target returned Windows security information")
                    };
                    error::io_result(strand, vfs.user_name(info.uid).await)?
                }
                OperatingSystemFamily::Windows => {
                    let SecurityInfo::Windows(info) = security_info(strand, global)? else {
                        unreachable!("Windows target returned Unix security information")
                    };
                    error::io_result(strand, vfs.sid_name(&info.user_sid).await)?.name
                }
            };
            Output::set(strand, out, name.as_str());
            Ok(())
        })
        .commit();

    builder
        .module("security.unix")
        .value("Identity", global.types.unix_identity)
        .value("Acl", global.types.posix_acl)
        .value("Ace", global.types.posix_ace)
        .value("Permission", global.types.permission)
        .function_with_slots("id", async move |strand, args, mut out, [mut group_ids]| {
            let ([], []) = unpack!(strand, args, 0, 0)?;
            let SecurityInfo::Unix(info) = security_info(strand, global)? else {
                return Err(Error::not_supported(strand));
            };

            Output::set(
                strand,
                &mut group_ids,
                AsTuple::new(info.group_ids.iter().copied()),
            );

            global
                .types
                .unix_identity
                .create_with_annex(strand, Identity, info, &mut out);
            global
                .types
                .unix_identity
                .cast(&out)
                .unwrap()
                .enter_sync(strand, |strand, this| {
                    Output::set(
                        strand,
                        Mut::slot_mut::<0>(&mut this.borrow_mut_unwrap()),
                        &group_ids,
                    );
                });
            Ok(())
        })
        .function("user_name", async move |strand, args, out| {
            let ([uid], []) = unpack!(strand, args, 1, 0)?;
            if global.local.get(strand).target().operating_system.family()
                != OperatingSystemFamily::Unix
            {
                return Err(Error::not_supported(strand));
            }
            let uid = uid.to_u32(strand)?;
            let vfs = global.local.get(strand).vfs();
            let name = error::io_result(strand, vfs.user_name(uid).await)?;
            Output::set(strand, out, name.as_str());
            Ok(())
        })
        .function("user_id", async move |strand, args, out| {
            let ([name], []) = unpack!(strand, args, 1, 0)?;
            if global.local.get(strand).target().operating_system.family()
                != OperatingSystemFamily::Unix
            {
                return Err(Error::not_supported(strand));
            }
            let name = name
                .as_str(strand)
                .ok_or_else(|| Error::type_error(strand, "user_id: expected Str"))?
                .to_string();
            let vfs = global.local.get(strand).vfs();
            let uid = error::io_result(strand, vfs.user_id(&name).await)?;
            Output::set(strand, out, uid);
            Ok(())
        })
        .function("group_name", async move |strand, args, out| {
            let ([gid], []) = unpack!(strand, args, 1, 0)?;
            if global.local.get(strand).target().operating_system.family()
                != OperatingSystemFamily::Unix
            {
                return Err(Error::not_supported(strand));
            }
            let gid = gid.to_u32(strand)?;
            let vfs = global.local.get(strand).vfs();
            let name = error::io_result(strand, vfs.group_name(gid).await)?;
            Output::set(strand, out, name.as_str());
            Ok(())
        })
        .function("group_id", async move |strand, args, out| {
            let ([name], []) = unpack!(strand, args, 1, 0)?;
            if global.local.get(strand).target().operating_system.family()
                != OperatingSystemFamily::Unix
            {
                return Err(Error::not_supported(strand));
            }
            let name = name
                .as_str(strand)
                .ok_or_else(|| Error::type_error(strand, "group_id: expected Str"))?
                .to_string();
            let vfs = global.local.get(strand).vfs();
            let gid = error::io_result(strand, vfs.group_id(&name).await)?;
            Output::set(strand, out, gid);
            Ok(())
        })
        .commit();

    builder
        .module("security.nfs4")
        .value("Acl", global.types.nfs4_acl)
        .value("Ace", global.types.nfs4_ace)
        .value("Mask", global.types.nfs4_ace_mask)
        .value("Flags", global.types.nfs4_ace_flags)
        .commit();

    builder
        .module("security.macos")
        .value("Acl", global.types.macos_acl)
        .value("Ace", global.types.macos_ace)
        .value("Mask", global.types.macos_ace_mask)
        .value("Flags", global.types.macos_ace_flags)
        .commit();

    builder
        .module("security.windows")
        .value("AccessMask", global.types.access_mask)
        .value("SecDescControl", global.types.sec_desc_control)
        .value("SecInfo", global.types.sec_info)
        .value("TokenGroupAttributes", global.types.token_group_attributes)
        .value("Acl", global.types.acl)
        .value("Ace", global.types.ace)
        .value("SecDesc", global.types.sec_desc)
        .value("Sid", global.types.sid)
        .value("SidName", global.types.sid_name)
        .value("TokenGroup", global.types.token_group)
        .value("TokenInfo", global.types.token_info)
        .function_with_slots(
            "token_info",
            async move |strand, args, mut out, [mut sid]| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let SecurityInfo::Windows(info) = security_info(strand, global)? else {
                    return Err(Error::not_supported(strand));
                };

                global.types.token_info.create_with_annex(
                    strand,
                    TokenInfo,
                    info.clone(),
                    &mut out,
                );
                global
                    .types
                    .token_info
                    .cast(&out)
                    .unwrap()
                    .enter_sync(strand, |strand, this| {
                        for (slot, value) in [
                            (0, info.user_sid.clone()),
                            (1, info.owner_sid.clone()),
                            (2, info.primary_group_sid.clone()),
                        ] {
                            create_sid(strand, global, value, &mut sid);
                            let mut borrow = this.borrow_mut_unwrap();
                            match slot {
                                0 => Output::set(strand, Mut::slot_mut::<0>(&mut borrow), &sid),
                                1 => Output::set(strand, Mut::slot_mut::<1>(&mut borrow), &sid),
                                2 => Output::set(strand, Mut::slot_mut::<2>(&mut borrow), &sid),
                                _ => unreachable!(),
                            }
                        }

                        if let Some(logon_sid) = info.logon_sid().cloned() {
                            create_sid(strand, global, logon_sid, &mut sid);
                            Output::set(
                                strand,
                                Mut::slot_mut::<3>(&mut this.borrow_mut_unwrap()),
                                &sid,
                            );
                        }
                    });
                Ok(())
            },
        )
        .commit();
}
