//! Windows registry VFS extension for `dolang-vfs`.
//!
//! Registers a remoteable [`dolang_vfs::extension::VfsExtension`] providing
//! typed CRUD access to the Windows registry, dispatched identically
//! whether served in-process or over a real VFS RPC session. See
//! [`Key`] for the public entry point.
//!
//! Its wire codec is linked on every platform so cross-platform clients can
//! communicate with Windows peers. Only Windows backends advertise and
//! dispatch the extension.

mod api;
mod backend;
mod key;
mod value;
mod wire;

pub use api::{Key, SubKeys, Values};
pub use value::Value;
pub use wire::{Access, PredefinedRoot, View};
