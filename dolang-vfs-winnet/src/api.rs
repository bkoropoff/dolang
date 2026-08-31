use std::collections::VecDeque;

use dolang_vfs::{
    Vfs,
    error::{Error, ErrorKind},
    extension::VfsExtension,
};
use dolang_winterop::security::Sid;

use crate::wire::{
    GroupCreate, GroupInfo, GroupUpdate, UserCreate, UserInfo, UserUpdate, WinNetExt,
    WinNetRequest, WinNetResponse,
};

fn unsupported() -> Error {
    Error::new(
        ErrorKind::Unsupported,
        "Windows local-user management is not supported by this VFS backend",
    )
}

fn unexpected(request: &str) -> Error {
    Error::new(
        ErrorKind::InvalidData,
        format!("unexpected response for {request}"),
    )
}

async fn call(vfs: &Vfs, request: WinNetRequest) -> Result<WinNetResponse, Error> {
    if vfs
        .extensions()
        .maximum_common_version(WinNetExt::NAME, &[WinNetExt::VERSION])
        .is_none()
    {
        return Err(unsupported());
    }
    vfs.call_extension::<WinNetExt>(request).await?
}

/// A SID-stable Windows local user.
#[derive(Clone)]
pub struct User {
    vfs: Vfs,
    sid: Sid,
    name: String,
}

impl User {
    pub fn from_info(vfs: &Vfs, info: &UserInfo) -> Self {
        Self {
            vfs: vfs.clone(),
            sid: info.sid.clone(),
            name: info.name.clone(),
        }
    }
    pub async fn by_name(vfs: &Vfs, name: &str) -> Result<Self, Error> {
        match call(vfs, WinNetRequest::UserByName { name: name.into() }).await? {
            WinNetResponse::User { name, sid } => Ok(Self {
                vfs: vfs.clone(),
                sid,
                name,
            }),
            _ => Err(unexpected("UserByName")),
        }
    }
    pub async fn by_sid(vfs: &Vfs, sid: &Sid) -> Result<Self, Error> {
        let resolved = vfs.sid_name(sid).await?;
        let user = Self::by_name(vfs, resolved.name()).await?;
        if user.sid != *sid {
            return Err(Error::new(
                ErrorKind::NotFound,
                "resolved user name does not identify the requested SID",
            ));
        }
        Ok(user)
    }
    pub async fn create(vfs: &Vfs, create: UserCreate) -> Result<Self, Error> {
        match call(vfs, WinNetRequest::CreateUser(create)).await? {
            WinNetResponse::User { name, sid } => Ok(Self {
                vfs: vfs.clone(),
                sid,
                name,
            }),
            _ => Err(unexpected("CreateUser")),
        }
    }
    pub fn sid(&self) -> &Sid {
        &self.sid
    }
    async fn retry_name(&mut self, error: Error) -> Result<(), Error> {
        if error.kind() != ErrorKind::NotFound {
            return Err(error);
        }
        let resolved = self.vfs.sid_name(&self.sid).await.map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                Error::new(ErrorKind::NotFound, "Windows user SID no longer resolves")
            } else {
                e
            }
        })?;
        self.name = resolved.name().to_owned();
        Ok(())
    }
    pub async fn info(&mut self) -> Result<UserInfo, Error> {
        let response = match call(
            &self.vfs,
            WinNetRequest::Info {
                name: self.name.clone(),
                sid: self.sid.clone(),
            },
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                self.retry_name(e).await?;
                call(
                    &self.vfs,
                    WinNetRequest::Info {
                        name: self.name.clone(),
                        sid: self.sid.clone(),
                    },
                )
                .await?
            }
        };
        match response {
            WinNetResponse::Info(info) => {
                self.name.clone_from(&info.name);
                Ok(info)
            }
            _ => Err(unexpected("Info")),
        }
    }
    pub async fn update(&mut self, update: UserUpdate) -> Result<UserInfo, Error> {
        let response = match call(
            &self.vfs,
            WinNetRequest::Update {
                name: self.name.clone(),
                sid: self.sid.clone(),
                update: update.clone(),
            },
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                self.retry_name(e).await?;
                call(
                    &self.vfs,
                    WinNetRequest::Update {
                        name: self.name.clone(),
                        sid: self.sid.clone(),
                        update,
                    },
                )
                .await?
            }
        };
        match response {
            WinNetResponse::Info(info) => {
                self.name.clone_from(&info.name);
                Ok(info)
            }
            _ => Err(unexpected("Update")),
        }
    }
    pub async fn delete(mut self) -> Result<(), Error> {
        let response = match call(
            &self.vfs,
            WinNetRequest::Delete {
                name: self.name.clone(),
                sid: self.sid.clone(),
            },
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                self.retry_name(e).await?;
                call(
                    &self.vfs,
                    WinNetRequest::Delete {
                        name: self.name.clone(),
                        sid: self.sid.clone(),
                    },
                )
                .await?
            }
        };
        match response {
            WinNetResponse::Deleted => Ok(()),
            _ => Err(unexpected("Delete")),
        }
    }
}

/// A SID-stable Windows local group.
#[derive(Clone)]
pub struct Group {
    vfs: Vfs,
    sid: Sid,
    name: String,
}
impl Group {
    fn from_parts(vfs: &Vfs, name: String, sid: Sid) -> Self {
        Self {
            vfs: vfs.clone(),
            sid,
            name,
        }
    }
    pub fn from_info(vfs: &Vfs, info: &GroupInfo) -> Self {
        Self::from_parts(vfs, info.name.clone(), info.sid.clone())
    }
    pub async fn by_name(vfs: &Vfs, name: &str) -> Result<Self, Error> {
        match call(vfs, WinNetRequest::GroupByName { name: name.into() }).await? {
            WinNetResponse::Group { name, sid } => Ok(Self::from_parts(vfs, name, sid)),
            _ => Err(unexpected("GroupByName")),
        }
    }
    pub async fn by_sid(vfs: &Vfs, sid: &Sid) -> Result<Self, Error> {
        let resolved = vfs.sid_name(sid).await?;
        let group = Self::by_name(vfs, resolved.name()).await?;
        if group.sid != *sid {
            return Err(Error::new(
                ErrorKind::NotFound,
                "resolved group name does not identify the requested SID",
            ));
        }
        Ok(group)
    }
    pub async fn create(vfs: &Vfs, create: GroupCreate) -> Result<Self, Error> {
        match call(vfs, WinNetRequest::CreateGroup(create)).await? {
            WinNetResponse::Group { name, sid } => Ok(Self::from_parts(vfs, name, sid)),
            _ => Err(unexpected("CreateGroup")),
        }
    }
    pub fn sid(&self) -> &Sid {
        &self.sid
    }
    async fn retry_name(&mut self, error: Error) -> Result<(), Error> {
        if error.kind() != ErrorKind::NotFound {
            return Err(error);
        }
        let resolved = self.vfs.sid_name(&self.sid).await.map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                Error::new(ErrorKind::NotFound, "Windows group SID no longer resolves")
            } else {
                e
            }
        })?;
        self.name = resolved.name().to_owned();
        Ok(())
    }
    async fn request(
        &mut self,
        make: impl Fn(&str, &Sid) -> WinNetRequest,
    ) -> Result<WinNetResponse, Error> {
        match call(&self.vfs, make(&self.name, &self.sid)).await {
            Ok(v) => Ok(v),
            Err(e) => {
                self.retry_name(e).await?;
                call(&self.vfs, make(&self.name, &self.sid)).await
            }
        }
    }
    pub async fn info(&mut self) -> Result<GroupInfo, Error> {
        match self
            .request(|name, sid| WinNetRequest::GroupInfo {
                name: name.into(),
                sid: sid.clone(),
            })
            .await?
        {
            WinNetResponse::GroupInfo(v) => Ok(v),
            _ => Err(unexpected("GroupInfo")),
        }
    }
    pub async fn update(&mut self, update: GroupUpdate) -> Result<GroupInfo, Error> {
        let response = self
            .request(|name, sid| WinNetRequest::GroupUpdate {
                name: name.into(),
                sid: sid.clone(),
                update: update.clone(),
            })
            .await?;
        match response {
            WinNetResponse::GroupInfo(v) => {
                self.name.clone_from(&v.name);
                Ok(v)
            }
            _ => Err(unexpected("GroupUpdate")),
        }
    }
    pub fn members(&self) -> GroupMembers {
        GroupMembers {
            group: self.clone(),
            resume: 0,
            entries: VecDeque::new(),
            done: false,
        }
    }
    pub async fn add_member(&mut self, member: Sid) -> Result<(), Error> {
        match self
            .request(|name, sid| WinNetRequest::GroupAddMember {
                name: name.into(),
                sid: sid.clone(),
                member: member.clone(),
            })
            .await?
        {
            WinNetResponse::Unit => Ok(()),
            _ => Err(unexpected("GroupAddMember")),
        }
    }
    pub async fn remove_member(&mut self, member: Sid) -> Result<(), Error> {
        match self
            .request(|name, sid| WinNetRequest::GroupRemoveMember {
                name: name.into(),
                sid: sid.clone(),
                member: member.clone(),
            })
            .await?
        {
            WinNetResponse::Unit => Ok(()),
            _ => Err(unexpected("GroupRemoveMember")),
        }
    }
    pub async fn delete(mut self) -> Result<(), Error> {
        match self
            .request(|name, sid| WinNetRequest::GroupDelete {
                name: name.into(),
                sid: sid.clone(),
            })
            .await?
        {
            WinNetResponse::Deleted => Ok(()),
            _ => Err(unexpected("GroupDelete")),
        }
    }
}

pub struct Groups {
    vfs: Vfs,
    resume: u64,
    entries: VecDeque<GroupInfo>,
    done: bool,
}
impl Groups {
    pub fn new(vfs: &Vfs) -> Self {
        Self {
            vfs: vfs.clone(),
            resume: 0,
            entries: VecDeque::new(),
            done: false,
        }
    }
    pub async fn next_entry(&mut self) -> Result<Option<GroupInfo>, Error> {
        loop {
            if let Some(v) = self.entries.pop_front() {
                return Ok(Some(v));
            }
            if self.done {
                return Ok(None);
            }
            let previous_resume = self.resume;
            match call(
                &self.vfs,
                WinNetRequest::GroupsPage {
                    resume: self.resume,
                },
            )
            .await?
            {
                WinNetResponse::GroupsPage {
                    groups,
                    resume,
                    done,
                } => {
                    if groups.is_empty() && !done && resume == previous_resume {
                        return Err(Error::new(
                            ErrorKind::InvalidData,
                            "empty group enumeration page did not advance",
                        ));
                    }
                    self.resume = resume;
                    self.done = done;
                    self.entries.extend(groups);
                }
                _ => return Err(unexpected("GroupsPage")),
            }
        }
    }
}

pub struct GroupMembers {
    group: Group,
    resume: u64,
    entries: VecDeque<Sid>,
    done: bool,
}
impl GroupMembers {
    pub async fn next_entry(&mut self) -> Result<Option<dolang_vfs::security::SidName>, Error> {
        if let Some(sid) = self.entries.pop_front() {
            return self.group.vfs.sid_name(&sid).await.map(Some);
        }
        if self.done {
            return Ok(None);
        }
        let response = self
            .group
            .request(|name, sid| WinNetRequest::GroupMembersPage {
                name: name.into(),
                sid: sid.clone(),
                resume: self.resume,
            })
            .await?;
        match response {
            WinNetResponse::GroupMembersPage {
                members,
                resume,
                done,
            } => {
                self.resume = resume;
                self.done = done;
                self.entries.extend(members);
                if let Some(sid) = self.entries.pop_front() {
                    self.group.vfs.sid_name(&sid).await.map(Some)
                } else if done {
                    Ok(None)
                } else {
                    Err(Error::new(
                        ErrorKind::InvalidData,
                        "empty non-final group member page",
                    ))
                }
            }
            _ => Err(unexpected("GroupMembersPage")),
        }
    }
}

/// A paged forward iterator over normal local users.
pub struct Users {
    vfs: Vfs,
    resume: u64,
    entries: VecDeque<UserInfo>,
    done: bool,
}
impl Users {
    pub fn new(vfs: &Vfs) -> Self {
        Self {
            vfs: vfs.clone(),
            resume: 0,
            entries: VecDeque::new(),
            done: false,
        }
    }
    pub async fn next_entry(&mut self) -> Result<Option<UserInfo>, Error> {
        loop {
            if let Some(info) = self.entries.pop_front() {
                return Ok(Some(info));
            }
            if self.done {
                return Ok(None);
            }
            let previous_resume = self.resume;
            match call(
                &self.vfs,
                WinNetRequest::UsersPage {
                    resume: self.resume,
                },
            )
            .await?
            {
                WinNetResponse::UsersPage {
                    users,
                    resume,
                    done,
                } => {
                    if users.is_empty() && !done && resume == previous_resume {
                        return Err(Error::new(
                            ErrorKind::InvalidData,
                            "empty user enumeration page did not advance",
                        ));
                    }
                    self.resume = resume;
                    self.done = done;
                    self.entries.extend(users);
                }
                _ => return Err(unexpected("UsersPage")),
            }
        }
    }
}
