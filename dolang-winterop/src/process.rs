//! Windows process command-line argument encoding and parsing.
//!
//! Windows process creation receives one command-line string, not an argv
//! array. [`join_arguments`] encodes an argv-like input using the convention
//! understood by the MSVC runtime and Rust's standard library; pair it with
//! [`split_arguments`] when parsing that same convention.

use std::{error::Error, fmt};

/// An argument contains a NUL character and cannot be represented in a
/// Windows command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NulError;

impl fmt::Display for NulError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Windows process arguments cannot contain NUL characters")
    }
}

impl Error for NulError {}

/// Appends one argument encoded with the quoting rules used by MSVC-compatible
/// Windows command-line parsers and `std::process::Command`.
///
/// Callers that build a complete command line from untrusted arguments should
/// prefer [`join_arguments`], which rejects NUL characters.
///
/// This low-level helper intentionally does not reject NUL characters because
/// it appends into caller-owned output.
pub fn quote_argument(argument: &str, output: &mut String) {
    let quote = argument.is_empty() || argument.contains([' ', '\t', '"']);
    if !quote {
        output.push_str(argument);
        return;
    }

    output.push('"');
    let mut backslashes = 0;
    for ch in argument.chars() {
        if ch == '\\' {
            backslashes += 1;
        } else if ch == '"' {
            output.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
            output.push(ch);
            backslashes = 0;
        } else {
            output.extend(std::iter::repeat_n('\\', backslashes));
            output.push(ch);
            backslashes = 0;
        }
    }
    output.extend(std::iter::repeat_n('\\', backslashes * 2));
    output.push('"');
}

/// Encodes arguments as one MSVC-compatible Windows command line.
///
/// Returns [`NulError`] when an argument contains a NUL character.
///
/// ```
/// use dolang_winterop::process::{join_arguments, split_arguments};
///
/// let command_line = join_arguments(["tool", "two words", r#"a\"quote"#])?;
/// assert_eq!(split_arguments(&command_line), ["tool", "two words", r#"a\"quote"#]);
/// # Ok::<(), dolang_winterop::process::NulError>(())
/// ```
pub fn join_arguments<I, S>(arguments: I) -> std::result::Result<String, NulError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command_line = String::new();
    for argument in arguments {
        let argument = argument.as_ref();
        if argument.contains('\0') {
            return Err(NulError);
        }
        if !command_line.is_empty() {
            command_line.push(' ');
        }
        quote_argument(argument, &mut command_line);
    }
    Ok(command_line)
}

/// Parses an MSVC-compatible Windows command line into arguments.
///
/// This follows the convention used for process arguments by MSVC and Rust's
/// `std::process`; it does not implement `CommandLineToArgvW`'s special
/// treatment of the executable name.
pub fn split_arguments(command_line: &str) -> Vec<String> {
    let mut chars = command_line.chars().peekable();
    let mut arguments = Vec::new();

    loop {
        while matches!(chars.peek(), Some(' ' | '\t')) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        let mut argument = String::new();
        let mut in_quotes = false;
        loop {
            match chars.peek() {
                None => break,
                Some(' ' | '\t') if !in_quotes => break,
                Some('\\') => {
                    let mut backslashes = 0;
                    while chars.next_if_eq(&'\\').is_some() {
                        backslashes += 1;
                    }
                    if chars.next_if_eq(&'"').is_some() {
                        argument.extend(std::iter::repeat_n('\\', backslashes / 2));
                        let literal_quote =
                            backslashes % 2 == 1 || (in_quotes && chars.next_if_eq(&'"').is_some());
                        if literal_quote {
                            argument.push('"');
                        } else {
                            in_quotes = !in_quotes;
                        }
                    } else {
                        argument.extend(std::iter::repeat_n('\\', backslashes));
                    }
                }
                Some('"') => {
                    chars.next();
                    if in_quotes && chars.next_if_eq(&'"').is_some() {
                        argument.push('"');
                    } else {
                        in_quotes = !in_quotes;
                    }
                }
                Some(_) => argument.push(chars.next().unwrap()),
            }
        }
        arguments.push(argument);
    }

    arguments
}

#[cfg(test)]
mod tests {
    use super::{NulError, join_arguments, quote_argument, split_arguments};

    fn quote(argument: &str) -> String {
        let mut result = String::new();
        quote_argument(argument, &mut result);
        result
    }

    #[test]
    fn quotes_arguments() {
        assert_eq!(quote(r"C:\plain"), r"C:\plain");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("two words"), r#""two words""#);
        assert_eq!(quote("two\twords"), "\"two\twords\"");
        assert_eq!(quote(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(
            quote("C:\\path with space\\"),
            "\"C:\\path with space\\\\\""
        );
    }

    #[test]
    fn splits_arguments() {
        assert_eq!(split_arguments(""), Vec::<String>::new());
        assert_eq!(split_arguments("a\tb"), ["a", "b"]);
        assert_eq!(
            split_arguments(r#""two words" a\\\"b"#),
            ["two words", "a\\\"b"]
        );
        assert_eq!(
            split_arguments(r#""C:\path with space\\""#),
            ["C:\\path with space\\"]
        );
        assert_eq!(split_arguments(r#""a""b""#), ["a\"b"]);
    }

    #[test]
    fn joins_and_splits_round_trip() {
        let arguments = [
            "",
            "plain",
            "two words",
            "two\twords",
            r#"a\"b"#,
            r"trailing\\",
        ];
        let command_line = join_arguments(arguments).unwrap();
        assert_eq!(split_arguments(&command_line), arguments);
    }

    #[test]
    fn join_rejects_nul_arguments() {
        assert_eq!(join_arguments(["a\0b"]), Err(NulError));
    }
}
