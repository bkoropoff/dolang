//! SMB client connection management.

use dolang_vfs::{Vfs, error::Error, path};

use crate::{
    api::{Paged, call, unexpected},
    wire::{WinNetRequest, WinNetResponse},
};

pub use crate::wire::{
    ConnectionCreate as Create, ConnectionInfo as Info, ConnectionKind as Kind,
    ConnectionState as State,
};

/// A connection to a remote SMB resource.
///
/// Connections belong to a logon session, so one made under a given account is
/// not visible to a process running as another.
pub struct Connection {
    vfs: Vfs,
    name: String,
}

/// Returns a capability for an already-enumerated connection.
pub fn from_info(vfs: &Vfs, info: &Info) -> Connection {
    Connection {
        vfs: vfs.clone(),
        name: info.name().into(),
    }
}

/// Looks up an existing connection by local device or remote name.
pub async fn by_name(vfs: &Vfs, name: &str) -> Result<Connection, Error> {
    match call(vfs, WinNetRequest::ConnectionInfo { name: name.into() }).await? {
        WinNetResponse::ConnectionInfo(info) => Ok(from_info(vfs, &info)),
        _ => Err(unexpected("ConnectionInfo")),
    }
}

/// Adds a connection.
pub async fn add(vfs: &Vfs, create: Create) -> Result<Connection, Error> {
    match call(vfs, WinNetRequest::AddConnection(Box::new(create))).await? {
        WinNetResponse::ConnectionInfo(info) => Ok(from_info(vfs, &info)),
        _ => Err(unexpected("AddConnection")),
    }
}

/// Enumerates every connection, including ones saved in the profile that are
/// not currently connected.
pub fn enumerate(vfs: &Vfs) -> Connections {
    Connections(Paged::new(vfs))
}

/// Resolves a path on a redirected device to its UNC form.
pub async fn universal_name(vfs: &Vfs, path: path::Path<'_>) -> Result<String, Error> {
    match call(
        vfs,
        WinNetRequest::UniversalName {
            path: path.to_path_buf(),
        },
    )
    .await?
    {
        WinNetResponse::UniversalName(name) => Ok(name),
        _ => Err(unexpected("UniversalName")),
    }
}

impl Connection {
    /// The local device or remote name this connection is addressed by.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reads fresh connection information.
    pub async fn info(&self) -> Result<Info, Error> {
        match call(
            &self.vfs,
            WinNetRequest::ConnectionInfo {
                name: self.name.clone(),
            },
        )
        .await?
        {
            WinNetResponse::ConnectionInfo(info) => Ok(*info),
            _ => Err(unexpected("ConnectionInfo")),
        }
    }

    /// Disconnects, removing any profile entry.
    ///
    /// `force` closes the connection even with open files or directories on it.
    /// `forget_credentials` removes credentials saved for the target server;
    /// unset removes them only when the connection was persistent, mirroring
    /// when [`Create::save_credentials`] stores them.
    pub async fn disconnect(
        &self,
        force: bool,
        forget_credentials: Option<bool>,
    ) -> Result<(), Error> {
        match call(
            &self.vfs,
            WinNetRequest::CancelConnection {
                name: self.name.clone(),
                force,
                forget_credentials,
            },
        )
        .await?
        {
            WinNetResponse::Deleted => Ok(()),
            _ => Err(unexpected("CancelConnection")),
        }
    }
}

/// A paged forward iterator over connections.
pub struct Connections(Paged<Info>);

impl Connections {
    /// Yields the next connection.
    pub async fn next_entry(&mut self) -> Result<Option<Info>, Error> {
        self.0
            .next_entry(
                |resume| WinNetRequest::ConnectionsPage { resume },
                |response| match response {
                    WinNetResponse::ConnectionsPage {
                        connections,
                        resume,
                        done,
                    } => Some((connections, resume, done)),
                    _ => None,
                },
                "connection enumeration",
            )
            .await
    }
}
