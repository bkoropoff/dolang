use std::{error, fmt, str::FromStr};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, SeqAccess, Visitor},
    ser::SerializeSeq,
};

const REVISION: u8 = SidRevision::One as u8;
const MIN_SUB_AUTHORITIES: usize = 1;
const MAX_SUB_AUTHORITIES: usize = 15;
const IDENTIFIER_AUTHORITY_MAX: u64 = (1 << 48) - 1;

/// A Windows security identifier (SID).
///
/// The text form is canonical `S-1-...` notation and the binary helpers use
/// the native Windows packet layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sid {
    identifier_authority: SidIdentifierAuthority,
    sub_authorities: Box<[u32]>,
}

impl Sid {
    /// Creates a revision-1 SID from its identifier authority and sub-authorities.
    ///
    /// The authority must fit in 48 bits and a SID must have one through 15
    /// sub-authorities.
    pub fn new(
        identifier_authority: impl Into<SidIdentifierAuthority>,
        sub_authorities: impl IntoIterator<Item = u32>,
    ) -> Result<Self, SidError> {
        let identifier_authority = identifier_authority.into();
        let identifier_authority_value = u64::from(identifier_authority);
        if identifier_authority_value > IDENTIFIER_AUTHORITY_MAX {
            return Err(SidError::IdentifierAuthority);
        }
        let sub_authorities = sub_authorities.into_iter().collect::<Box<[_]>>();
        if !(MIN_SUB_AUTHORITIES..=MAX_SUB_AUTHORITIES).contains(&sub_authorities.len()) {
            return Err(SidError::SubAuthorityCount(sub_authorities.len()));
        }
        Ok(Self {
            identifier_authority,
            sub_authorities,
        })
    }

    /// Returns the SID revision.
    pub const fn revision(&self) -> SidRevision {
        SidRevision::One
    }

    /// Returns the identifier authority
    pub const fn identifier_authority(&self) -> SidIdentifierAuthority {
        self.identifier_authority
    }

    /// Returns the SID sub-authorities.
    pub fn sub_authorities(&self) -> &[u32] {
        &self.sub_authorities
    }

    /// Parses a Windows-native SID packet.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SidError> {
        if bytes.len() < 8 {
            return Err(SidError::PacketLength);
        }
        if bytes[0] != REVISION {
            return Err(SidError::Revision(bytes[0]));
        }
        let count = usize::from(bytes[1]);
        if !(MIN_SUB_AUTHORITIES..=MAX_SUB_AUTHORITIES).contains(&count) {
            return Err(SidError::SubAuthorityCount(count));
        }
        let expected = 8 + count * 4;
        if bytes.len() != expected {
            return Err(SidError::PacketLength);
        }

        let mut identifier_authority_bytes = [0; 8];
        identifier_authority_bytes[2..].copy_from_slice(&bytes[2..8]);
        let identifier_authority = u64::from_be_bytes(identifier_authority_bytes);
        let sub_authorities = bytes[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_le_bytes);
        Self::new(identifier_authority, sub_authorities)
    }

    /// Converts this SID to the Windows-native SID packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + self.sub_authorities.len() * 4);
        bytes.push(REVISION);
        bytes.push(u8::try_from(self.sub_authorities.len()).unwrap());
        bytes.extend_from_slice(&u64::from(self.identifier_authority).to_be_bytes()[2..]);
        for sub_authority in &self.sub_authorities {
            bytes.extend_from_slice(&sub_authority.to_le_bytes());
        }
        bytes
    }

    fn identifier_authority_bytes(&self) -> [u8; 6] {
        u64::from(self.identifier_authority).to_be_bytes()[2..]
            .try_into()
            .unwrap()
    }
}

impl TryFrom<&[u8]> for Sid {
    type Error = SidError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "S-{REVISION}-")?;
        let identifier_authority = u64::from(self.identifier_authority);
        if identifier_authority < (1 << 32) {
            write!(f, "{identifier_authority}")?;
        } else {
            write!(f, "0x{identifier_authority:012X}")?;
        }
        for sub_authority in &self.sub_authorities {
            write!(f, "-{sub_authority}")?;
        }
        Ok(())
    }
}

/// SID packet format revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SidRevision {
    /// Revision 1, the only SID format revision.
    One = 1,
}

impl Serialize for SidRevision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for SidRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u8::deserialize(deserializer)? {
            REVISION => Ok(Self::One),
            revision => Err(de::Error::custom(SidError::Revision(revision))),
        }
    }
}

/// Authority responsible for issuing a SID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SidIdentifierAuthority {
    /// Null authority.
    Null,
    /// World authority.
    World,
    /// Local authority.
    Local,
    /// Creator authority.
    Creator,
    /// Non-unique authority.
    NonUnique,
    /// NT authority.
    Nt,
    /// Resource manager authority.
    ResourceManager,
    /// Application package authority.
    AppPackage,
    /// Mandatory label authority.
    MandatoryLabel,
    /// Scoped policy identifier authority.
    ScopedPolicy,
    /// Authentication authority.
    Authentication,
    /// Process trust authority.
    ProcessTrust,
    /// An authority not known by this version of the crate.
    Unknown(u64),
}

impl From<u64> for SidIdentifierAuthority {
    fn from(value: u64) -> Self {
        match value {
            0 => Self::Null,
            1 => Self::World,
            2 => Self::Local,
            3 => Self::Creator,
            4 => Self::NonUnique,
            5 => Self::Nt,
            9 => Self::ResourceManager,
            15 => Self::AppPackage,
            16 => Self::MandatoryLabel,
            17 => Self::ScopedPolicy,
            18 => Self::Authentication,
            19 => Self::ProcessTrust,
            value => Self::Unknown(value),
        }
    }
}

impl From<SidIdentifierAuthority> for u64 {
    fn from(authority: SidIdentifierAuthority) -> Self {
        match authority {
            SidIdentifierAuthority::Null => 0,
            SidIdentifierAuthority::World => 1,
            SidIdentifierAuthority::Local => 2,
            SidIdentifierAuthority::Creator => 3,
            SidIdentifierAuthority::NonUnique => 4,
            SidIdentifierAuthority::Nt => 5,
            SidIdentifierAuthority::ResourceManager => 9,
            SidIdentifierAuthority::AppPackage => 15,
            SidIdentifierAuthority::MandatoryLabel => 16,
            SidIdentifierAuthority::ScopedPolicy => 17,
            SidIdentifierAuthority::Authentication => 18,
            SidIdentifierAuthority::ProcessTrust => 19,
            SidIdentifierAuthority::Unknown(value) => value,
        }
    }
}

impl Serialize for SidIdentifierAuthority {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64((*self).into())
    }
}

impl<'de> Deserialize<'de> for SidIdentifierAuthority {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(u64::deserialize(deserializer)?.into())
    }
}

impl FromStr for Sid {
    type Err = SidError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split('-');
        if parts.next() != Some("S") || parts.next() != Some("1") {
            return Err(SidError::StringSyntax);
        }
        let authority = parts.next().ok_or(SidError::StringSyntax)?;
        let identifier_authority = if let Some(hex) = authority.strip_prefix("0x") {
            if hex.len() != 12 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(SidError::StringSyntax);
            }
            let authority = u64::from_str_radix(hex, 16).map_err(|_| SidError::StringSyntax)?;
            if authority < (1 << 32) {
                return Err(SidError::StringSyntax);
            }
            authority
        } else {
            if authority.is_empty()
                || (authority.len() > 1 && authority.starts_with('0'))
                || !authority.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(SidError::StringSyntax);
            }
            let authority = authority
                .parse::<u64>()
                .map_err(|_| SidError::IdentifierAuthority)?;
            if authority >= (1 << 32) {
                return Err(SidError::StringSyntax);
            }
            authority
        };

        let sub_authorities = parts
            .map(|part| {
                if part.is_empty()
                    || (part.len() > 1 && part.starts_with('0'))
                    || !part.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(SidError::StringSyntax);
                }
                part.parse::<u32>().map_err(|_| SidError::StringSyntax)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(identifier_authority, sub_authorities)
    }
}

impl Serialize for Sid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(3 + self.sub_authorities.len()))?;
        seq.serialize_element(&REVISION)?;
        seq.serialize_element(&u8::try_from(self.sub_authorities.len()).unwrap())?;
        seq.serialize_element(&self.identifier_authority_bytes())?;
        for sub_authority in &self.sub_authorities {
            seq.serialize_element(sub_authority)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Sid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SidVisitor;

        impl<'de> Visitor<'de> for SidVisitor {
            type Value = Sid;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a structurally encoded Windows SID")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let revision: u8 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                if revision != REVISION {
                    return Err(de::Error::custom(SidError::Revision(revision)));
                }
                let count: u8 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let count = usize::from(count);
                if !(MIN_SUB_AUTHORITIES..=MAX_SUB_AUTHORITIES).contains(&count) {
                    return Err(de::Error::custom(SidError::SubAuthorityCount(count)));
                }
                let authority: [u8; 6] = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;
                let mut authority_bytes = [0; 8];
                authority_bytes[2..].copy_from_slice(&authority);
                let authority = u64::from_be_bytes(authority_bytes);
                let mut sub_authorities = Vec::with_capacity(count);
                for index in 0..count {
                    sub_authorities.push(
                        seq.next_element()?
                            .ok_or_else(|| de::Error::invalid_length(3 + index, &self))?,
                    );
                }
                if seq.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::invalid_length(4 + count, &self));
                }
                Sid::new(authority, sub_authorities).map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_seq(SidVisitor)
    }
}

/// Error returned when constructing or parsing a SID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidError {
    /// The SID uses a revision other than revision 1.
    Revision(u8),
    /// The SID has fewer than one or more than 15 sub-authorities.
    SubAuthorityCount(usize),
    /// The identifier authority exceeds the 48-bit Windows field.
    IdentifierAuthority,
    /// The binary packet length does not match its declared structure.
    PacketLength,
    /// Text was not canonical SID notation.
    StringSyntax,
}

impl fmt::Display for SidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Revision(revision) => write!(f, "unsupported SID revision {revision}"),
            Self::SubAuthorityCount(count) => {
                write!(
                    f,
                    "SID must contain between 1 and 15 sub-authorities, got {count}"
                )
            }
            Self::IdentifierAuthority => f.write_str("SID identifier authority exceeds 48 bits"),
            Self::PacketLength => f.write_str("SID packet length does not match its structure"),
            Self::StringSyntax => f.write_str("invalid canonical SID string"),
        }
    }
}

impl error::Error for SidError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_and_packet_round_trip() {
        let sid: Sid = "S-1-5-21-287454020-2864434397".parse().unwrap();
        let bytes = [
            1, 3, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 0x44, 0x33, 0x22, 0x11, 0xDD, 0xCC, 0xBB, 0xAA,
        ];
        assert_eq!(sid.to_bytes(), bytes);
        assert_eq!(Sid::from_bytes(&bytes).unwrap(), sid);
        assert_eq!(sid.to_string(), "S-1-5-21-287454020-2864434397");
        assert_eq!(Sid::new(5, [32, 544]).unwrap().to_string(), "S-1-5-32-544");
    }

    #[test]
    fn high_identifier_authority_uses_hexadecimal() {
        let sid: Sid = "S-1-0x010203040506-7".parse().unwrap();
        assert_eq!(
            sid.identifier_authority(),
            SidIdentifierAuthority::Unknown(0x0102_0304_0506)
        );
        assert_eq!(sid.to_string(), "S-1-0x010203040506-7");
        assert_eq!(&sid.to_bytes()[2..8], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn rejects_noncanonical_or_malformed_values() {
        assert!("S-1-5".parse::<Sid>().is_err());
        assert!("S-1-05-1".parse::<Sid>().is_err());
        assert!("S-1-5-01".parse::<Sid>().is_err());
        assert!("S-1-0x000000000005-1".parse::<Sid>().is_err());
        assert!(Sid::from_bytes(&[2, 1, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0]).is_err());
        assert!(Sid::from_bytes(&[1, 1, 0, 0, 0, 0, 0, 5]).is_err());
    }

    #[test]
    fn serde_is_structural() {
        let sid: Sid = "S-1-5-32-544".parse().unwrap();
        let encoded = postcard::to_stdvec(&sid).unwrap();
        let expected =
            postcard::to_stdvec(&(1u8, 2u8, [0u8, 0, 0, 0, 0, 5], 32u32, 544u32)).unwrap();
        assert_eq!(encoded, [vec![5], expected].concat());
        assert_eq!(postcard::from_bytes::<Sid>(&encoded).unwrap(), sid);
    }

    #[test]
    fn revisions_and_authorities_use_native_values() {
        assert_eq!(
            Sid::new(SidIdentifierAuthority::Nt, [32])
                .unwrap()
                .revision(),
            SidRevision::One
        );
        assert_eq!(SidIdentifierAuthority::from(5), SidIdentifierAuthority::Nt);
        assert_eq!(
            SidIdentifierAuthority::from(42),
            SidIdentifierAuthority::Unknown(42)
        );
        assert_eq!(postcard::to_stdvec(&SidRevision::One).unwrap(), vec![1]);
        assert!(postcard::from_bytes::<SidRevision>(&[2]).is_err());
        assert_eq!(
            postcard::to_stdvec(&SidIdentifierAuthority::Nt).unwrap(),
            postcard::to_stdvec(&5u64).unwrap()
        );
    }
}
