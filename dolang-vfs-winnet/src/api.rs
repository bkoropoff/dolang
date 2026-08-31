use std::collections::VecDeque;

use dolang_vfs::{
    Vfs,
    error::{Error, ErrorKind},
    extension::VfsExtension,
};
use dolang_winterop::security::Sid;

use crate::wire::{UserCreate, UserInfo, UserUpdate, WinNetExt, WinNetRequest, WinNetResponse};

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

/// A SID-stable Windows local user. The name is a refreshable cache.
#[derive(Clone)]
pub struct User {
    vfs: Vfs,
    sid: Sid,
    name: String,
}

impl User {
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
    pub fn name(&self) -> &str {
        &self.name
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
            WinNetResponse::Info(info) => Ok(info),
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
            WinNetResponse::Info(info) => Ok(info),
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

/// A paged forward iterator over normal local users.
pub struct Users {
    vfs: Vfs,
    resume: u32,
    entries: VecDeque<User>,
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
    pub async fn next_entry(&mut self) -> Result<Option<User>, Error> {
        if let Some(user) = self.entries.pop_front() {
            return Ok(Some(user));
        }
        if self.done {
            return Ok(None);
        }
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
                self.resume = resume;
                self.done = done;
                self.entries
                    .extend(users.into_iter().map(|(name, sid)| User {
                        vfs: self.vfs.clone(),
                        sid,
                        name,
                    }));
                self.entries.pop_front().map_or_else(
                    || {
                        if done {
                            Ok(None)
                        } else {
                            Err(Error::new(
                                ErrorKind::InvalidData,
                                "empty non-final user enumeration page",
                            ))
                        }
                    },
                    |u| Ok(Some(u)),
                )
            }
            _ => Err(unexpected("UsersPage")),
        }
    }
}
