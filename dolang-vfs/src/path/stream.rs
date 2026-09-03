//! Windows alternate data stream specifiers.
//!
//! NTFS names a stream with two parts: a name and a type. They reach this crate
//! spelled two different ways, and the two grammars are not interchangeable:
//!
//! - As a suffix on the final component of a path the caller wrote, such as
//!   `file.txt:zone` or `file.txt:zone:$DATA`. The type is optional and, when
//!   present, carries a leading `$`.
//! - As the raw name reported by an enumeration, such as `:zone:$DATA`. Both
//!   the leading `:` and the type are mandatory.
//!
//! Only the first grammar is part of the public API; the raw form is parsed on
//! the way in and never handed back out.

use crate::error::{Error, ErrorKind, Result};

/// A borrowed alternate data stream specifier.
///
/// The type is stored without its leading `$`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamSpec<'a> {
    name: &'a str,
    stream_type: Option<&'a str>,
}

impl<'a> StreamSpec<'a> {
    /// Creates a specifier from a stream name and optional type.
    pub const fn new(name: &'a str, stream_type: Option<&'a str>) -> Self {
        Self { name, stream_type }
    }

    /// Returns the stream name.
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the stream type, without its leading `$`.
    pub const fn stream_type(&self) -> Option<&'a str> {
        self.stream_type
    }

    /// Converts this specifier into an owned one.
    pub fn to_spec_buf(&self) -> StreamSpecBuf {
        StreamSpecBuf {
            name: self.name.to_owned(),
            stream_type: self.stream_type.map(str::to_owned),
        }
    }
}

/// An owned alternate data stream specifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamSpecBuf {
    name: String,
    stream_type: Option<String>,
}

impl StreamSpecBuf {
    /// Creates a specifier from a stream name and optional type.
    pub fn new(name: impl Into<String>, stream_type: Option<impl Into<String>>) -> Self {
        Self {
            name: name.into(),
            stream_type: stream_type.map(Into::into),
        }
    }

    /// Returns the stream name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the stream type, without its leading `$`.
    pub fn stream_type(&self) -> Option<&str> {
        self.stream_type.as_deref()
    }

    /// Borrows this specifier.
    pub fn to_spec(&self) -> StreamSpec<'_> {
        StreamSpec {
            name: &self.name,
            stream_type: self.stream_type.as_deref(),
        }
    }
}

impl<'a> From<StreamSpec<'a>> for StreamSpecBuf {
    fn from(spec: StreamSpec<'a>) -> Self {
        spec.to_spec_buf()
    }
}

impl From<&crate::file::StreamEntry> for StreamSpecBuf {
    fn from(entry: &crate::file::StreamEntry) -> Self {
        Self::new(entry.name(), Some(entry.stream_type()))
    }
}

/// Splits a path's final component into its base name and stream specifier.
///
/// # Errors
///
/// Fails if the component has a stream suffix that does not follow the
/// `name:stream[:$TYPE]` grammar.
pub(super) fn split_suffix(component: &str) -> Result<(&str, Option<StreamSpec<'_>>)> {
    let mut parts = component.split(':');
    let base = parts.next().expect("split always yields one part");
    let Some(name) = parts.next() else {
        return Ok((base, None));
    };
    let stream_type = parts.next();
    if parts.next().is_some() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "path final component has too many alternate data stream parts",
        ));
    }
    let stream_type = stream_type
        .map(|stream_type| {
            stream_type.strip_prefix('$').ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidInput,
                    "explicit alternate data stream type must start with `$`",
                )
            })
        })
        .transpose()?;
    Ok((base, Some(StreamSpec { name, stream_type })))
}

/// Renders a base name with `spec` appended as a suffix.
pub(super) fn join_suffix(base: &str, spec: Option<StreamSpec<'_>>) -> String {
    let mut name = base.to_owned();
    if let Some(spec) = spec {
        name.push(':');
        name.push_str(spec.name);
        if let Some(stream_type) = spec.stream_type {
            name.push_str(":$");
            name.push_str(stream_type);
        }
    }
    name
}

/// Parses a raw NTFS stream name of the form `:name:$TYPE`.
///
/// # Errors
///
/// Fails if either mandatory part is missing.
// Only the Windows direct backend enumerates raw stream names, but the grammar
// belongs with its sibling above, and its tests are worth running everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_raw_name(raw: &str) -> Result<(String, String)> {
    let rest = raw
        .strip_prefix(':')
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "stream name missing `:` prefix"))?;
    let split = rest
        .rfind(':')
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "stream name missing type suffix"))?;
    let stream_type = rest[split + 1..]
        .strip_prefix('$')
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "stream type missing `$` prefix"))?;
    Ok((rest[..split].to_owned(), stream_type.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{join_suffix, parse_raw_name, split_suffix};

    #[test]
    fn suffix_grammar_accepts_optional_type() {
        let (base, spec) = split_suffix("file.txt").unwrap();
        assert_eq!(base, "file.txt");
        assert!(spec.is_none());

        let (base, spec) = split_suffix("file.txt:zone").unwrap();
        let spec = spec.unwrap();
        assert_eq!(base, "file.txt");
        assert_eq!(spec.name(), "zone");
        assert_eq!(spec.stream_type(), None);

        let (base, spec) = split_suffix("file.txt:zone:$DATA").unwrap();
        let spec = spec.unwrap();
        assert_eq!(base, "file.txt");
        assert_eq!(spec.name(), "zone");
        assert_eq!(spec.stream_type(), Some("DATA"));

        let (_, spec) = split_suffix("file.txt::$DATA").unwrap();
        assert_eq!(spec.unwrap().name(), "");
    }

    #[test]
    fn suffix_grammar_rejects_malformed_types_and_extra_parts() {
        assert!(split_suffix("file.txt:zone:DATA").is_err());
        assert!(split_suffix("file.txt:a:b:c").is_err());
    }

    #[test]
    fn join_suffix_round_trips_split_suffix() {
        for component in ["file.txt", "file.txt:zone", "file.txt:zone:$DATA"] {
            let (base, spec) = split_suffix(component).unwrap();
            assert_eq!(join_suffix(base, spec), component);
        }
    }

    #[test]
    fn raw_names_require_both_delimiters() {
        assert_eq!(
            parse_raw_name(":zone:$DATA").unwrap(),
            ("zone".to_owned(), "DATA".to_owned())
        );
        assert_eq!(
            parse_raw_name("::$DATA").unwrap(),
            (String::new(), "DATA".to_owned())
        );
        assert!(parse_raw_name("zone:$DATA").is_err());
        assert!(parse_raw_name(":zone").is_err());
        assert!(parse_raw_name(":zone:DATA").is_err());
    }
}
