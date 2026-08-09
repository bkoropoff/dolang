//! Alternate data streams associated with files.

use serde::{Deserialize, Serialize};

/// Describes one alternate data stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamEntry {
    /// Stream name.
    pub name: String,
    /// Stream type reported by the target.
    pub r#type: String,
    /// Logical stream length in bytes.
    pub size: u64,
    /// Allocated stream size in bytes.
    pub alloc_size: u64,
}
