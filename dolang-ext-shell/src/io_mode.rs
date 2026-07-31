use std::io;

use dolang::runtime::{Result, Strand, Value, value::View};
use dolang_shell_vfs::OperatingSystem;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IoMode {
    Line,
    Chunk,
}

pub(crate) enum ReadValue {
    Line(String),
    Chunk(Vec<u8>),
}

#[derive(Clone, Copy)]
pub(crate) enum ValueEncoding {
    Display,
    Argument,
}

pub(crate) async fn read_value<R>(reader: &mut R, mode: IoMode) -> io::Result<Option<ReadValue>>
where
    R: AsyncBufRead + Unpin,
{
    match mode {
        IoMode::Line => {
            let mut line = String::new();
            if reader.read_line(&mut line).await? == 0 {
                Ok(None)
            } else {
                line.truncate(strip_line_ending(&line).len());
                Ok(Some(ReadValue::Line(line)))
            }
        }
        IoMode::Chunk => {
            let mut chunk = vec![0; 8192];
            let len = reader.read(&mut chunk).await?;
            if len == 0 {
                Ok(None)
            } else {
                chunk.truncate(len);
                Ok(Some(ReadValue::Chunk(chunk)))
            }
        }
    }
}

pub(crate) fn encode_value<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    mode: IoMode,
    encoding: ValueEncoding,
    operating_system: OperatingSystem,
) -> Result<'v, 's, Vec<u8>> {
    let (bytes, verbatim) = match value.view(strand) {
        View::Str(value) => (value.pin().as_bytes().to_vec(), false),
        View::Bin(value) => (value.pin().to_vec(), true),
        _ => {
            let value = match encoding {
                ValueEncoding::Display => value.to_string(strand)?,
                ValueEncoding::Argument => value.to_arg(strand)?,
            };
            (value.into_bytes(), false)
        }
    };
    Ok(frame_value(bytes, mode, verbatim, operating_system))
}

fn frame_value(
    mut bytes: Vec<u8>,
    mode: IoMode,
    verbatim: bool,
    operating_system: OperatingSystem,
) -> Vec<u8> {
    if mode == IoMode::Line && !verbatim {
        bytes.extend_from_slice(line_ending(operating_system));
    }
    bytes
}

pub(crate) fn line_ending(operating_system: OperatingSystem) -> &'static [u8] {
    match operating_system {
        OperatingSystem::Windows => b"\r\n",
        OperatingSystem::FreeBsd | OperatingSystem::Linux | OperatingSystem::Macos => b"\n",
    }
}

pub(crate) fn strip_line_ending(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use tokio::io::BufReader;

    use super::*;

    #[tokio::test]
    async fn reader_honors_line_and_chunk_modes() {
        let mut reader = BufReader::new(&b"first\r\nsecond\n"[..]);
        let Some(ReadValue::Line(first)) = read_value(&mut reader, IoMode::Line).await.unwrap()
        else {
            panic!("expected line");
        };
        assert_eq!(first, "first");
        let Some(ReadValue::Line(second)) = read_value(&mut reader, IoMode::Line).await.unwrap()
        else {
            panic!("expected line");
        };
        assert_eq!(second, "second");
        assert!(
            read_value(&mut reader, IoMode::Line)
                .await
                .unwrap()
                .is_none()
        );

        let mut reader = BufReader::new(&b"\x00\xffraw"[..]);
        let Some(ReadValue::Chunk(chunk)) = read_value(&mut reader, IoMode::Chunk).await.unwrap()
        else {
            panic!("expected chunk");
        };
        assert_eq!(chunk, b"\x00\xffraw");
    }

    #[tokio::test]
    async fn line_reader_rejects_invalid_utf8() {
        let mut reader = BufReader::new(&b"\xff\n"[..]);
        assert!(read_value(&mut reader, IoMode::Line).await.is_err());
    }

    #[test]
    fn framing_uses_target_line_endings_for_non_binary_values() {
        assert_eq!(
            frame_value(
                b"text".to_vec(),
                IoMode::Line,
                false,
                OperatingSystem::Linux
            ),
            b"text\n"
        );
        assert_eq!(
            frame_value(
                b"text".to_vec(),
                IoMode::Line,
                false,
                OperatingSystem::Windows
            ),
            b"text\r\n"
        );
        assert_eq!(
            frame_value(
                b"BIN".to_vec(),
                IoMode::Line,
                true,
                OperatingSystem::Windows
            ),
            b"BIN"
        );
        assert_eq!(
            frame_value(
                b"text".to_vec(),
                IoMode::Chunk,
                false,
                OperatingSystem::Windows
            ),
            b"text"
        );
    }

    #[test]
    fn strips_exactly_one_complete_line_ending() {
        assert_eq!(strip_line_ending("text\r\n"), "text");
        assert_eq!(strip_line_ending("text\n"), "text");
        assert_eq!(strip_line_ending("text\r"), "text\r");
        assert_eq!(strip_line_ending("text\n\n"), "text\n");
        assert_eq!(strip_line_ending("text\r\r\n"), "text\r");
    }
}
