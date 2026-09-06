//! Domain join state, membership changes and offline join provisioning.
//!
//! Every mutation here takes effect only after the affected Windows
//! installation is restarted; none of them report that as a runtime condition
//! because none of them can fail to require it.

use dolang_vfs::{Vfs, error::Error};

use crate::{
    api::{call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

pub use crate::wire::{
    JoinInfo as Status, JoinKind as Kind, JoinOptions as Options, JoinRequest as Join,
    OfflineJoinRequest as OfflineJoin, ProvisionOptions, ProvisionRequest as Provision,
    RenameRequest as Rename, UnjoinRequest as Unjoin,
};

/// Reads the current workgroup or domain membership.
pub async fn status(vfs: &Vfs) -> Result<Status, Error> {
    match call(vfs, WinNetRequest::JoinStatus).await? {
        WinNetResponse::JoinInfo(info) => Ok(info),
        _ => Err(unexpected("JoinStatus")),
    }
}

/// Joins a domain. Takes effect on restart.
pub async fn join(vfs: &Vfs, request: Join) -> Result<(), Error> {
    match call(vfs, WinNetRequest::JoinDomain(Box::new(request))).await? {
        WinNetResponse::Unit => Ok(()),
        _ => Err(unexpected("JoinDomain")),
    }
}

/// Leaves the current domain. Takes effect on restart.
pub async fn unjoin(vfs: &Vfs, request: Unjoin) -> Result<(), Error> {
    match call(vfs, WinNetRequest::UnjoinDomain(request)).await? {
        WinNetResponse::Unit => Ok(()),
        _ => Err(unexpected("UnjoinDomain")),
    }
}

/// Renames the machine within its domain. Takes effect on restart.
pub async fn rename(vfs: &Vfs, request: Rename) -> Result<(), Error> {
    match call(vfs, WinNetRequest::RenameMachine(request)).await? {
        WinNetResponse::Unit => Ok(()),
        _ => Err(unexpected("RenameMachine")),
    }
}

/// Creates a computer account and returns the blob that applies it.
///
/// This runs where a domain controller is reachable; the blob is applied
/// elsewhere with [`apply_offline`].
pub async fn provision(vfs: &Vfs, request: Provision) -> Result<Vec<u8>, Error> {
    match call(vfs, WinNetRequest::ProvisionComputer(Box::new(request))).await? {
        WinNetResponse::Blob(blob) => Ok(blob),
        _ => Err(unexpected("ProvisionComputer")),
    }
}

/// Applies a provisioning blob to a Windows installation, joining it to the
/// domain without contacting a domain controller. Takes effect on restart.
pub async fn apply_offline(vfs: &Vfs, request: OfflineJoin) -> Result<(), Error> {
    match call(vfs, WinNetRequest::ApplyOfflineJoin(Box::new(request))).await? {
        WinNetResponse::Unit => Ok(()),
        _ => Err(unexpected("ApplyOfflineJoin")),
    }
}
