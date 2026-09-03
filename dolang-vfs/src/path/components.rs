//! Component iteration and Windows prefix classification.

use std::fmt;

use typed_path::{Utf8TypedComponent, Utf8TypedComponents, Utf8WindowsPrefix};

use super::Path;

/// One component of a [`Path`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Component<'a>(pub(super) Utf8TypedComponent<'a>);

impl<'a> Component<'a> {
    /// Returns the component text.
    pub fn as_str(&self) -> &'a str {
        self.0.as_str()
    }

    /// Returns whether this is a root component.
    pub fn is_root(&self) -> bool {
        self.0.is_root()
    }

    /// Returns whether this is an ordinary named component.
    pub fn is_normal(&self) -> bool {
        self.0.is_normal()
    }

    /// Returns whether this is a parent (`..`) component.
    pub fn is_parent(&self) -> bool {
        self.0.is_parent()
    }

    /// Returns whether this is a current-directory (`.`) component.
    pub fn is_current(&self) -> bool {
        self.0.is_current()
    }
}

impl fmt::Debug for Component<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Component<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Iterator over the components of a [`Path`].
#[derive(Clone)]
pub struct Components<'a>(pub(super) Utf8TypedComponents<'a>);

impl<'a> Components<'a> {
    /// Returns the remaining components as a path.
    pub fn to_path(&self) -> Path<'a> {
        Path(self.0.to_path())
    }

    /// Returns the remaining components as a string.
    pub fn as_str(&self) -> &'a str {
        self.0.as_str()
    }

    /// Returns whether the path being iterated is absolute.
    pub fn is_absolute(&self) -> bool {
        self.0.is_absolute()
    }

    /// Returns whether the path being iterated has a root.
    pub fn has_root(&self) -> bool {
        self.0.has_root()
    }
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(Component)
    }
}

impl DoubleEndedIterator for Components<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(Component)
    }
}

impl std::iter::FusedIterator for Components<'_> {}

impl fmt::Debug for Components<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

/// Leading prefix of a Windows path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum WindowsPrefix<'a> {
    /// Verbatim prefix, e.g. `\\?\cat_pics`.
    Verbatim(&'a str),
    /// Verbatim UNC prefix, e.g. `\\?\UNC\server\share`.
    VerbatimUNC(&'a str, &'a str),
    /// Verbatim disk prefix, e.g. `\\?\C:`.
    VerbatimDisk(char),
    /// Device namespace prefix, e.g. `\\.\COM42`.
    DeviceNS(&'a str),
    /// UNC prefix, e.g. `\\server\share`.
    UNC(&'a str, &'a str),
    /// Disk prefix, e.g. `C:`.
    Disk(char),
}

impl WindowsPrefix<'_> {
    /// Returns whether this prefix uses verbatim (`\\?\`) syntax.
    pub fn is_verbatim(&self) -> bool {
        matches!(
            self,
            Self::Verbatim(_) | Self::VerbatimUNC(..) | Self::VerbatimDisk(_)
        )
    }
}

impl<'a> From<Utf8WindowsPrefix<'a>> for WindowsPrefix<'a> {
    fn from(prefix: Utf8WindowsPrefix<'a>) -> Self {
        match prefix {
            Utf8WindowsPrefix::Verbatim(name) => Self::Verbatim(name),
            Utf8WindowsPrefix::VerbatimUNC(server, share) => Self::VerbatimUNC(server, share),
            Utf8WindowsPrefix::VerbatimDisk(disk) => Self::VerbatimDisk(disk),
            Utf8WindowsPrefix::DeviceNS(name) => Self::DeviceNS(name),
            Utf8WindowsPrefix::UNC(server, share) => Self::UNC(server, share),
            Utf8WindowsPrefix::Disk(disk) => Self::Disk(disk),
        }
    }
}
