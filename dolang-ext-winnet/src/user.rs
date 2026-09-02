use std::time::{Duration, SystemTime};

use dolang::runtime::{
    Error, Instance, Object, Output, Result, Slot, State, Strand, Value,
    object::TypeBuilder,
    unpack,
    value::{Empty, Nil, TypeObject},
    vm::ModuleBuilder,
};
use dolang_ext_shell::ResultExt;
use dolang_vfs_winnet::{UserCreate, UserUpdate};

use crate::global::Global;

pub(crate) struct User(pub(crate) Option<dolang_vfs_winnet::User>);
pub(crate) struct Users(pub(crate) dolang_vfs_winnet::Users);
pub(crate) struct UserInfo;
pub(crate) struct UserFlags;
pub(crate) struct UserInfoAnnex<'v> {
    global: State<'v, Global<'v>>,
    info: dolang_vfs_winnet::UserInfo,
}

fn make_user<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    user: dolang_vfs_winnet::User,
    out: impl Output<'v>,
) {
    global.user.create(strand, User(Some(user)), out);
}
fn make_info<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    info: dolang_vfs_winnet::UserInfo,
    out: impl Output<'v>,
) {
    global
        .info
        .create_with_annex(strand, UserInfo, UserInfoAnnex { global, info }, out);
}
fn nullable_str<'v>(value: Option<&str>, out: impl Output<'v>, strand: &mut Strand<'v, '_>) {
    match value {
        Some(v) => Output::set(strand, out, v),
        None => Output::set(strand, out, Nil),
    }
}
fn nullable_windows_path<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: Option<&typed_path::Utf8WindowsPath>,
    out: Slot<'v, '_>,
) -> Result<'v, 's, ()> {
    match value {
        Some(path) => dolang_ext_shell::windows_path(strand, path.as_str(), out),
        None => {
            Output::set(strand, out, Nil);
            Ok(())
        }
    }
}
pub(crate) fn make_rights<'v, 's>(
    strand: &mut Strand<'v, 's>,
    rights: Vec<String>,
    mut out: Slot<'v, '_>,
) -> Result<'v, 's, ()> {
    strand.with_slots_sync(|strand, [mut item]| {
        Output::set(strand, &mut out, Empty::Array);
        let array = out.as_array(strand).unwrap();
        for right in rights {
            Output::set(strand, &mut item, right.as_str());
            array.push(strand, &item)?;
        }
        Ok(())
    })
}
fn nullable_time<'v, 's>(
    strand: &mut Strand<'v, 's>,
    seconds: Option<u64>,
    out: Slot<'v, '_>,
) -> Result<'v, 's, ()> {
    match seconds {
        Some(seconds) => dolang_ext_shell::datetime(
            strand,
            SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            out,
        )
        .map_err(|e| Error::runtime(strand, e)),
        None => {
            Output::set(strand, out, Nil);
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_from_slots<'v, 's>(
    strand: &mut Strand<'v, 's>,
    name: Option<Slot<'v, '_>>,
    password: Option<Slot<'v, '_>>,
    full_name: Option<Slot<'v, '_>>,
    comment: Option<Slot<'v, '_>>,
    user_comment: Option<Slot<'v, '_>>,
    home_dir: Option<Slot<'v, '_>>,
    home_dir_drive: Option<Slot<'v, '_>>,
    profile: Option<Slot<'v, '_>>,
    script_path: Option<Slot<'v, '_>>,
    account_expires: Option<Slot<'v, '_>>,
    disabled: Option<Slot<'v, '_>>,
    password_never_expires: Option<Slot<'v, '_>>,
    password_cannot_change: Option<Slot<'v, '_>>,
) -> Result<'v, 's, UserUpdate> {
    let mut update = UserUpdate::default();
    if let Some(value) = name {
        update = update.name(
            value
                .as_str(strand)
                .ok_or_else(|| Error::type_error(strand, "name must be a Str"))?
                .to_string(),
        );
    }
    macro_rules! nullable_text {
        ($slot:expr, $method:ident, $name:literal) => {
            if let Some(value) = $slot {
                update = if value.is_nil() {
                    update.$method(None)
                } else {
                    update.$method(Some(
                        value
                            .as_str(strand)
                            .ok_or_else(|| {
                                Error::type_error(strand, concat!($name, " must be a Str or nil"))
                            })?
                            .to_string(),
                    ))
                };
            }
        };
    }
    if let Some(value) = password {
        update = update.password(
            value
                .as_str(strand)
                .ok_or_else(|| Error::type_error(strand, "password must be a Str"))?
                .to_string(),
        );
    }
    nullable_text!(full_name, full_name, "full_name");
    nullable_text!(comment, comment, "comment");
    nullable_text!(user_comment, user_comment, "user_comment");
    nullable_text!(home_dir_drive, home_dir_drive, "home_dir_drive");
    macro_rules! nullable_windows_path {
        ($slot:expr, $method:ident, $name:literal) => {
            if let Some(value) = $slot {
                update = if value.is_nil() {
                    update.$method(None)
                } else {
                    let path =
                        dolang_ext_shell::as_windows_path(strand, &value).ok_or_else(|| {
                            Error::type_error(
                                strand,
                                concat!($name, " must be an fs.windows.Path or nil"),
                            )
                        })?;
                    update.$method(Some(path))
                };
            }
        };
    }
    nullable_windows_path!(home_dir, home_dir, "home_dir");
    nullable_windows_path!(profile, profile, "profile");
    nullable_windows_path!(script_path, script_path, "script_path");
    if let Some(value) = account_expires {
        update = if value.is_nil() {
            update.account_expires(None)
        } else {
            let time = dolang_ext_shell::as_datetime(strand, &value).ok_or_else(|| {
                Error::type_error(strand, "account_expires must be a time.DateTime or nil")
            })?;
            let seconds = time
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|_| Error::value(strand, "account_expires precedes the Unix epoch"))?
                .as_secs();
            update.account_expires(Some(seconds))
        };
    }
    macro_rules! boolean {
        ($slot:expr, $method:ident, $name:literal) => {
            if let Some(value) = $slot {
                let value = value
                    .as_bool(strand)
                    .ok_or_else(|| Error::type_error(strand, concat!($name, " must be a Bool")))?;
                update = update.$method(value);
            }
        };
    }
    boolean!(disabled, disabled, "disabled");
    boolean!(
        password_never_expires,
        password_never_expires,
        "password_never_expires"
    );
    boolean!(
        password_cannot_change,
        password_cannot_change,
        "password_cannot_change"
    );
    Ok(update)
}

impl<'v> Object<'v> for User {
    const NAME: &'v str = "User";
    const MODULE: &'v str = "winnet";
    type Annex = ();
    type Type = ();
    type TypeAnnex = ();
    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("sid", |this, strand, mut out| {
                let b = this.borrow(strand)?;
                let u =
                    b.0.as_ref()
                        .ok_or_else(|| Error::state_error(strand, "user was deleted"))?;
                dolang_ext_shell::windows_sid(strand, u.sid().clone(), &mut out);
                Ok(())
            })
            .method("info", async move |this, strand, args, out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let global = strand.state::<Global<'v>>();
                let info = {
                    let mut b = this.borrow_mut(strand)?;
                    b.0.as_mut()
                        .ok_or_else(|| Error::state_error(strand, "user was deleted"))?
                        .info()
                        .await
                        .into_sys(strand)?
                };
                make_info(strand, global, info, out);
                Ok(())
            })
            .method("rights", async move |this, strand, args, out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let rights = this
                    .borrow(strand)?
                    .0
                    .as_ref()
                    .ok_or_else(|| Error::state_error(strand, "user was deleted"))?
                    .rights()
                    .await
                    .into_sys(strand)?;
                make_rights(strand, rights, out)
            })
            .method("grant_right", async move |this, strand, args, out| {
                let ([right], []) = unpack!(strand, args, 1, 0)?;
                let right = right
                    .as_str(strand)
                    .ok_or_else(|| Error::type_error(strand, "right must be a Str"))?
                    .to_string();
                this.borrow(strand)?
                    .0
                    .as_ref()
                    .ok_or_else(|| Error::state_error(strand, "user was deleted"))?
                    .grant_right(right)
                    .await
                    .into_sys(strand)?;
                Output::set(strand, out, Nil);
                Ok(())
            })
            .method("revoke_right", async move |this, strand, args, out| {
                let ([right], []) = unpack!(strand, args, 1, 0)?;
                let right = right
                    .as_str(strand)
                    .ok_or_else(|| Error::type_error(strand, "right must be a Str"))?
                    .to_string();
                this.borrow(strand)?
                    .0
                    .as_ref()
                    .ok_or_else(|| Error::state_error(strand, "user was deleted"))?
                    .revoke_right(right)
                    .await
                    .into_sys(strand)?;
                Output::set(strand, out, Nil);
                Ok(())
            })
            .method("update", async move |this, strand, args, out| {
                let global = strand.state::<Global<'v>>();
                let name_sym = global.name;
                let password_sym = global.password;
                let full_name_sym = global.full_name;
                let comment_sym = global.comment;
                let user_comment_sym = global.user_comment;
                let home_dir_sym = global.home_dir;
                let home_dir_drive_sym = global.home_dir_drive;
                let profile_sym = global.profile;
                let script_path_sym = global.script_path;
                let account_expires_sym = global.account_expires;
                let disabled_sym = global.disabled;
                let password_never_expires_sym = global.password_never_expires;
                let password_cannot_change_sym = global.password_cannot_change;
                let (
                    [],
                    [
                        name,
                        password,
                        full_name,
                        comment,
                        user_comment,
                        home_dir,
                        home_dir_drive,
                        profile,
                        script_path,
                        account_expires,
                        disabled,
                        password_never_expires,
                        password_cannot_change,
                    ],
                ) = unpack!(
                    strand,
                    args,
                    0,
                    0,
                    name_sym = None,
                    password_sym = None,
                    full_name_sym = None,
                    comment_sym = None,
                    user_comment_sym = None,
                    home_dir_sym = None,
                    home_dir_drive_sym = None,
                    profile_sym = None,
                    script_path_sym = None,
                    account_expires_sym = None,
                    disabled_sym = None,
                    password_never_expires_sym = None,
                    password_cannot_change_sym = None
                )?;
                let update = update_from_slots(
                    strand,
                    name,
                    password,
                    full_name,
                    comment,
                    user_comment,
                    home_dir,
                    home_dir_drive,
                    profile,
                    script_path,
                    account_expires,
                    disabled,
                    password_never_expires,
                    password_cannot_change,
                )?;
                let info = {
                    let mut b = this.borrow_mut(strand)?;
                    b.0.as_mut()
                        .ok_or_else(|| Error::state_error(strand, "user was deleted"))?
                        .update(update)
                        .await
                        .into_sys(strand)?
                };
                make_info(strand, global, info, out);
                Ok(())
            })
            .method("delete", async move |this, strand, args, out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let user = this
                    .borrow_mut(strand)?
                    .0
                    .take()
                    .ok_or_else(|| Error::state_error(strand, "user was deleted"))?;
                user.delete().await.into_sys(strand)?;
                Output::set(strand, out, Nil);
                Ok(())
            })
    }
}

impl<'v> Object<'v> for Users {
    const NAME: &'v str = "Users";
    const MODULE: &'v str = "winnet";
    type Annex = State<'v, Global<'v>>;
    type Type = ();
    type TypeAnnex = ();
    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder.supertype(TypeObject::Iter)
    }
    async fn iter<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        Output::set(strand, out, this);
        Ok(())
    }
    async fn next<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, bool> {
        let user = this
            .borrow_mut(strand)?
            .0
            .next_entry()
            .await
            .into_sys(strand)?;
        if let Some(user) = user {
            make_info(strand, *this.annex(), user, out);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl<'v> Object<'v> for UserFlags {
    const NAME: &'v str = "UserFlags";
    const MODULE: &'v str = "winnet";
    type Annex = dolang_vfs_winnet::UserFlags;
    type Type = ();
    type TypeAnnex = ();
    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder.get("int", |this, strand, out| {
            Output::set(strand, out, u64::from(this.annex().bits()));
            Ok(())
        })
    }
}

impl<'v> Object<'v> for UserInfo {
    const NAME: &'v str = "UserInfo";
    const MODULE: &'v str = "winnet";
    type Annex = UserInfoAnnex<'v>;
    type Type = ();
    type TypeAnnex = ();
    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("sid", |this, strand, mut out| {
                dolang_ext_shell::windows_sid(strand, this.annex().info.sid().clone(), &mut out);
                Ok(())
            })
            .get("name", |this, strand, out| {
                Output::set(strand, out, this.annex().info.name());
                Ok(())
            })
            .get("full_name", |this, strand, out| {
                nullable_str(this.annex().info.full_name(), out, strand);
                Ok(())
            })
            .get("comment", |this, strand, out| {
                nullable_str(this.annex().info.comment(), out, strand);
                Ok(())
            })
            .get("user_comment", |this, strand, out| {
                nullable_str(this.annex().info.user_comment(), out, strand);
                Ok(())
            })
            .get("home_dir", |this, strand, out| {
                nullable_windows_path(strand, this.annex().info.home_dir(), out)
            })
            .get("home_dir_drive", |this, strand, out| {
                nullable_str(this.annex().info.home_dir_drive(), out, strand);
                Ok(())
            })
            .get("profile", |this, strand, out| {
                nullable_windows_path(strand, this.annex().info.profile(), out)
            })
            .get("script_path", |this, strand, out| {
                nullable_windows_path(strand, this.annex().info.script_path(), out)
            })
            .get("flags", |this, strand, out| {
                let a = this.annex();
                a.global
                    .flags
                    .create_with_annex(strand, UserFlags, a.info.flags(), out);
                Ok(())
            })
            .get("disabled", |this, strand, out| {
                Output::set(
                    strand,
                    out,
                    this.annex()
                        .info
                        .flags()
                        .contains(dolang_vfs_winnet::UserFlags::ACCOUNT_DISABLED),
                );
                Ok(())
            })
            .get("password_never_expires", |this, strand, out| {
                Output::set(
                    strand,
                    out,
                    this.annex()
                        .info
                        .flags()
                        .contains(dolang_vfs_winnet::UserFlags::PASSWORD_NEVER_EXPIRES),
                );
                Ok(())
            })
            .get("password_cannot_change", |this, strand, out| {
                Output::set(
                    strand,
                    out,
                    this.annex()
                        .info
                        .flags()
                        .contains(dolang_vfs_winnet::UserFlags::PASSWORD_CANNOT_CHANGE),
                );
                Ok(())
            })
            .get("password_age", |this, strand, out| {
                dolang_ext_shell::duration(
                    strand,
                    Duration::from_secs(this.annex().info.password_age()),
                    out,
                )
            })
            .get("password_expired", |this, strand, out| {
                Output::set(strand, out, this.annex().info.password_expired());
                Ok(())
            })
            .get("last_logon", |this, strand, out| {
                nullable_time(strand, this.annex().info.last_logon(), out)
            })
            .get("account_expires", |this, strand, out| {
                nullable_time(strand, this.annex().info.account_expires(), out)
            })
            .get("bad_password_count", |this, strand, out| {
                Output::set(strand, out, this.annex().info.bad_password_count());
                Ok(())
            })
            .get("logon_count", |this, strand, out| {
                Output::set(strand, out, this.annex().info.logon_count());
                Ok(())
            })
    }
}

fn principal<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    value: &Value<'v>,
) -> Result<'v, 's, ResultPrincipal> {
    if let Some(info) = global.info.cast(value) {
        return Ok(ResultPrincipal::Info(Box::new(
            info.enter_sync(strand, |_, info| info.annex().info.clone()),
        )));
    }
    if let Some(name) = value.as_str(strand) {
        return Ok(ResultPrincipal::Name(name.to_string()));
    }
    if let Some(sid) = dolang_ext_shell::as_windows_sid(strand, value) {
        return Ok(ResultPrincipal::Sid(sid));
    }
    Err(Error::type_error(
        strand,
        "principal must be an account name or security.windows.Sid",
    ))
}
enum ResultPrincipal {
    Name(String),
    Sid(dolang_winterop::security::Sid),
    Info(Box<dolang_vfs_winnet::UserInfo>),
}

pub(crate) fn configure_module<'v, 'a>(
    module: ModuleBuilder<'v, 'a>,
    global: State<'v, Global<'v>>,
) -> ModuleBuilder<'v, 'a> {
    module
        .value("User", global.user)
        .value("UserInfo", global.info)
        .value("UserFlags", global.flags)
        .function("user", async move |strand, args, out| {
            let ([value], []) = unpack!(strand, args, 1, 0)?;
            let vfs = dolang_ext_shell::vfs(strand);
            let user = match principal(strand, global, &value)? {
                ResultPrincipal::Name(name) => dolang_vfs_winnet::User::by_name(&vfs, &name).await,
                ResultPrincipal::Sid(sid) => dolang_vfs_winnet::User::by_sid(&vfs, &sid).await,
                ResultPrincipal::Info(info) => Ok(dolang_vfs_winnet::User::from_info(&vfs, &info)),
            }
            .into_sys(strand)?;
            make_user(strand, global, user, out);
            Ok(())
        })
        .function("users", async move |strand, args, out| {
            let ([], []) = unpack!(strand, args, 0, 0)?;
            global.users.create_with_annex(
                strand,
                Users(dolang_vfs_winnet::Users::new(&dolang_ext_shell::vfs(
                    strand,
                ))),
                global,
                out,
            );
            Ok(())
        })
        .function("create_user", async move |strand, args, out| {
            let name_sym = global.name;
            let password_sym = global.password;
            let full_name_sym = global.full_name;
            let comment_sym = global.comment;
            let user_comment_sym = global.user_comment;
            let home_dir_sym = global.home_dir;
            let home_dir_drive_sym = global.home_dir_drive;
            let profile_sym = global.profile;
            let script_path_sym = global.script_path;
            let account_expires_sym = global.account_expires;
            let disabled_sym = global.disabled;
            let password_never_expires_sym = global.password_never_expires;
            let password_cannot_change_sym = global.password_cannot_change;
            let (
                [name, password],
                [
                    full_name,
                    comment,
                    user_comment,
                    home_dir,
                    home_dir_drive,
                    profile,
                    script_path,
                    account_expires,
                    disabled,
                    password_never_expires,
                    password_cannot_change,
                ],
            ) = unpack!(
                strand,
                args,
                0,
                0,
                name_sym,
                password_sym,
                full_name_sym = None,
                comment_sym = None,
                user_comment_sym = None,
                home_dir_sym = None,
                home_dir_drive_sym = None,
                profile_sym = None,
                script_path_sym = None,
                account_expires_sym = None,
                disabled_sym = None,
                password_never_expires_sym = None,
                password_cannot_change_sym = None
            )?;
            let name = name
                .as_str(strand)
                .ok_or_else(|| Error::type_error(strand, "name must be a Str"))?
                .to_string();
            let password = password
                .as_str(strand)
                .ok_or_else(|| Error::type_error(strand, "password must be a Str"))?
                .to_string();
            let update = update_from_slots(
                strand,
                None,
                None,
                full_name,
                comment,
                user_comment,
                home_dir,
                home_dir_drive,
                profile,
                script_path,
                account_expires,
                disabled,
                password_never_expires,
                password_cannot_change,
            )?;
            let user = dolang_vfs_winnet::User::create(
                &dolang_ext_shell::vfs(strand),
                UserCreate::new(name, password).update(update),
            )
            .await
            .into_sys(strand)?;
            make_user(strand, global, user, out);
            Ok(())
        })
}
