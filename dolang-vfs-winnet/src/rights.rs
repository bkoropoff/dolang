//! Account rights held in the local security policy.
//!
//! Rights are assigned to a SID, so they apply uniformly to users and groups.

use dolang_vfs::{Vfs, error::Error};
use dolang_winterop::security::Sid;

use crate::{
    api::{call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

/// Lists the rights assigned to an account.
pub async fn list(vfs: &Vfs, sid: &Sid) -> Result<Vec<String>, Error> {
    match call(vfs, WinNetRequest::AccountRights { sid: sid.clone() }).await? {
        WinNetResponse::AccountRights(rights) => Ok(rights),
        _ => Err(unexpected("AccountRights")),
    }
}

/// Grants a right, such as `SeServiceLogonRight`.
pub async fn grant(vfs: &Vfs, sid: &Sid, right: String) -> Result<(), Error> {
    match call(
        vfs,
        WinNetRequest::GrantAccountRight {
            sid: sid.clone(),
            right,
        },
    )
    .await?
    {
        WinNetResponse::Unit => Ok(()),
        _ => Err(unexpected("GrantAccountRight")),
    }
}

/// Revokes a right. Revoking an unassigned right has no effect.
pub async fn revoke(vfs: &Vfs, sid: &Sid, right: String) -> Result<(), Error> {
    match call(
        vfs,
        WinNetRequest::RevokeAccountRight {
            sid: sid.clone(),
            right,
        },
    )
    .await?
    {
        WinNetResponse::Unit => Ok(()),
        _ => Err(unexpected("RevokeAccountRight")),
    }
}
