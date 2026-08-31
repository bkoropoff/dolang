use dolang_vfs::{
    error::Error,
    extension::{ExtContext, VfsExtension},
};
use dolang_winterop::security::Sid;
use serde::{Deserialize, Deserializer, Serialize};

#[cfg(windows)]
use crate::backend;

bitflags::bitflags! {
    /// Native `USER_INFO_4::usri4_flags`, including unknown bits.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct UserFlags: u32 {
        const SCRIPT = 0x0001;
        const ACCOUNT_DISABLED = 0x0002;
        const HOME_DIR_REQUIRED = 0x0008;
        const LOCKOUT = 0x0010;
        const PASSWORD_NOT_REQUIRED = 0x0020;
        const PASSWORD_CANNOT_CHANGE = 0x0040;
        const ENCRYPTED_TEXT_PASSWORD_ALLOWED = 0x0080;
        const TEMP_DUPLICATE_ACCOUNT = 0x0100;
        const NORMAL_ACCOUNT = 0x0200;
        const INTERDOMAIN_TRUST_ACCOUNT = 0x0800;
        const WORKSTATION_TRUST_ACCOUNT = 0x1000;
        const SERVER_TRUST_ACCOUNT = 0x2000;
        const PASSWORD_NEVER_EXPIRES = 0x10000;
        const MNS_LOGON_ACCOUNT = 0x20000;
        const SMARTCARD_REQUIRED = 0x40000;
        const TRUSTED_FOR_DELEGATION = 0x80000;
        const NOT_DELEGATED = 0x100000;
        const USE_DES_KEY_ONLY = 0x200000;
        const DONT_REQUIRE_PREAUTH = 0x400000;
        const PASSWORD_EXPIRED = 0x800000;
        const TRUSTED_TO_AUTHENTICATE_FOR_DELEGATION = 0x1000000;
        const NO_AUTH_DATA_REQUIRED = 0x2000000;
        const PARTIAL_SECRETS_ACCOUNT = 0x4000000;
        const USE_AES_KEYS = 0x8000000;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInfo {
    pub sid: Sid,
    pub name: String,
    pub full_name: Option<String>,
    pub comment: Option<String>,
    pub user_comment: Option<String>,
    pub home_dir: Option<String>,
    pub home_dir_drive: Option<String>,
    pub profile: Option<String>,
    pub script_path: Option<String>,
    pub flags: UserFlags,
    pub password_age: u64,
    pub password_expired: bool,
    pub last_logon: Option<u64>,
    pub account_expires: Option<u64>,
    pub bad_password_count: u32,
    pub logon_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct UserUpdate {
    pub name: Option<String>,
    pub password: Option<String>,
    pub full_name: Option<Option<String>>,
    pub comment: Option<Option<String>>,
    pub user_comment: Option<Option<String>>,
    pub home_dir: Option<Option<String>>,
    pub home_dir_drive: Option<Option<String>>,
    pub profile: Option<Option<String>>,
    pub script_path: Option<Option<String>>,
    pub account_expires: Option<Option<u64>>,
    pub(crate) set_flags: UserFlags,
    pub(crate) clear_flags: UserFlags,
}

#[derive(Serialize, Deserialize)]
struct UserUpdateWire {
    name: Option<String>,
    password: Option<String>,
    full_name: Option<Option<String>>,
    comment: Option<Option<String>>,
    user_comment: Option<Option<String>>,
    home_dir: Option<Option<String>>,
    home_dir_drive: Option<Option<String>>,
    profile: Option<Option<String>>,
    script_path: Option<Option<String>>,
    account_expires: Option<Option<u64>>,
    set_flags: UserFlags,
    clear_flags: UserFlags,
}

impl<'de> Deserialize<'de> for UserUpdate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let w = UserUpdateWire::deserialize(deserializer)?;
        if w.set_flags.intersects(w.clear_flags) {
            return Err(serde::de::Error::custom(
                "user flag set and clear masks overlap",
            ));
        }
        Ok(Self {
            name: w.name,
            password: w.password,
            full_name: w.full_name,
            comment: w.comment,
            user_comment: w.user_comment,
            home_dir: w.home_dir,
            home_dir_drive: w.home_dir_drive,
            profile: w.profile,
            script_path: w.script_path,
            account_expires: w.account_expires,
            set_flags: w.set_flags,
            clear_flags: w.clear_flags,
        })
    }
}

impl UserUpdate {
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }
    fn flag(mut self, flag: UserFlags, value: bool) -> Self {
        if value {
            self.set_flags.insert(flag);
            self.clear_flags.remove(flag);
        } else {
            self.clear_flags.insert(flag);
            self.set_flags.remove(flag);
        }
        self
    }
    pub fn disabled(self, value: bool) -> Self {
        self.flag(UserFlags::ACCOUNT_DISABLED, value)
    }
    pub fn password_never_expires(self, value: bool) -> Self {
        self.flag(UserFlags::PASSWORD_NEVER_EXPIRES, value)
    }
    pub fn password_cannot_change(self, value: bool) -> Self {
        self.flag(UserFlags::PASSWORD_CANNOT_CHANGE, value)
    }
    pub fn password(mut self, value: String) -> Self {
        self.password = Some(value);
        self
    }
    pub fn full_name(mut self, value: Option<String>) -> Self {
        self.full_name = Some(value);
        self
    }
    pub fn comment(mut self, value: Option<String>) -> Self {
        self.comment = Some(value);
        self
    }
    pub fn user_comment(mut self, value: Option<String>) -> Self {
        self.user_comment = Some(value);
        self
    }
    pub fn home_dir(mut self, value: Option<String>) -> Self {
        self.home_dir = Some(value);
        self
    }
    pub fn home_dir_drive(mut self, value: Option<String>) -> Self {
        self.home_dir_drive = Some(value);
        self
    }
    pub fn profile(mut self, value: Option<String>) -> Self {
        self.profile = Some(value);
        self
    }
    pub fn script_path(mut self, value: Option<String>) -> Self {
        self.script_path = Some(value);
        self
    }
    pub fn account_expires(mut self, value: Option<u64>) -> Self {
        self.account_expires = Some(value);
        self
    }
    pub fn set_flags(&self) -> UserFlags {
        self.set_flags
    }
    pub fn clear_flags(&self) -> UserFlags {
        self.clear_flags
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCreate {
    pub name: String,
    pub password: String,
    pub update: UserUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupInfo {
    pub sid: Sid,
    pub name: String,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdate {
    pub name: Option<String>,
    pub comment: Option<Option<String>>,
}
impl GroupUpdate {
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }
    pub fn comment(mut self, value: Option<String>) -> Self {
        self.comment = Some(value);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCreate {
    pub name: String,
    pub comment: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) enum WinNetRequest {
    UserByName {
        name: String,
    },
    UserBySid {
        sid: Sid,
    },
    UsersPage {
        resume: u64,
    },
    CreateUser(UserCreate),
    Info {
        name: String,
        sid: Sid,
    },
    Update {
        name: String,
        sid: Sid,
        update: UserUpdate,
    },
    Delete {
        name: String,
        sid: Sid,
    },
    GroupByName {
        name: String,
    },
    GroupsPage {
        resume: u64,
    },
    CreateGroup(GroupCreate),
    GroupInfo {
        name: String,
        sid: Sid,
    },
    GroupUpdate {
        name: String,
        sid: Sid,
        update: GroupUpdate,
    },
    GroupMembersPage {
        name: String,
        sid: Sid,
        resume: u64,
    },
    GroupAddMember {
        name: String,
        sid: Sid,
        member: Sid,
    },
    GroupRemoveMember {
        name: String,
        sid: Sid,
        member: Sid,
    },
    GroupDelete {
        name: String,
        sid: Sid,
    },
}

#[derive(Serialize, Deserialize)]
pub(crate) enum WinNetResponse {
    User {
        name: String,
        sid: Sid,
    },
    UsersPage {
        users: Vec<UserInfo>,
        resume: u64,
        done: bool,
    },
    Info(UserInfo),
    Deleted,
    Group {
        name: String,
        sid: Sid,
    },
    GroupsPage {
        groups: Vec<GroupInfo>,
        resume: u64,
        done: bool,
    },
    GroupInfo(GroupInfo),
    GroupMembersPage {
        members: Vec<Sid>,
        resume: u64,
        done: bool,
    },
    Unit,
}

pub(crate) struct WinNetExt;
impl VfsExtension for WinNetExt {
    type Request = WinNetRequest;
    type Response = Result<WinNetResponse, Error>;
    const NAME: &'static str = "dolang-vfs-winnet";
    const VERSION: u16 = 2;
    const AVAILABLE: bool = cfg!(windows);
    async fn handle(&self, ctx: &mut ExtContext<'_>, request: WinNetRequest) -> Self::Response {
        #[cfg(windows)]
        return backend::handle(ctx, request).await;
        #[cfg(not(windows))]
        {
            let _ = (ctx, request);
            unreachable!("unavailable VFS extension was dispatched")
        }
    }
}
dolang_vfs::extension::vfs_extension!(WinNetExt);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flags_preserve_unknown_bits() {
        let f = UserFlags::from_bits_retain(0x8000_0202);
        let b = postcard::to_stdvec(&f).unwrap();
        assert_eq!(postcard::from_bytes::<UserFlags>(&b).unwrap(), f);
    }
    #[test]
    fn builder_moves_semantic_bits() {
        let u = UserUpdate::default().disabled(true).disabled(false);
        assert!(!u.set_flags().contains(UserFlags::ACCOUNT_DISABLED));
        assert!(u.clear_flags().contains(UserFlags::ACCOUNT_DISABLED));
    }
    #[test]
    fn updates_round_trip_renames() {
        let user = UserUpdate::default().name("new-user".into());
        let bytes = postcard::to_stdvec(&user).unwrap();
        assert_eq!(postcard::from_bytes::<UserUpdate>(&bytes).unwrap(), user);
        let group = GroupUpdate::default()
            .name("new-group".into())
            .comment(None);
        let bytes = postcard::to_stdvec(&group).unwrap();
        assert_eq!(postcard::from_bytes::<GroupUpdate>(&bytes).unwrap(), group);
    }
    #[test]
    fn resume_handles_preserve_64_bits() {
        let request = WinNetRequest::GroupsPage {
            resume: u64::MAX - 1,
        };
        let bytes = postcard::to_stdvec(&request).unwrap();
        match postcard::from_bytes::<WinNetRequest>(&bytes).unwrap() {
            WinNetRequest::GroupsPage { resume } => assert_eq!(resume, u64::MAX - 1),
            _ => panic!("wrong request variant"),
        }
    }
    #[test]
    fn overlap_is_rejected() {
        let bytes = postcard::to_stdvec(&UserUpdateWire {
            name: None,
            password: None,
            full_name: None,
            comment: None,
            user_comment: None,
            home_dir: None,
            home_dir_drive: None,
            profile: None,
            script_path: None,
            account_expires: None,
            set_flags: UserFlags::ACCOUNT_DISABLED,
            clear_flags: UserFlags::ACCOUNT_DISABLED,
        })
        .unwrap();
        assert!(postcard::from_bytes::<UserUpdate>(&bytes).is_err());
    }
}
