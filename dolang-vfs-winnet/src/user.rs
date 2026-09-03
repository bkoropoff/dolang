//! Local user account management.

use dolang_vfs::{
    Vfs,
    error::{Error, ErrorKind},
};
use dolang_winterop::security::Sid;

use crate::{
    api::{Paged, call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

pub use crate::wire::{
    UserCreate as Create, UserFlags as Flags, UserInfo as Info, UserUpdate as Update,
};

/// A SID-stable Windows local user.
#[derive(Clone)]
pub struct User {
    vfs: Vfs,
    sid: Sid,
    name: String,
}

/// Returns a capability for an already-enumerated user.
pub fn from_info(vfs: &Vfs, info: &Info) -> User {
    User {
        vfs: vfs.clone(),
        sid: info.sid().clone(),
        name: info.name().into(),
    }
}

/// Looks up an existing account by name.
pub async fn by_name(vfs: &Vfs, name: &str) -> Result<User, Error> {
    match call(vfs, WinNetRequest::UserByName { name: name.into() }).await? {
        WinNetResponse::User { name, sid } => Ok(User {
            vfs: vfs.clone(),
            sid,
            name,
        }),
        _ => Err(unexpected("UserByName")),
    }
}

/// Looks up an existing account by SID.
pub async fn by_sid(vfs: &Vfs, sid: &Sid) -> Result<User, Error> {
    let resolved = vfs.sid_name(sid).await?;
    let user = by_name(vfs, resolved.name()).await?;
    if user.sid != *sid {
        return Err(Error::new(
            ErrorKind::NotFound,
            "resolved user name does not identify the requested SID",
        ));
    }
    Ok(user)
}

/// Creates an account.
pub async fn create(vfs: &Vfs, create: Create) -> Result<User, Error> {
    match call(vfs, WinNetRequest::CreateUser(create)).await? {
        WinNetResponse::User { name, sid } => Ok(User {
            vfs: vfs.clone(),
            sid,
            name,
        }),
        _ => Err(unexpected("CreateUser")),
    }
}

/// Enumerates normal local accounts.
pub fn enumerate(vfs: &Vfs) -> Users {
    Users(Paged::new(vfs))
}

impl User {
    /// The account SID, which survives a rename.
    pub fn sid(&self) -> &Sid {
        &self.sid
    }

    /// Re-resolves the account name after a not-found failure.
    ///
    /// The SID is the stable identity, so a name that no longer resolves the
    /// account usually means it was renamed behind our back.
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

    /// Reads fresh account information.
    pub async fn info(&mut self) -> Result<Info, Error> {
        match self
            .request(|name, sid| WinNetRequest::Info {
                name: name.into(),
                sid: sid.clone(),
            })
            .await?
        {
            WinNetResponse::Info(info) => {
                self.name.clone_from(&info.name);
                Ok(*info)
            }
            _ => Err(unexpected("Info")),
        }
    }

    /// Applies the supplied changes and returns fresh account information.
    pub async fn update(&mut self, update: Update) -> Result<Info, Error> {
        match self
            .request(|name, sid| WinNetRequest::Update {
                name: name.into(),
                sid: sid.clone(),
                update: update.clone(),
            })
            .await?
        {
            WinNetResponse::Info(info) => {
                self.name.clone_from(&info.name);
                Ok(*info)
            }
            _ => Err(unexpected("Update")),
        }
    }

    /// Deletes the account.
    pub async fn delete(mut self) -> Result<(), Error> {
        match self
            .request(|name, sid| WinNetRequest::Delete {
                name: name.into(),
                sid: sid.clone(),
            })
            .await?
        {
            WinNetResponse::Deleted => Ok(()),
            _ => Err(unexpected("Delete")),
        }
    }
}

/// A paged forward iterator over normal local users.
pub struct Users(Paged<Info>);

impl Users {
    /// Yields the next account.
    pub async fn next_entry(&mut self) -> Result<Option<Info>, Error> {
        self.0
            .next_entry(
                |resume| WinNetRequest::UsersPage { resume },
                |response| match response {
                    WinNetResponse::UsersPage {
                        users,
                        resume,
                        done,
                    } => Some((users, resume, done)),
                    _ => None,
                },
                "user enumeration",
            )
            .await
    }
}
