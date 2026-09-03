use dolang_vfs::{
    error::Error,
    extension::{ExtContext, VfsExtension},
    path,
};
use dolang_winterop::security::{SecDesc, Sid};
use serde::{Deserialize, Serialize};

#[cfg(windows)]
use crate::backend;

/// Narrows a wire path to a Windows path.
///
/// The wire carries a path syntax tag, so a peer could in principle send a Unix
/// path where a Windows one belongs; every accessor here reports that as an
/// absent value rather than handing back a path the caller cannot use.
fn windows_path(path: &path::PathBuf) -> Option<path::Path<'_>> {
    (path.kind() == path::Kind::Windows).then(|| path.to_path())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShareKind {
    #[default]
    DiskTree,
    PrintQueue,
    Device,
    Ipc,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareInfo {
    pub(crate) name: String,
    pub(crate) kind: ShareKind,
    pub(crate) special: bool,
    pub(crate) temporary: bool,
    pub(crate) comment: Option<String>,
    pub(crate) max_uses: Option<u32>,
    pub(crate) current_uses: u32,
    pub(crate) path: path::PathBuf,
    pub(crate) sec_desc: Option<SecDesc>,
}
impl ShareInfo {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn kind(&self) -> ShareKind {
        self.kind
    }
    pub fn special(&self) -> bool {
        self.special
    }
    pub fn temporary(&self) -> bool {
        self.temporary
    }
    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }
    pub fn max_uses(&self) -> Option<u32> {
        self.max_uses
    }
    pub fn current_uses(&self) -> u32 {
        self.current_uses
    }
    pub fn path(&self) -> Option<path::Path<'_>> {
        windows_path(&self.path)
    }
    /// The share's security descriptor, or `None` when the share uses the
    /// server service's default security.
    pub fn sec_desc(&self) -> Option<&SecDesc> {
        self.sec_desc.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareCreate {
    pub(crate) name: String,
    pub(crate) path: path::PathBuf,
    pub(crate) kind: ShareKind,
    pub(crate) comment: Option<String>,
    pub(crate) max_uses: Option<u32>,
    pub(crate) special: bool,
    pub(crate) temporary: bool,
    pub(crate) sec_desc: Option<SecDesc>,
}
impl ShareCreate {
    pub fn new(name: String, path: path::PathBuf) -> Self {
        Self {
            name,
            path,
            kind: ShareKind::default(),
            comment: None,
            max_uses: None,
            special: false,
            temporary: false,
            sec_desc: None,
        }
    }
    pub fn kind(mut self, value: ShareKind) -> Self {
        self.kind = value;
        self
    }
    pub fn comment(mut self, value: Option<String>) -> Self {
        self.comment = value;
        self
    }
    pub fn max_uses(mut self, value: Option<u32>) -> Self {
        self.max_uses = value;
        self
    }
    pub fn special(mut self, value: bool) -> Self {
        self.special = value;
        self
    }
    pub fn temporary(mut self, value: bool) -> Self {
        self.temporary = value;
        self
    }
    pub fn sec_desc(mut self, value: SecDesc) -> Self {
        self.sec_desc = Some(value);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareUpdate {
    pub(crate) comment: Option<Option<String>>,
    pub(crate) max_uses: Option<Option<u32>>,
    pub(crate) sec_desc: Option<SecDesc>,
}
impl ShareUpdate {
    pub fn comment(mut self, value: Option<String>) -> Self {
        self.comment = Some(value);
        self
    }
    pub fn max_uses(mut self, value: Option<u32>) -> Self {
        self.max_uses = Some(value);
        self
    }
    pub fn sec_desc(mut self, value: SecDesc) -> Self {
        self.sec_desc = Some(value);
        self
    }
}

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
    pub(crate) sid: Sid,
    pub(crate) name: String,
    pub(crate) full_name: Option<String>,
    pub(crate) comment: Option<String>,
    pub(crate) user_comment: Option<String>,
    pub(crate) home_dir: Option<path::PathBuf>,
    pub(crate) home_dir_drive: Option<String>,
    pub(crate) profile: Option<path::PathBuf>,
    pub(crate) script_path: Option<path::PathBuf>,
    pub(crate) flags: UserFlags,
    pub(crate) password_age: u64,
    pub(crate) password_expired: bool,
    pub(crate) last_logon: Option<u64>,
    pub(crate) account_expires: Option<u64>,
    pub(crate) bad_password_count: u32,
    pub(crate) logon_count: u32,
}
impl UserInfo {
    pub fn sid(&self) -> &Sid {
        &self.sid
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn full_name(&self) -> Option<&str> {
        self.full_name.as_deref()
    }
    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }
    pub fn user_comment(&self) -> Option<&str> {
        self.user_comment.as_deref()
    }
    pub fn home_dir(&self) -> Option<path::Path<'_>> {
        self.home_dir.as_ref().and_then(windows_path)
    }
    pub fn home_dir_drive(&self) -> Option<&str> {
        self.home_dir_drive.as_deref()
    }
    pub fn profile(&self) -> Option<path::Path<'_>> {
        self.profile.as_ref().and_then(windows_path)
    }
    pub fn script_path(&self) -> Option<path::Path<'_>> {
        self.script_path.as_ref().and_then(windows_path)
    }
    pub fn flags(&self) -> UserFlags {
        self.flags
    }
    pub fn password_age(&self) -> u64 {
        self.password_age
    }
    pub fn password_expired(&self) -> bool {
        self.password_expired
    }
    pub fn last_logon(&self) -> Option<u64> {
        self.last_logon
    }
    pub fn account_expires(&self) -> Option<u64> {
        self.account_expires
    }
    pub fn bad_password_count(&self) -> u32 {
        self.bad_password_count
    }
    pub fn logon_count(&self) -> u32 {
        self.logon_count
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserUpdate {
    pub(crate) name: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) full_name: Option<Option<String>>,
    pub(crate) comment: Option<Option<String>>,
    pub(crate) user_comment: Option<Option<String>>,
    pub(crate) home_dir: Option<Option<path::PathBuf>>,
    pub(crate) home_dir_drive: Option<Option<String>>,
    pub(crate) profile: Option<Option<path::PathBuf>>,
    pub(crate) script_path: Option<Option<path::PathBuf>>,
    pub(crate) account_expires: Option<Option<u64>>,
    pub(crate) set_flags: UserFlags,
    pub(crate) clear_flags: UserFlags,
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
    pub fn home_dir(mut self, value: Option<path::PathBuf>) -> Self {
        self.home_dir = Some(value);
        self
    }
    pub fn home_dir_drive(mut self, value: Option<String>) -> Self {
        self.home_dir_drive = Some(value);
        self
    }
    pub fn profile(mut self, value: Option<path::PathBuf>) -> Self {
        self.profile = Some(value);
        self
    }
    pub fn script_path(mut self, value: Option<path::PathBuf>) -> Self {
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
    pub(crate) name: String,
    pub(crate) password: String,
    pub(crate) update: UserUpdate,
}
impl UserCreate {
    pub fn new(name: String, password: String) -> Self {
        Self {
            name,
            password,
            update: UserUpdate::default(),
        }
    }
    pub fn update(mut self, update: UserUpdate) -> Self {
        self.update = update;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupInfo {
    pub(crate) sid: Sid,
    pub(crate) name: String,
    pub(crate) comment: Option<String>,
}
impl GroupInfo {
    pub fn sid(&self) -> &Sid {
        &self.sid
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdate {
    pub(crate) name: Option<String>,
    pub(crate) comment: Option<Option<String>>,
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
    pub(crate) name: String,
    pub(crate) comment: Option<String>,
}
impl GroupCreate {
    pub fn new(name: String) -> Self {
        Self {
            name,
            comment: None,
        }
    }
    pub fn comment(mut self, value: Option<String>) -> Self {
        self.comment = value;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPolicy {
    pub(crate) min_password_length: u32,
    pub(crate) max_password_age: Option<u64>,
    pub(crate) min_password_age: u64,
    pub(crate) force_logoff: Option<u64>,
    pub(crate) password_history_length: u32,
    pub(crate) lockout_duration: u64,
    pub(crate) lockout_observation_window: u64,
    pub(crate) lockout_threshold: u32,
}
impl AccountPolicy {
    pub fn min_password_length(&self) -> u32 {
        self.min_password_length
    }
    pub fn max_password_age(&self) -> Option<u64> {
        self.max_password_age
    }
    pub fn min_password_age(&self) -> u64 {
        self.min_password_age
    }
    pub fn force_logoff(&self) -> Option<u64> {
        self.force_logoff
    }
    pub fn password_history_length(&self) -> u32 {
        self.password_history_length
    }
    pub fn lockout_duration(&self) -> u64 {
        self.lockout_duration
    }
    pub fn lockout_observation_window(&self) -> u64 {
        self.lockout_observation_window
    }
    pub fn lockout_threshold(&self) -> u32 {
        self.lockout_threshold
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPolicyUpdate {
    pub(crate) min_password_length: Option<u32>,
    pub(crate) max_password_age: Option<Option<u64>>,
    pub(crate) min_password_age: Option<u64>,
    pub(crate) force_logoff: Option<Option<u64>>,
    pub(crate) password_history_length: Option<u32>,
    pub(crate) lockout_duration: Option<u64>,
    pub(crate) lockout_observation_window: Option<u64>,
    pub(crate) lockout_threshold: Option<u32>,
}
impl AccountPolicyUpdate {
    pub fn min_password_length(mut self, value: u32) -> Self {
        self.min_password_length = Some(value);
        self
    }
    pub fn max_password_age(mut self, value: Option<u64>) -> Self {
        self.max_password_age = Some(value);
        self
    }
    pub fn min_password_age(mut self, value: u64) -> Self {
        self.min_password_age = Some(value);
        self
    }
    pub fn force_logoff(mut self, value: Option<u64>) -> Self {
        self.force_logoff = Some(value);
        self
    }
    pub fn password_history_length(mut self, value: u32) -> Self {
        self.password_history_length = Some(value);
        self
    }
    pub fn lockout_duration(mut self, value: u64) -> Self {
        self.lockout_duration = Some(value);
        self
    }
    pub fn lockout_observation_window(mut self, value: u64) -> Self {
        self.lockout_observation_window = Some(value);
        self
    }
    pub fn lockout_threshold(mut self, value: u32) -> Self {
        self.lockout_threshold = Some(value);
        self
    }
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
    AccountRights {
        sid: Sid,
    },
    GrantAccountRight {
        sid: Sid,
        right: String,
    },
    RevokeAccountRight {
        sid: Sid,
        right: String,
    },
    AccountPolicy,
    UpdateAccountPolicy(AccountPolicyUpdate),
    ShareInfo {
        name: String,
    },
    SharesPage {
        resume: u64,
    },
    CreateShare(ShareCreate),
    UpdateShare {
        name: String,
        update: ShareUpdate,
    },
    DeleteShare {
        name: String,
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
    Info(Box<UserInfo>),
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
    AccountRights(Vec<String>),
    AccountPolicy(AccountPolicy),
    ShareInfo(Box<ShareInfo>),
    SharesPage {
        shares: Vec<ShareInfo>,
        resume: u64,
        done: bool,
    },
}

pub(crate) struct WinNetExt;
impl VfsExtension for WinNetExt {
    type Request = WinNetRequest;
    type Response = Result<WinNetResponse, Error>;
    const NAME: &'static str = "dolang-vfs-winnet";
    const VERSION: u16 = 4;
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
    fn account_policy_updates_round_trip() {
        let update = AccountPolicyUpdate::default()
            .min_password_length(12)
            .max_password_age(None)
            .lockout_duration(300)
            .lockout_threshold(5);
        let bytes = postcard::to_stdvec(&update).unwrap();
        assert_eq!(
            postcard::from_bytes::<AccountPolicyUpdate>(&bytes).unwrap(),
            update
        );
    }
    #[test]
    fn shares_round_trip() {
        let descriptor = SecDesc::from_bytes(&[
            1, 0, 0, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ])
        .unwrap();
        let create = ShareCreate::new("docs".into(), path::PathBuf::from_windows(r"C:\docs"))
            .kind(ShareKind::Ipc)
            .comment(Some("comment".into()))
            .max_uses(Some(7))
            .special(true)
            .temporary(true)
            .sec_desc(descriptor.clone());
        let bytes = postcard::to_stdvec(&create).unwrap();
        assert_eq!(postcard::from_bytes::<ShareCreate>(&bytes).unwrap(), create);
        let update = ShareUpdate::default()
            .comment(None)
            .max_uses(None)
            .sec_desc(descriptor);
        let bytes = postcard::to_stdvec(&update).unwrap();
        assert_eq!(postcard::from_bytes::<ShareUpdate>(&bytes).unwrap(), update);
        let request = WinNetRequest::SharesPage {
            resume: u64::MAX - 1,
        };
        let bytes = postcard::to_stdvec(&request).unwrap();
        assert!(
            matches!(postcard::from_bytes::<WinNetRequest>(&bytes).unwrap(), WinNetRequest::SharesPage { resume } if resume == u64::MAX - 1)
        );
    }
}
