//! Local SMB share management.

use dolang_vfs::{Vfs, error::Error};

use crate::{
    api::{Paged, call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

pub use crate::wire::{
    ShareCreate as Create, ShareInfo as Info, ShareKind as Kind, ShareUpdate as Update,
};

/// A local SMB share.
pub struct Share {
    vfs: Vfs,
    name: String,
}

/// Returns a capability for an already-enumerated share.
pub fn from_info(vfs: &Vfs, info: &Info) -> Share {
    Share {
        vfs: vfs.clone(),
        name: info.name().into(),
    }
}

/// Looks up an existing share by name.
pub async fn by_name(vfs: &Vfs, name: &str) -> Result<Share, Error> {
    match call(vfs, WinNetRequest::ShareInfo { name: name.into() }).await? {
        WinNetResponse::ShareInfo(_) => Ok(Share {
            vfs: vfs.clone(),
            name: name.into(),
        }),
        _ => Err(unexpected("ShareInfo")),
    }
}

/// Creates a share.
pub async fn create(vfs: &Vfs, create: Create) -> Result<Share, Error> {
    match call(vfs, WinNetRequest::CreateShare(create)).await? {
        WinNetResponse::ShareInfo(info) => Ok(from_info(vfs, &info)),
        _ => Err(unexpected("CreateShare")),
    }
}

/// Enumerates every local share, including administrative and non-disk shares.
pub fn enumerate(vfs: &Vfs) -> Shares {
    Shares(Paged::new(vfs))
}

impl Share {
    /// The share name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reads fresh share information.
    pub async fn info(&self) -> Result<Info, Error> {
        match call(
            &self.vfs,
            WinNetRequest::ShareInfo {
                name: self.name.clone(),
            },
        )
        .await?
        {
            WinNetResponse::ShareInfo(info) => Ok(*info),
            _ => Err(unexpected("ShareInfo")),
        }
    }

    /// Applies the supplied changes and returns fresh share information.
    pub async fn update(&self, update: Update) -> Result<Info, Error> {
        match call(
            &self.vfs,
            WinNetRequest::UpdateShare {
                name: self.name.clone(),
                update,
            },
        )
        .await?
        {
            WinNetResponse::ShareInfo(info) => Ok(*info),
            _ => Err(unexpected("UpdateShare")),
        }
    }

    /// Deletes the share.
    pub async fn delete(&self) -> Result<(), Error> {
        match call(
            &self.vfs,
            WinNetRequest::DeleteShare {
                name: self.name.clone(),
            },
        )
        .await?
        {
            WinNetResponse::Deleted => Ok(()),
            _ => Err(unexpected("DeleteShare")),
        }
    }
}

/// A paged forward iterator over local shares.
pub struct Shares(Paged<Info>);

impl Shares {
    /// Yields the next share.
    pub async fn next_entry(&mut self) -> Result<Option<Info>, Error> {
        self.0
            .next_entry(
                |resume| WinNetRequest::SharesPage { resume },
                |response| match response {
                    WinNetResponse::SharesPage {
                        shares,
                        resume,
                        done,
                    } => Some((shares, resume, done)),
                    _ => None,
                },
                "share enumeration",
            )
            .await
    }
}
