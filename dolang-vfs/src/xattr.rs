//! Extended attribute names and directory entries.

use serde::{Deserialize, Serialize};

/// Selects an extended-attribute namespace when listing attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XattrNamespace<'a> {
    /// The target's default namespace.
    Default,
    /// One named target-specific namespace.
    Named(&'a str),
    /// Every namespace supported by the target.
    Any,
}

/// Describes one extended attribute without reading its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XattrEntry {
    /// Attribute name within its namespace.
    pub name: String,
    /// Namespace, when the target reports one separately.
    pub namespace: Option<String>,
    /// Value size, when available without reading it.
    pub size: Option<u64>,
    /// Target-specific attribute flags.
    pub flags: Option<u8>,
}
