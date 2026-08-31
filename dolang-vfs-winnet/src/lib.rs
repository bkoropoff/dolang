#![deny(warnings)]
//! Remoteable Windows NetAPI bindings.

mod api;
mod backend;
mod wire;

pub use api::{Group, GroupMembers, Groups, User, Users};
pub use wire::{GroupCreate, GroupInfo, GroupUpdate, UserCreate, UserFlags, UserInfo, UserUpdate};
