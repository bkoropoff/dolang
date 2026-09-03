#![deny(warnings)]
//! Remoteable Windows NetAPI bindings.
//!
//! Each management domain is a module: [`user`], [`group`], [`share`],
//! [`policy`] and [`rights`]. A domain module holds its capability types
//! alongside the free functions that obtain them from a
//! [`Vfs`](dolang_vfs::Vfs), so `share::enumerate(&vfs)` and
//! `user::by_name(&vfs, name)` read the same way.

mod api;
mod backend;
pub mod group;
pub mod policy;
pub mod rights;
pub mod share;
pub mod user;
mod wire;
