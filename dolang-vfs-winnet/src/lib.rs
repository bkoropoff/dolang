#![deny(warnings)]
//! Remoteable Windows NetAPI bindings.

mod api;
mod backend;
mod wire;

pub use api::{Group, GroupMembers, Groups, User, Users, account_policy, update_account_policy};
pub use wire::{
    AccountPolicy, AccountPolicyUpdate, GroupCreate, GroupInfo, GroupUpdate, UserCreate, UserFlags,
    UserInfo, UserUpdate,
};
