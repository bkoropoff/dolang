//! Globally unique identifiers (GUIDs).

use std::{error, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A Windows globally unique identifier (GUID).
///
/// Mirrors the fields of the native `GUID` struct (`Data1`/`Data2`/`Data3`/
/// `Data4`) rather than an opaque byte packet.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Guid {
    /// Generates a random version 4 GUID.
    pub fn new_v4() -> Self {
        let mut bytes = [0u8; 16];
        rand::fill(&mut bytes);
        bytes[7] = (bytes[7] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Self::from_bytes(&bytes).unwrap()
    }

    /// Parses the 16-byte in-memory layout used by Windows APIs.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let bytes: [u8; 16] = bytes.try_into().map_err(|_| Error::PacketLength)?;
        Ok(Self {
            data1: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            data2: u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
            data3: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            data4: bytes[8..16].try_into().unwrap(),
        })
    }

    /// Returns the native 16-byte Windows GUID representation.
    pub const fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        let d1 = self.data1.to_le_bytes();
        let d2 = self.data2.to_le_bytes();
        let d3 = self.data3.to_le_bytes();
        bytes[0] = d1[0];
        bytes[1] = d1[1];
        bytes[2] = d1[2];
        bytes[3] = d1[3];
        bytes[4] = d2[0];
        bytes[5] = d2[1];
        bytes[6] = d3[0];
        bytes[7] = d3[1];
        let mut i = 0;
        while i < 8 {
            bytes[8 + i] = self.data4[i];
            i += 1;
        }
        bytes
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
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-",
            self.data1, self.data2, self.data3, self.data4[0], self.data4[1]
        )?;
        for byte in &self.data4[2..] {
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
        let mut data4 = [0u8; 8];
        data4[0..2].copy_from_slice(&data4a.to_be_bytes());
        data4[2..8].copy_from_slice(&data4b.to_be_bytes()[2..]);
        Ok(Self {
            data1,
            data2,
            data3,
            data4,
        })
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
        assert_eq!(Guid::from_bytes(&guid.to_bytes()).unwrap(), guid);
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
        assert_eq!(guid.data3 >> 12, 4);
        assert_eq!(guid.data4[0] >> 6, 2);
    }
}
