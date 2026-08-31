//! Minimal, byte-exact `cat`/`echo`/`grep` replacements for Do language shell
//! tests, so tests don't depend on cmd.exe built-ins (`more`, `findstr`,
//! `type`), which are line-oriented and mangle data in ways that differ
//! between real Windows and Wine.

use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(mode) = args.next() else {
        eprintln!("dolang-private-polyfill: expected a subcommand (cat, echo, grep, sleep)");
        return ExitCode::FAILURE;
    };
    let args: Vec<_> = args.collect();

    let result = match mode.to_str() {
        Some("cat") => cat(&args).map(|()| true),
        Some("echo") => echo(&args).map(|()| true),
        Some("grep") => grep(&args),
        Some("sleep") => sleep(&args).map(|()| true),
        _ => {
            eprintln!("dolang-private-polyfill: unknown subcommand {mode:?}");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("dolang-private-polyfill: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Sleeps for the given number of seconds.
///
/// Exists so a test can start a process that stays alive without depending on
/// a platform shell: `sleep(1)` is POSIX-only, and cmd.exe has no equivalent
/// that behaves the same under Wine.
fn sleep(args: &[OsString]) -> io::Result<()> {
    let seconds: u64 = args
        .first()
        .and_then(|arg| arg.to_str())
        .and_then(|arg| arg.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "sleep: expected seconds"))?;
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    Ok(())
}

/// Copies each file given as an argument to stdout in order, or copies
/// stdin to stdout exactly (no newline normalization, no buffering that
/// would drop a final line without a trailing newline) if no arguments are
/// given.
fn cat(args: &[OsString]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    if args.is_empty() {
        io::copy(&mut io::stdin(), &mut stdout)?;
    } else {
        for path in args {
            let mut file = fs::File::open(path)?;
            io::copy(&mut file, &mut stdout)?;
        }
    }
    Ok(())
}

/// Prints its arguments separated by single spaces, followed by a newline.
fn echo(args: &[OsString]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            stdout.write_all(b" ")?;
        }
        stdout.write_all(arg.to_string_lossy().as_bytes())?;
    }
    stdout.write_all(b"\n")?;
    Ok(())
}

/// Prints lines matching a regex pattern (first argument) from the given
/// files, or from stdin if none are given. Returns `Ok(false)` (mapped to a
/// failure exit code) if no line matched, mirroring standard `grep`.
fn grep(args: &[OsString]) -> io::Result<bool> {
    let mut args = args.iter();
    let pattern = args
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "grep: missing pattern"))?
        .to_string_lossy();
    let regex = regex::bytes::Regex::new(&pattern)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let paths: Vec<_> = args.collect();
    let mut matched = false;
    if paths.is_empty() {
        matched |= grep_reader(&regex, &mut io::stdin(), &mut stdout)?;
    } else {
        for path in paths {
            let mut file = fs::File::open(path)?;
            matched |= grep_reader(&regex, &mut file, &mut stdout)?;
        }
    }
    Ok(matched)
}

/// Reads `reader` fully, writes every line matching `regex` to `writer`
/// terminated by `\n`, and returns whether any line matched.
fn grep_reader(
    regex: &regex::bytes::Regex,
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> io::Result<bool> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    // Drop a single trailing newline so a fully-terminated final line
    // doesn't produce a spurious empty line after it.
    if data.last() == Some(&b'\n') {
        data.pop();
    }
    let mut matched = false;
    for line in data.split(|&byte| byte == b'\n') {
        if regex.is_match(line) {
            matched = true;
            writer.write_all(line)?;
            writer.write_all(b"\n")?;
        }
    }
    Ok(matched)
}
