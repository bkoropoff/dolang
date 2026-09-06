//! Machine identity as the workstation and server services report it.

use dolang_vfs::{Vfs, error::Error};

use crate::{
    api::{call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

pub use crate::wire::{MachineInfo as Info, ServerType};

/// Reads the computer name, domain membership, OS level and server role.
pub async fn info(vfs: &Vfs) -> Result<Info, Error> {
    match call(vfs, WinNetRequest::MachineInfo).await? {
        WinNetResponse::MachineInfo(info) => Ok(*info),
        _ => Err(unexpected("MachineInfo")),
    }
}
