/// Appends one argument using the quoting rules used by Windows command-line
/// parsers and `std::process::Command`.
pub fn quote_windows_argument(argument: &str, output: &mut String) {
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

#[cfg(test)]
mod tests {
    use super::quote_windows_argument;

    fn quote(argument: &str) -> String {
        let mut result = String::new();
        quote_windows_argument(argument, &mut result);
        result
    }

    #[test]
    fn quotes_windows_arguments() {
        assert_eq!(quote(r"C:\plain"), r"C:\plain");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("two words"), r#""two words""#);
        assert_eq!(quote(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(
            quote("C:\\path with space\\"),
            "\"C:\\path with space\\\\\""
        );
    }
}
