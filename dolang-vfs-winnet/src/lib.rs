#![deny(warnings)]
//! Remoteable Windows NetAPI bindings.
//!
//! Each management domain is a module: [`user`], [`group`], [`share`],
//! [`connection`], [`policy`], [`rights`], [`domain`] and [`machine`]. A domain
//! module holds its capability types alongside the free functions that obtain
//! them from a [`Vfs`](dolang_vfs::Vfs), so `share::enumerate(&vfs)` and
//! `user::by_name(&vfs, name)` read the same way.
//!
//! [`share`] covers the shares this machine publishes; [`connection`] covers
//! the remote shares it uses.

mod api;
mod backend;
pub mod connection;
pub mod domain;
pub mod group;
pub mod machine;
pub mod policy;
pub mod rights;
pub mod share;
pub mod user;
mod wire;
