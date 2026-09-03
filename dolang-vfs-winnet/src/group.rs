//! Local group management.

use std::collections::VecDeque;

use dolang_vfs::{
    Vfs,
    error::{Error, ErrorKind},
    security::SidName,
};
use dolang_winterop::security::Sid;

use crate::{
    api::{Paged, call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

pub use crate::wire::{GroupCreate as Create, GroupInfo as Info, GroupUpdate as Update};

/// A SID-stable Windows local group.
#[derive(Clone)]
pub struct Group {
    vfs: Vfs,
    sid: Sid,
    name: String,
}

/// Returns a capability for an already-enumerated group.
pub fn from_info(vfs: &Vfs, info: &Info) -> Group {
    Group {
        vfs: vfs.clone(),
        sid: info.sid().clone(),
        name: info.name().into(),
    }
}

/// Looks up an existing group by name.
pub async fn by_name(vfs: &Vfs, name: &str) -> Result<Group, Error> {
    match call(vfs, WinNetRequest::GroupByName { name: name.into() }).await? {
        WinNetResponse::Group { name, sid } => Ok(Group {
            vfs: vfs.clone(),
            sid,
            name,
        }),
        _ => Err(unexpected("GroupByName")),
    }
}

/// Looks up an existing group by SID.
pub async fn by_sid(vfs: &Vfs, sid: &Sid) -> Result<Group, Error> {
    let resolved = vfs.sid_name(sid).await?;
    let group = by_name(vfs, resolved.name()).await?;
    if group.sid != *sid {
        return Err(Error::new(
            ErrorKind::NotFound,
            "resolved group name does not identify the requested SID",
        ));
    }
    Ok(group)
}

/// Creates a group.
pub async fn create(vfs: &Vfs, create: Create) -> Result<Group, Error> {
    match call(vfs, WinNetRequest::CreateGroup(create)).await? {
        WinNetResponse::Group { name, sid } => Ok(Group {
            vfs: vfs.clone(),
            sid,
            name,
        }),
        _ => Err(unexpected("CreateGroup")),
    }
}

/// Enumerates local groups.
pub fn enumerate(vfs: &Vfs) -> Groups {
    Groups(Paged::new(vfs))
}

impl Group {
    /// The group SID, which survives a rename.
    pub fn sid(&self) -> &Sid {
        &self.sid
    }

    /// Re-resolves the group name after a not-found failure.
    ///
    /// The SID is the stable identity, so a name that no longer resolves the
    /// group usually means it was renamed behind our back.
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

    /// Issues a request, retrying once under a re-resolved name.
    async fn request(
        &mut self,
        make: impl Fn(&str, &Sid) -> WinNetRequest,
    ) -> Result<WinNetResponse, Error> {
        match call(&self.vfs, make(&self.name, &self.sid)).await {
            Ok(response) => Ok(response),
            Err(error) => {
                self.retry_name(error).await?;
                call(&self.vfs, make(&self.name, &self.sid)).await
            }
        }
    }

    /// Reads fresh group information.
    pub async fn info(&mut self) -> Result<Info, Error> {
        match self
            .request(|name, sid| WinNetRequest::GroupInfo {
                name: name.into(),
                sid: sid.clone(),
            })
            .await?
        {
            WinNetResponse::GroupInfo(info) => Ok(info),
            _ => Err(unexpected("GroupInfo")),
        }
    }

    /// Applies the supplied changes and returns fresh group information.
    pub async fn update(&mut self, update: Update) -> Result<Info, Error> {
        match self
            .request(|name, sid| WinNetRequest::GroupUpdate {
                name: name.into(),
                sid: sid.clone(),
                update: update.clone(),
            })
            .await?
        {
            WinNetResponse::GroupInfo(info) => {
                self.name.clone_from(&info.name);
                Ok(info)
            }
            _ => Err(unexpected("GroupUpdate")),
        }
    }

    /// Enumerates the group's members.
    pub fn members(&self) -> Members {
        Members {
            group: self.clone(),
            resume: 0,
            entries: VecDeque::new(),
            done: false,
        }
    }

    /// Adds a member.
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

    /// Removes a member.
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

    /// Deletes the group.
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

/// A paged forward iterator over local groups.
pub struct Groups(Paged<Info>);

impl Groups {
    /// Yields the next group.
    pub async fn next_entry(&mut self) -> Result<Option<Info>, Error> {
        self.0
            .next_entry(
                |resume| WinNetRequest::GroupsPage { resume },
                |response| match response {
                    WinNetResponse::GroupsPage {
                        groups,
                        resume,
                        done,
                    } => Some((groups, resume, done)),
                    _ => None,
                },
                "group enumeration",
            )
            .await
    }
}

/// A paged forward iterator over the members of a group.
///
/// Membership pages are requested through the group itself so that a rename
/// mid-enumeration is recovered from the same way as any other operation.
pub struct Members {
    group: Group,
    resume: u64,
    entries: VecDeque<Sid>,
    done: bool,
}

impl Members {
    /// Yields the next member.
    pub async fn next_entry(&mut self) -> Result<Option<SidName>, Error> {
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
