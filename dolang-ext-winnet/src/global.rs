use crate::{
    group::{Group, GroupInfo, GroupMembers, Groups},
    user::{User, UserFlags, UserInfo, Users},
};
use dolang::runtime::{
    Sym, Type,
    vm::{Builder, Stateful},
};

pub(crate) struct Global<'v> {
    pub(crate) user: Type<'v, User>,
    pub(crate) info: Type<'v, UserInfo>,
    pub(crate) flags: Type<'v, UserFlags>,
    pub(crate) users: Type<'v, Users>,
    pub(crate) group: Type<'v, Group>,
    pub(crate) group_info: Type<'v, GroupInfo>,
    pub(crate) groups: Type<'v, Groups>,
    pub(crate) group_members: Type<'v, GroupMembers>,
    pub(crate) name: Sym<'v, 'v>,
    pub(crate) password: Sym<'v, 'v>,
    pub(crate) full_name: Sym<'v, 'v>,
    pub(crate) comment: Sym<'v, 'v>,
    pub(crate) user_comment: Sym<'v, 'v>,
    pub(crate) home_dir: Sym<'v, 'v>,
    pub(crate) home_dir_drive: Sym<'v, 'v>,
    pub(crate) profile: Sym<'v, 'v>,
    pub(crate) script_path: Sym<'v, 'v>,
    pub(crate) account_expires: Sym<'v, 'v>,
    pub(crate) disabled: Sym<'v, 'v>,
    pub(crate) password_never_expires: Sym<'v, 'v>,
    pub(crate) password_cannot_change: Sym<'v, 'v>,
}
pub struct Tag;
impl<'v> Stateful<'v> for Global<'v> {
    type Tag = Tag;
}
impl<'v> Global<'v> {
    pub(crate) fn new(builder: &mut Builder<'v>) -> Self {
        Self {
            user: builder.register_type(),
            info: builder.register_type(),
            flags: builder.register_type(),
            users: builder.register_type(),
            group: builder.register_type(),
            group_info: builder.register_type(),
            groups: builder.register_type(),
            group_members: builder.register_type(),
            name: builder.sym("name"),
            password: builder.sym("password"),
            full_name: builder.sym("full_name"),
            comment: builder.sym("comment"),
            user_comment: builder.sym("user_comment"),
            home_dir: builder.sym("home_dir"),
            home_dir_drive: builder.sym("home_dir_drive"),
            profile: builder.sym("profile"),
            script_path: builder.sym("script_path"),
            account_expires: builder.sym("account_expires"),
            disabled: builder.sym("disabled"),
            password_never_expires: builder.sym("password_never_expires"),
            password_cannot_change: builder.sym("password_cannot_change"),
        }
    }
}
