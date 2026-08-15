//! Windows globally unique identifiers (GUIDs).

use std::{error, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A Windows globally unique identifier (GUID).
///
/// Parse and format it using the canonical hyphenated form:
///
/// ```
/// use dolang_winterop::guid::Guid;
///
/// let guid: Guid = "00112233-4455-6677-8899-aabbccddeeff".parse()?;
/// assert_eq!(guid.to_string(), "00112233-4455-6677-8899-aabbccddeeff");
/// # Ok::<(), dolang_winterop::guid::Error>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Guid([u8; 16]);

impl Guid {
    /// Constructs a GUID from the fields of the Windows `GUID` structure.
    pub const fn from_components(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        let data1 = data1.to_le_bytes();
        let data2 = data2.to_le_bytes();
        let data3 = data3.to_le_bytes();
        Self([
            data1[0], data1[1], data1[2], data1[3], data2[0], data2[1], data3[0], data3[1],
            data4[0], data4[1], data4[2], data4[3], data4[4], data4[5], data4[6], data4[7],
        ])
    }

    /// Generates a random version 4 GUID.
    pub fn new_v4() -> Self {
        let mut bytes = [0; 16];
        rand::fill(&mut bytes);
        bytes[7] = (bytes[7] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Self(bytes)
    }

    /// Parses the 16-byte in-memory layout used by Windows APIs.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let bytes = bytes.try_into().map_err(|_| Error::PacketLength)?;
        Ok(Self(bytes))
    }

    /// Returns the fields of the Windows `GUID` structure.
    pub const fn components(self) -> (u32, u16, u16, [u8; 8]) {
        (
            u32::from_le_bytes([self.0[0], self.0[1], self.0[2], self.0[3]]),
            u16::from_le_bytes([self.0[4], self.0[5]]),
            u16::from_le_bytes([self.0[6], self.0[7]]),
            [
                self.0[8], self.0[9], self.0[10], self.0[11], self.0[12], self.0[13], self.0[14],
                self.0[15],
            ],
        )
    }

    /// Returns the native 16-byte Windows GUID representation.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Returns the native 16-byte Windows GUID representation.
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl TryFrom<&[u8]> for Guid {
    type Error = Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data1 = u32::from_le_bytes(self.0[0..4].try_into().unwrap());
        let data2 = u16::from_le_bytes(self.0[4..6].try_into().unwrap());
        let data3 = u16::from_le_bytes(self.0[6..8].try_into().unwrap());
        write!(
            f,
            "{data1:08x}-{data2:04x}-{data3:04x}-{:02x}{:02x}-",
            self.0[8], self.0[9]
        )?;
        for byte in &self.0[10..] {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Guid {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 36
            || value.as_bytes()[8] != b'-'
            || value.as_bytes()[13] != b'-'
            || value.as_bytes()[18] != b'-'
            || value.as_bytes()[23] != b'-'
        {
            return Err(Error::StringSyntax);
        }
        let parse = |start, end| {
            u64::from_str_radix(&value[start..end], 16).map_err(|_| Error::StringSyntax)
        };
        let data1 = u32::try_from(parse(0, 8)?).unwrap();
        let data2 = u16::try_from(parse(9, 13)?).unwrap();
        let data3 = u16::try_from(parse(14, 18)?).unwrap();
        let data4a = u16::try_from(parse(19, 23)?).unwrap();
        let data4b = parse(24, 36)?;
        let mut bytes = [0; 16];
        bytes[0..4].copy_from_slice(&data1.to_le_bytes());
        bytes[4..6].copy_from_slice(&data2.to_le_bytes());
        bytes[6..8].copy_from_slice(&data3.to_le_bytes());
        bytes[8..10].copy_from_slice(&data4a.to_be_bytes());
        bytes[10..16].copy_from_slice(&data4b.to_be_bytes()[2..]);
        Ok(Self(bytes))
    }
}

impl Serialize for Guid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Guid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes = <[u8; 16]>::deserialize(deserializer)?;
        Guid::from_bytes(&bytes).map_err(de::Error::custom)
    }
}

/// Error returned when parsing a GUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A binary packet was not exactly 16 bytes long.
    PacketLength,
    /// Text was not a canonical hyphenated GUID.
    StringSyntax,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PacketLength => f.write_str("GUID packet must contain exactly 16 bytes"),
            Self::StringSyntax => f.write_str("invalid canonical GUID string"),
        }
    }
}

impl error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_and_native_packet_round_trip() {
        let guid: Guid = "00112233-4455-6677-8899-aabbccddeeff".parse().unwrap();
        assert_eq!(
            guid.components(),
            (
                0x0011_2233,
                0x4455,
                0x6677,
                [0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]
            )
        );
        assert_eq!(
            Guid::from_components(
                0x0011_2233,
                0x4455,
                0x6677,
                [0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]
            ),
            guid
        );
        assert_eq!(
            guid.to_bytes(),
            [
                0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]
        );
        assert_eq!(guid.to_string(), "00112233-4455-6677-8899-aabbccddeeff");
        assert_eq!(
            "00112233-4455-6677-8899-AABBCCDDEEFF"
                .parse::<Guid>()
                .unwrap(),
            guid
        );
        assert_eq!(Guid::from_bytes(guid.as_bytes()).unwrap(), guid);
    }

    #[test]
    fn rejects_noncanonical_text_and_packet_lengths() {
        assert!(
            "{00112233-4455-6677-8899-aabbccddeeff}"
                .parse::<Guid>()
                .is_err()
        );
        assert!("00112233445566778899aabbccddeeff".parse::<Guid>().is_err());
        assert!(Guid::from_bytes(&[0; 15]).is_err());
    }

    #[test]
    fn serde_round_trip() {
        let guid: Guid = "00112233-4455-6677-8899-aabbccddeeff".parse().unwrap();
        let encoded = postcard::to_stdvec(&guid).unwrap();
        assert_eq!(postcard::from_bytes::<Guid>(&encoded).unwrap(), guid);
    }

    #[test]
    fn generated_guid_is_version_4_with_rfc_variant() {
        let guid = Guid::new_v4();
        let (_, _, data3, data4) = guid.components();
        assert_eq!(data3 >> 12, 4);
        assert_eq!(data4[0] >> 6, 2);
    }
}
