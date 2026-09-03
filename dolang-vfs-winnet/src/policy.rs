//! Local password and account-lockout policy.

use dolang_vfs::{Vfs, error::Error};

use crate::{
    api::{call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

pub use crate::wire::{AccountPolicy as Policy, AccountPolicyUpdate as Update};

/// Reads the current policy.
pub async fn get(vfs: &Vfs) -> Result<Policy, Error> {
    match call(vfs, WinNetRequest::AccountPolicy).await? {
        WinNetResponse::AccountPolicy(policy) => Ok(policy),
        _ => Err(unexpected("AccountPolicy")),
    }
}

/// Applies the supplied changes and returns the resulting policy.
pub async fn update(vfs: &Vfs, update: Update) -> Result<Policy, Error> {
    match call(vfs, WinNetRequest::UpdateAccountPolicy(update)).await? {
        WinNetResponse::AccountPolicy(policy) => Ok(policy),
        _ => Err(unexpected("UpdateAccountPolicy")),
    }
}
