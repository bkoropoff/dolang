#[cfg(unix)]
use std::collections::HashMap;
use std::{io, path::Path};

#[cfg(any(windows, target_os = "linux"))]
use dolang_vfs::metadata::{AttrFlags, AttrsPatch};
#[cfg(any(unix, windows))]
use dolang_vfs::process::{ProcessControl, Signal, TerminationPolicy};
use dolang_vfs::{
    Child, Command, FileHandle, OpenOptions, Vfs,
    direct::Direct,
    file::{FileLockBehavior, FileLockMode, FileLockRange, FileLockRequest},
    metadata::{FileType, MetadataPatch},
};
#[cfg(windows)]
use dolang_winterop::security::SecInfo;
use tempfile::tempdir;
use typed_path::{Utf8TypedPath, Utf8UnixPath, Utf8WindowsPath};

fn typed(path: &Path) -> Utf8TypedPath<'_> {
    let path = path.to_str().unwrap();
    if cfg!(windows) {
        Utf8TypedPath::Windows(Utf8WindowsPath::new(path))
    } else {
        Utf8TypedPath::Unix(Utf8UnixPath::new(path))
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn attr_patch(flag: AttrFlags, value: bool) -> AttrsPatch {
    let mut patch = AttrsPatch::default();
    patch.update(flag, Some(value));
    patch
}

fn typed_str(path: &str) -> Utf8TypedPath<'_> {
    if cfg!(windows) {
        Utf8TypedPath::Windows(Utf8WindowsPath::new(path))
    } else {
        Utf8TypedPath::Unix(Utf8UnixPath::new(path))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[tokio::test]
async fn rename_can_refuse_to_replace_destination() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let from = dir.path().join("rename-from");
    let to = dir.path().join("rename-to");
    tokio::fs::write(&from, b"from").await.unwrap();
    tokio::fs::write(&to, b"to").await.unwrap();

    let error = direct
        .rename(typed(&from), typed(&to), false)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(tokio::fs::read(&from).await.unwrap(), b"from");
    assert_eq!(tokio::fs::read(&to).await.unwrap(), b"to");

    tokio::fs::remove_file(&to).await.unwrap();
    direct
        .rename(typed(&from), typed(&to), false)
        .await
        .unwrap();
    assert!(!from.exists());
    assert_eq!(tokio::fs::read(&to).await.unwrap(), b"from");
}

#[cfg(target_os = "freebsd")]
#[tokio::test]
async fn rename_without_replacement_is_unsupported() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let from = dir.path().join("rename-from");
    let to = dir.path().join("rename-to");
    tokio::fs::write(&from, b"from").await.unwrap();

    let error = direct
        .rename(typed(&from), typed(&to), false)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(tokio::fs::read(&from).await.unwrap(), b"from");
    assert!(!to.exists());
}

#[cfg(windows)]
#[tokio::test]
async fn rename_replaces_an_open_destination() {
    use std::{io::Read as _, os::windows::fs::OpenOptionsExt as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    if dolang_winterop::is_wine() {
        return;
    }

    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let from = dir.path().join("rename-from");
    let to = dir.path().join("rename-to");
    tokio::fs::write(&from, b"from").await.unwrap();
    tokio::fs::write(&to, b"to").await.unwrap();
    let mut old_destination = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&to)
        .unwrap();

    direct.rename(typed(&from), typed(&to), true).await.unwrap();
    assert_eq!(tokio::fs::read(&to).await.unwrap(), b"from");
    let mut old_contents = String::new();
    old_destination.read_to_string(&mut old_contents).unwrap();
    assert_eq!(old_contents, "to");
}

#[cfg(unix)]
async fn wait_for_pid(path: &Path) -> libc::pid_t {
    for _ in 0..200 {
        if let Ok(pid) = tokio::fs::read_to_string(path)
            .await
            .and_then(|pid| pid.trim().parse::<libc::pid_t>().map_err(io::Error::other))
        {
            return pid;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("child did not write {}", path.display());
}

#[cfg(unix)]
fn process_exists(pid: libc::pid_t) -> bool {
    (unsafe { libc::kill(pid, 0) }) == 0
}

#[cfg(unix)]
#[tokio::test]
async fn background_termination_signals_the_process_group() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("descendant.pid");
    let script = format!("sleep 60 & echo $! > '{}'; wait", pid_path.display());
    let mut command = direct.command(Utf8TypedPath::Unix(Utf8UnixPath::new("sh")));
    command
        .arg("-c")
        .arg(&script)
        .process_control(ProcessControl::Background)
        .termination_policy(TerminationPolicy {
            signal: Signal::Term,
            grace: std::time::Duration::from_secs(1),
            force: true,
        });
    let child = command.spawn().await.unwrap();
    let descendant = wait_for_pid(&pid_path).await;

    assert!(child.terminate().await.unwrap().is_some());
    for _ in 0..100 {
        if !process_exists(descendant) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("background descendant {descendant} survived group termination");
}

#[cfg(unix)]
#[tokio::test]
async fn force_false_orphans_the_background_process_group() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("group.pid");
    let script = format!(
        "trap '' TERM; echo $$ > '{}'; sleep 60 & wait",
        pid_path.display()
    );
    let mut command = direct.command(Utf8TypedPath::Unix(Utf8UnixPath::new("sh")));
    command
        .arg("-c")
        .arg(&script)
        .process_control(ProcessControl::Background)
        .termination_policy(TerminationPolicy {
            signal: Signal::Term,
            grace: std::time::Duration::from_millis(50),
            force: false,
        });
    let child = command.spawn().await.unwrap();
    let group = wait_for_pid(&pid_path).await;

    assert_eq!(child.terminate().await.unwrap(), None);
    assert!(process_exists(group));
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
}

#[cfg(windows)]
async fn assert_windows_termination(control: ProcessControl) {
    let direct = Direct::new().unwrap();
    let mut command = direct.command(Utf8TypedPath::Windows(Utf8WindowsPath::new("cmd")));
    command
        .arg("/C")
        .arg("ping -n 60 127.0.0.1 >nul")
        .process_control(control)
        .termination_policy(TerminationPolicy {
            signal: Signal::Term,
            grace: std::time::Duration::from_millis(50),
            force: true,
        });
    let child = command.spawn().await.unwrap();
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.terminate())
        .await
        .unwrap()
        .unwrap();
    assert!(status.is_some());
}

#[cfg(windows)]
#[tokio::test]
async fn windows_foreground_process_group_can_be_terminated() {
    assert_windows_termination(ProcessControl::Foreground).await;
}

#[cfg(windows)]
#[tokio::test]
async fn windows_background_job_can_be_terminated() {
    assert_windows_termination(ProcessControl::Background).await;
}

fn lock_request(
    start: u64,
    end: Option<u64>,
    mode: FileLockMode,
    behavior: FileLockBehavior,
) -> FileLockRequest {
    FileLockRequest {
        range: FileLockRange { start, end },
        mode,
        behavior,
    }
}

async fn open_lock_file(direct: &Direct, path: &Path) -> dolang_vfs::direct::DirectFile {
    direct
        .open_options()
        .read(true)
        .write(true)
        .create(true)
        .open(typed(path))
        .await
        .unwrap()
}

#[cfg(not(target_os = "freebsd"))]
#[tokio::test]
async fn byte_range_locks_contend_and_release() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("locks");
    let first = open_lock_file(&direct, &path).await;
    let second = open_lock_file(&direct, &path).await;

    let mut exclusive = first
        .lock(lock_request(
            0,
            Some(10),
            FileLockMode::Exclusive,
            FileLockBehavior::Blocking,
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(
        second
            .lock(lock_request(
                0,
                Some(10),
                FileLockMode::Exclusive,
                FileLockBehavior::Try,
            ))
            .await
            .unwrap()
            .is_none()
    );
    let mut adjacent = second
        .lock(lock_request(
            10,
            Some(20),
            FileLockMode::Exclusive,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap()
        .unwrap();
    adjacent.release().await.unwrap();
    exclusive.release().await.unwrap();
    assert!(
        second
            .lock(lock_request(
                0,
                Some(10),
                FileLockMode::Exclusive,
                FileLockBehavior::Try,
            ))
            .await
            .unwrap()
            .is_some()
    );
}

// Whole-file locks work everywhere on Unix, FreeBSD's `flock` included, and the
// child inherits this very descriptor there, so it would share the lock on
// every one of these platforms if the handoff did not lift it first.
#[cfg(unix)]
#[tokio::test]
async fn handing_a_file_to_a_child_releases_its_locks() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("closed-locks");
    let first = open_lock_file(&direct, &path).await;
    let second = open_lock_file(&direct, &path).await;

    let mut held = first
        .lock(lock_request(
            0,
            None,
            FileLockMode::Exclusive,
            FileLockBehavior::Blocking,
        ))
        .await
        .unwrap()
        .unwrap();
    // The description outlives this handle in the child, so closing our end
    // would not lift the lock; the handoff has to unlock in band, exactly as
    // `close` does.
    let handed_over = first.into_stdio_send(0).await.unwrap();

    assert!(
        second
            .lock(lock_request(
                0,
                None,
                FileLockMode::Exclusive,
                FileLockBehavior::Try,
            ))
            .await
            .unwrap()
            .is_some()
    );
    // Already released above; releasing again must be a no-op rather than an
    // error, since the script never asked for the lock to come off.
    held.release().await.unwrap();
    drop(handed_over);
}

#[cfg(target_os = "freebsd")]
#[tokio::test]
async fn byte_range_locks_are_rejected() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("byte-range-locks");
    let file = open_lock_file(&direct, &path).await;

    for (start, end) in [(0, Some(10)), (1, None)] {
        let error = file
            .lock(lock_request(
                start,
                end,
                FileLockMode::Exclusive,
                FileLockBehavior::Try,
            ))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    }
}

#[tokio::test]
async fn shared_locks_and_same_handle_overlap_rules() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("shared-locks");
    let first = open_lock_file(&direct, &path).await;
    let second = open_lock_file(&direct, &path).await;
    let third = open_lock_file(&direct, &path).await;

    let _first_shared = first
        .lock(lock_request(
            0,
            None,
            FileLockMode::Shared,
            FileLockBehavior::Blocking,
        ))
        .await
        .unwrap()
        .unwrap();
    let _second_shared = second
        .lock(lock_request(
            0,
            None,
            FileLockMode::Shared,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(
        third
            .lock(lock_request(
                0,
                None,
                FileLockMode::Exclusive,
                FileLockBehavior::Try,
            ))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        first
            .lock(lock_request(
                0,
                None,
                FileLockMode::Shared,
                FileLockBehavior::Try,
            ))
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn finite_empty_range_is_rejected() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("empty-lock");
    let first = open_lock_file(&direct, &path).await;

    let error = first
        .lock(lock_request(
            4,
            Some(4),
            FileLockMode::Exclusive,
            FileLockBehavior::Blocking,
        ))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[cfg(windows)]
#[tokio::test]
async fn finite_empty_range_uses_native_windows_behavior() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("empty-lock");
    let first = open_lock_file(&direct, &path).await;
    let second = open_lock_file(&direct, &path).await;

    let error = first
        .lock(lock_request(
            0,
            Some(0),
            FileLockMode::Exclusive,
            FileLockBehavior::Blocking,
        ))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

    let _empty = first
        .lock(lock_request(
            4,
            Some(4),
            FileLockMode::Exclusive,
            FileLockBehavior::Blocking,
        ))
        .await
        .unwrap()
        .unwrap();

    let mut same = first
        .lock(lock_request(
            4,
            Some(4),
            FileLockMode::Exclusive,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap()
        .unwrap();
    same.release().await.unwrap();

    assert!(
        second
            .lock(lock_request(
                0,
                None,
                FileLockMode::Exclusive,
                FileLockBehavior::Try,
            ))
            .await
            .unwrap()
            .is_none()
    );

    let mut same = second
        .lock(lock_request(
            4,
            Some(4),
            FileLockMode::Exclusive,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap()
        .unwrap();
    same.release().await.unwrap();

    let mut ending_at_offset = second
        .lock(lock_request(
            0,
            Some(4),
            FileLockMode::Exclusive,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap()
        .unwrap();
    ending_at_offset.release().await.unwrap();

    let mut starting_at_offset = second
        .lock(lock_request(
            4,
            Some(8),
            FileLockMode::Exclusive,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap()
        .unwrap();
    starting_at_offset.release().await.unwrap();

    let mut open_ended_at_offset = second
        .lock(lock_request(
            4,
            None,
            FileLockMode::Exclusive,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap()
        .unwrap();
    open_ended_at_offset.release().await.unwrap();

    let error = second
        .lock(lock_request(
            u64::MAX,
            None,
            FileLockMode::Exclusive,
            FileLockBehavior::Try,
        ))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

    assert!(
        second
            .lock(lock_request(
                3,
                Some(5),
                FileLockMode::Exclusive,
                FileLockBehavior::Try,
            ))
            .await
            .unwrap()
            .is_none()
    );
}

#[cfg(unix)]
fn failing_exit_command() -> (&'static str, [&'static str; 2]) {
    ("sh", ["-c", "exit 42"])
}

#[cfg(windows)]
fn failing_exit_command() -> (&'static str, [&'static str; 2]) {
    ("cmd", ["/C", "exit 42"])
}

#[cfg(unix)]
fn env_forwarding_command() -> (&'static str, [&'static str; 2]) {
    ("sh", ["-c", r#"test "$TEST_VAR" = value"#])
}

#[cfg(unix)]
fn successful_command() -> (&'static str, [&'static str; 2]) {
    ("sh", ["-c", "exit 0"])
}

#[cfg(windows)]
fn env_forwarding_command() -> (&'static str, [&'static str; 2]) {
    (
        "cmd",
        ["/C", r#"if "%TEST_VAR%"=="value" exit 0 else exit 1"#],
    )
}

#[cfg(windows)]
fn successful_command() -> (&'static str, [&'static str; 2]) {
    ("cmd", ["/C", "exit 0"])
}

#[tokio::test]
async fn direct_open_options_round_trip() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("file.txt");

    let mut options = direct.open_options();
    let mut file = options
        .write(true)
        .create(true)
        .truncate(true)
        .open(typed(&path))
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut file, b"hello")
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::flush(&mut file).await.unwrap();
    drop(file);

    let contents = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(contents, "hello");
}

/// A FIFO has no seekable offset, so `pread`/`pwrite` fail on it with
/// `ESPIPE` where plain `read`/`write` succeed. Streaming a non-regular file
/// has to keep working, since `open` reaches FIFOs, terminals and sockets just
/// as readily as it reaches regular files.
#[cfg(unix)]
#[tokio::test]
async fn direct_streams_a_non_seekable_file() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("fifo");
    nix::unistd::mkfifo(path.as_path(), nix::sys::stat::Mode::S_IRWXU).unwrap();

    let mut options = direct.open_options();
    // Opening read-write keeps the FIFO from blocking for a peer.
    let mut file = options
        .read(true)
        .write(true)
        .open(typed(&path))
        .await
        .unwrap();

    tokio::io::AsyncWriteExt::write_all(&mut file, b"through the pipe")
        .await
        .unwrap();

    let mut buf = [0u8; 16];
    tokio::io::AsyncReadExt::read_exact(&mut file, &mut buf)
        .await
        .unwrap();
    assert_eq!(&buf, b"through the pipe");

    file.close().await.unwrap();
}

/// Positional I/O names the offset it acts on and leaves the stream cursor
/// alone, in both directions.
#[tokio::test]
async fn direct_positional_io_ignores_the_cursor() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("positional.bin");
    std::fs::write(&path, b"0123456789").unwrap();

    let mut options = direct.open_options();
    let mut file = options
        .read(true)
        .write(true)
        .open(typed(&path))
        .await
        .unwrap();

    // Move the stream cursor somewhere unrelated first.
    assert_eq!(
        tokio::io::AsyncSeekExt::seek(&mut file, io::SeekFrom::Start(7))
            .await
            .unwrap(),
        7
    );

    let mut buf = bytes::BytesMut::with_capacity(4);
    assert_eq!(file.read_at(&mut buf, 2).await.unwrap(), 4);
    assert_eq!(&buf[..], b"2345");

    assert_eq!(
        file.write_at(bytes::Bytes::from_static(b"ab"), 0)
            .await
            .unwrap(),
        2
    );

    // The cursor is exactly where the seek left it.
    assert_eq!(
        tokio::io::AsyncSeekExt::stream_position(&mut file)
            .await
            .unwrap(),
        7
    );

    file.close().await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"ab23456789");
}

/// Reading past the end reports end of file by returning no bytes, and a
/// short read at the boundary returns only what exists.
#[tokio::test]
async fn direct_read_at_reports_short_reads_and_eof() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("short.bin");
    std::fs::write(&path, b"abcdef").unwrap();

    let mut options = direct.open_options();
    let file = options.read(true).open(typed(&path)).await.unwrap();

    let mut buf = bytes::BytesMut::with_capacity(64);
    assert_eq!(file.read_at(&mut buf, 4).await.unwrap(), 2);
    assert_eq!(&buf[..], b"ef");

    // Reads append into the buffer's spare capacity rather than replacing
    // what is there, so a recycled buffer has to be cleared first.
    let before = buf.len();
    assert_eq!(file.read_at(&mut buf, 99).await.unwrap(), 0);
    assert_eq!(buf.len(), before, "a read past the end adds nothing");

    buf.clear();
    assert_eq!(
        file.read_at(&mut buf, 99).await.unwrap(),
        0,
        "end of file is a zero-length transfer"
    );
    assert!(buf.is_empty());

    file.close().await.unwrap();
}

/// The borrowed-destination read agrees with the owned-buffer one on counts and
/// end of file, and reports only what it actually filled.
#[tokio::test]
async fn direct_read_at_into_fills_the_front_of_the_destination() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("into.bin");
    std::fs::write(&path, b"abcdef").unwrap();

    let mut options = direct.open_options();
    let file = options.read(true).open(typed(&path)).await.unwrap();

    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 64];
    let read = file.read_at_into(&mut buf, 4).await.unwrap();
    assert_eq!(read, 2);
    // Only the reported prefix may be read back; the rest is still
    // uninitialized and stays that way.
    let filled: Vec<u8> = buf[..read]
        .iter()
        .map(|byte| unsafe { byte.assume_init() })
        .collect();
    assert_eq!(filled, b"ef");

    assert_eq!(
        file.read_at_into(&mut buf, 99).await.unwrap(),
        0,
        "end of file is a zero-length transfer"
    );

    let mut empty: [std::mem::MaybeUninit<u8>; 0] = [];
    assert_eq!(
        file.read_at_into(&mut empty, 0).await.unwrap(),
        0,
        "nowhere to put anything is a zero-length transfer, not an error"
    );

    file.close().await.unwrap();
}

/// The borrowed-source write agrees with the owned one, and refuses an explicit
/// offset on an append handle for the same reason `write_at` does.
#[tokio::test]
async fn direct_write_at_from_borrows_its_source() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("from.bin");
    std::fs::write(&path, b"......").unwrap();

    let mut options = direct.open_options();
    let file = options
        .read(true)
        .write(true)
        .open(typed(&path))
        .await
        .unwrap();

    let source = b"xy".to_vec();
    assert_eq!(file.write_at_from(&source, 2).await.unwrap(), 2);
    file.commit().await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"..xy..");
    file.close().await.unwrap();

    let mut options = direct.open_options();
    let appender = options
        .append(true)
        .write(true)
        .open(typed(&path))
        .await
        .unwrap();
    let error = appender.write_at_from(&source, 0).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    appender.close().await.unwrap();
}

/// A handoff the handle is too busy for must hand the handle back intact, so a
/// caller can retry once the outstanding work finishes.
#[tokio::test]
async fn a_busy_handoff_returns_the_handle() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("busy.bin");
    std::fs::write(&path, b"abcdef").unwrap();

    let mut options = direct.open_options();
    let file = options.read(true).open(typed(&path)).await.unwrap();

    // A positional operation takes its own reference to the descriptor when it
    // is created, so holding the future without polling it is enough to make
    // the handle non-exclusive.
    let mut buf = bytes::BytesMut::with_capacity(8);
    let pending = file.read_at(&mut buf, 0);

    let failed = file.into_stdio_send(0).await.unwrap_err();
    assert_eq!(
        failed.error().kind(),
        dolang_vfs::error::ErrorKind::ResourceBusy
    );
    let file = failed.into_handle();

    // Nothing was surrendered, so the handle still works and the retry lands
    // once the outstanding operation is gone.
    assert_eq!(pending.await.unwrap(), 6);
    assert_eq!(&buf[..], b"abcdef");
    drop(file.into_stdio_send(0).await.unwrap());
}

/// The point of the positional API: many operations in flight on one handle,
/// with no serialization between them.
#[tokio::test]
async fn direct_positional_reads_run_concurrently() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("windows.bin");
    let payload: Vec<u8> = (0..64u32).map(|i| i as u8).collect();
    std::fs::write(&path, &payload).unwrap();

    let mut options = direct.open_options();
    let file = options.read(true).open(typed(&path)).await.unwrap();

    // Issue every window before awaiting any of them. This only compiles
    // because the operations take `&self` and their futures do not borrow the
    // handle — only their own buffer, which is why each window needs one.
    let mut bufs: Vec<_> = (0..8).map(|_| bytes::BytesMut::with_capacity(8)).collect();
    let pending: Vec<_> = bufs
        .iter_mut()
        .enumerate()
        .map(|(i, buf)| file.read_at(buf, i as u64 * 8))
        .collect();

    for future in pending {
        assert_eq!(future.await.unwrap(), 8);
    }
    let seen: Vec<u8> = bufs.iter().flat_map(|buf| buf.iter().copied()).collect();
    assert_eq!(seen, payload);

    file.close().await.unwrap();
}

/// An append handle has no meaningful notion of "write here", so asking for
/// one is refused rather than silently appending — which is what the kernel
/// would do with the offset thrown away.
#[tokio::test]
async fn direct_write_at_is_rejected_on_an_append_handle() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("append.log");
    std::fs::write(&path, b"start").unwrap();

    let mut options = direct.open_options();
    let file = options.append(true).open(typed(&path)).await.unwrap();

    let error = file
        .write_at(bytes::Bytes::from_static(b"x"), 0)
        .await
        .expect_err("positional write on an append handle");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    // The atomic append path still works, and reports where it landed.
    let (written, end) = file
        .append(bytes::Bytes::from_static(b"more"))
        .await
        .unwrap();
    assert_eq!(written, 4);
    assert_eq!(end, 9);

    file.close().await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"startmore");
}

/// Closing while an operation is still outstanding consumes the handle and
/// lets cleanup finish asynchronously, but says so — the same contract the
/// remote backend has always had for an opaque file still in use.
#[tokio::test]
async fn direct_close_reports_busy_with_an_operation_outstanding() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("busy.bin");
    std::fs::write(&path, vec![7u8; 1024]).unwrap();

    let mut options = direct.open_options();
    let file = options.read(true).open(typed(&path)).await.unwrap();

    let mut buf = bytes::BytesMut::with_capacity(1024);
    let outstanding = file.read_at(&mut buf, 0);
    let error = file
        .close()
        .await
        .expect_err("close with an operation outstanding");
    assert_eq!(error.kind(), dolang_vfs::error::ErrorKind::ResourceBusy);

    // The detached operation still completes against the descriptor it holds.
    assert_eq!(outstanding.await.unwrap(), 1024);
    assert_eq!(buf.len(), 1024);
}

#[tokio::test]
async fn direct_symlink_metadata_and_read_link() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    tokio::fs::write(&target, "hello").await.unwrap();

    direct
        .symlink(typed_str(""), typed(&target), typed(&link))
        .await
        .unwrap();

    let metadata = direct.symlink_metadata(typed(&link)).await.unwrap();
    assert_eq!(metadata.file_type, FileType::Symlink);
    assert_eq!(
        direct.read_link(typed(&link)).await.unwrap().as_str(),
        target.to_str().unwrap()
    );
}

#[tokio::test]
async fn direct_copy_symlink_preserves_link() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    let copied = dir.path().join("copied.txt");
    tokio::fs::write(&target, "hello").await.unwrap();

    direct
        .symlink(typed_str(""), typed(&target), typed(&link))
        .await
        .unwrap();
    direct
        .copy(typed(&link), typed(&copied), false)
        .await
        .unwrap();

    let metadata = direct.symlink_metadata(typed(&copied)).await.unwrap();
    assert_eq!(metadata.file_type, FileType::Symlink);
    assert_eq!(
        direct.read_link(typed(&copied)).await.unwrap().as_str(),
        target.to_str().unwrap()
    );
}

#[tokio::test]
async fn direct_hard_link_round_trip() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    tokio::fs::write(&target, "hello").await.unwrap();

    direct
        .hard_link(typed(&target), typed(&link))
        .await
        .unwrap();

    assert_eq!(tokio::fs::read_to_string(&link).await.unwrap(), "hello");
}

#[cfg(windows)]
#[tokio::test]
async fn direct_metadata_windows_attributes() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("readonly.txt");
    tokio::fs::write(&path, "hello").await.unwrap();

    let mut permissions = tokio::fs::metadata(&path).await.unwrap().permissions();
    permissions.set_readonly(true);
    tokio::fs::set_permissions(&path, permissions)
        .await
        .unwrap();

    let metadata = direct.metadata(typed(&path)).await.unwrap();

    assert!(metadata.windows().is_some());
    let attrs = metadata.win_attrs().unwrap();
    assert_ne!(attrs, 0);
    assert_ne!(attrs & 0x0000_0001, 0);
    assert_ne!(attrs & 0x1, 0);
}

#[tokio::test]
async fn direct_fs_metadata_basic() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("fsmeta.txt");
    tokio::fs::write(&path, "hello").await.unwrap();

    let metadata = direct.fs_metadata(typed(&path), true).await.unwrap();
    assert!(metadata.capacity > 0);
    assert!(metadata.free > 0);
    assert!(metadata.available > 0);
    assert!(metadata.block_size > 0 || cfg!(windows));
}

#[tokio::test]
async fn direct_file_fs_metadata_basic() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("fsmeta-file.txt");
    tokio::fs::write(&path, "hello").await.unwrap();
    let file = direct
        .open_options()
        .read(true)
        .open(typed(&path))
        .await
        .unwrap();

    let metadata = file.fs_metadata().await.unwrap();
    assert!(metadata.capacity > 0);
    assert!(metadata.free > 0);
    assert!(metadata.available > 0);
}

#[cfg(windows)]
#[tokio::test]
async fn direct_security_descriptor_path_and_file() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("security.txt");
    tokio::fs::write(&path, "hello").await.unwrap();

    let mask = SecInfo::OWNER | SecInfo::GROUP | SecInfo::DACL;
    let descriptor = direct.sec_desc(typed(&path), mask, true).await.unwrap();
    assert_eq!(descriptor.mask(), mask);
    assert!(descriptor.owner_loaded());
    assert!(descriptor.owner().is_some());
    assert!(descriptor.group_loaded());
    assert!(descriptor.dacl_loaded());

    let dacl = direct
        .sec_desc(typed(&path), SecInfo::DACL, true)
        .await
        .unwrap();
    match direct.set_sec_desc(typed(&path), &dacl, true).await {
        Ok(()) => {
            let round_trip = direct
                .sec_desc(typed(&path), SecInfo::DACL, true)
                .await
                .unwrap();
            assert_eq!(round_trip.dacl(), dacl.dacl());
        }
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied),
    }

    let file = direct
        .open_options()
        .read(true)
        .open(typed(&path))
        .await
        .unwrap();
    let file_descriptor = file.sec_desc(SecInfo::OWNER).await.unwrap();
    assert!(file_descriptor.owner().is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn direct_security_descriptors_are_unsupported() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("security.txt");
    tokio::fs::write(&path, "hello").await.unwrap();

    let error = direct
        .sec_desc(
            typed(&path),
            dolang_winterop::security::SecInfo::empty(),
            true,
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
}

#[cfg(unix)]
#[tokio::test]
async fn direct_set_metadata_rejects_created_timestamp() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("timestamps.txt");
    tokio::fs::write(&path, "hello").await.unwrap();

    let err = direct
        .set_metadata(
            &[typed(&path).to_path_buf()],
            MetadataPatch {
                created: Some(1_000_000_000),
                ..MetadataPatch::default()
            },
        )
        .await
        .unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
}

#[cfg(windows)]
#[tokio::test]
async fn direct_windows_attrs() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("attrs.txt");
    tokio::fs::write(&path, "hello").await.unwrap();

    direct
        .set_metadata(
            &[typed(&path).to_path_buf()],
            MetadataPatch {
                attrs: attr_patch(AttrFlags::READONLY, true),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let attrs = direct
        .metadata(typed(&path))
        .await
        .unwrap()
        .win_attrs()
        .unwrap();
    assert_ne!(attrs & 0x1, 0);

    direct
        .set_metadata(
            &[typed(&path).to_path_buf()],
            MetadataPatch {
                attrs: attr_patch(AttrFlags::READONLY, false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let attrs = direct
        .metadata(typed(&path))
        .await
        .unwrap()
        .win_attrs()
        .unwrap();
    assert_eq!(attrs & 0x1, 0);

    if dolang_winterop::is_wine() {
        return;
    }

    direct
        .set_metadata(
            &[typed(&path).to_path_buf()],
            MetadataPatch {
                attrs: attr_patch(AttrFlags::COMPRESSED, true),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let attrs = direct
        .metadata(typed(&path))
        .await
        .unwrap()
        .win_attrs()
        .unwrap();
    assert_ne!(attrs & 0x800, 0);

    direct
        .set_metadata(
            &[typed(&path).to_path_buf()],
            MetadataPatch {
                attrs: attr_patch(AttrFlags::COMPRESSED, false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let attrs = direct
        .metadata(typed(&path))
        .await
        .unwrap()
        .win_attrs()
        .unwrap();
    assert_eq!(attrs & 0x800, 0);
}

#[cfg(windows)]
#[tokio::test]
async fn direct_windows_streams() {
    if dolang_winterop::is_wine() {
        return;
    }

    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("streams.txt");
    let stream_path = dir.path().join("streams.txt:zone");
    tokio::fs::write(&path, "base").await.unwrap();
    tokio::fs::write(&stream_path, "stream").await.unwrap();

    let file = direct
        .open_options()
        .read(true)
        .open(typed(&path))
        .await
        .unwrap();
    let streams = file.streams().await.unwrap();
    assert!(streams.iter().any(|entry| {
        entry.name.is_empty()
            && entry.r#type == "DATA"
            && entry.size == 4
            && entry.alloc_size >= entry.size
    }));
    assert!(streams.iter().any(|entry| {
        entry.name == "zone"
            && entry.r#type == "DATA"
            && entry.size == 6
            && entry.alloc_size >= entry.size
    }));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn direct_linux_attrs() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("attrs.txt");
    tokio::fs::write(&path, "hello").await.unwrap();

    let _attrs = match direct.metadata(typed(&path)).await {
        Ok(metadata) => metadata.linux_attrs().unwrap(),
        Err(err)
            if matches!(
                err.raw_os_error(),
                Some(libc::ENOTTY | libc::EOPNOTSUPP | libc::EINVAL)
            ) =>
        {
            return;
        }
        Err(err) => panic!("attrs failed: {err}"),
    };

    if let Err(err) = direct
        .set_metadata(
            &[typed(&path).to_path_buf()],
            MetadataPatch {
                attrs: attr_patch(AttrFlags::NO_DUMP, true),
                ..Default::default()
            },
        )
        .await
    {
        if matches!(
            err.raw_os_error(),
            Some(libc::ENOTTY | libc::EOPNOTSUPP | libc::EINVAL | libc::EPERM)
        ) || err.kind() == io::ErrorKind::PermissionDenied
        {
            return;
        }
        panic!("set_metadata failed: {err}");
    }

    let attrs = direct
        .metadata(typed(&path))
        .await
        .unwrap()
        .linux_attrs()
        .unwrap();
    assert_ne!(attrs & 0x40, 0);

    direct
        .set_metadata(
            &[typed(&path).to_path_buf()],
            MetadataPatch {
                attrs: attr_patch(AttrFlags::NO_DUMP, false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let attrs = direct
        .metadata(typed(&path))
        .await
        .unwrap()
        .linux_attrs()
        .unwrap();
    assert_eq!(attrs & 0x40, 0);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn direct_metadata_handles_unix_socket_without_inode_attrs() {
    use std::os::unix::net::UnixListener;

    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("metadata.sock");
    let _listener = UnixListener::bind(&path).unwrap();

    let metadata = direct.metadata(typed(&path)).await.unwrap();
    assert_eq!(metadata.file_type, FileType::Socket);
    assert_eq!(metadata.linux_attrs(), None);
}

#[tokio::test]
async fn direct_copy_move_and_glob() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let nested = src.join("nested");
    let copied = dir.path().join("copied");
    let moved = dir.path().join("moved");

    tokio::fs::create_dir_all(&nested).await.unwrap();
    tokio::fs::write(nested.join("file.txt"), "hello")
        .await
        .unwrap();

    direct
        .copy(typed(&src), typed(&copied), true)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read_to_string(copied.join("nested").join("file.txt"))
            .await
            .unwrap(),
        "hello"
    );

    direct
        .move_(typed(&copied), typed(&moved), true)
        .await
        .unwrap();
    assert!(!copied.exists());

    let matches = direct
        .glob("**/*.txt", typed(dir.path()), false, None)
        .await
        .unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(
        matches
            .iter()
            .filter(|path| path.file_name().is_some_and(|name| name == "file.txt"))
            .count(),
        2
    );
}

#[tokio::test]
async fn direct_remove_dir_ignore_prunes_empty_branches() {
    let direct = Direct::new().unwrap();
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    tokio::fs::create_dir_all(root.join("keep").join("child"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(root.join("prune").join("leaf"))
        .await
        .unwrap();
    tokio::fs::write(root.join("keep").join("file.txt"), "hello")
        .await
        .unwrap();

    direct.remove_dir(typed(&root), true, true).await.unwrap();

    assert!(root.exists());
    assert!(root.join("keep").exists());
    assert!(!root.join("prune").exists());
}

#[tokio::test]
async fn direct_basic_spawn() {
    let direct = Direct::new().unwrap();
    let (program, args) = successful_command();
    let mut command = direct.command(typed_str(program));
    command.arg(args[0]).arg(args[1]);
    let mut child = command.spawn().await.unwrap();
    let status = child.wait().await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn direct_spawn_failure() {
    let direct = Direct::new().unwrap();
    let result = direct
        .command(typed_str("nonexistent_command_12345"))
        .spawn()
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn direct_exit_code() {
    let direct = Direct::new().unwrap();
    let (program, args) = failing_exit_command();
    let mut command = direct.command(typed_str(program));
    command.arg(args[0]).arg(args[1]);
    let mut child = command.spawn().await.unwrap();
    let status = child.wait().await.unwrap();
    assert!(!status.success());
    assert_eq!(status.code(), Some(42));
}

#[tokio::test]
async fn direct_env_vars() {
    let direct = Direct::new().unwrap();
    let (program, args) = env_forwarding_command();
    let mut command = direct.command(typed_str(program));
    command.arg(args[0]).arg(args[1]).env("TEST_VAR", "value");
    let mut child = command.spawn().await.unwrap();
    let status = child.wait().await.unwrap();
    assert!(status.success());
}

#[cfg(unix)]
#[tokio::test]
async fn direct_well_known_home_dir_prefers_absolute_home_override() {
    let direct = Direct::new().unwrap();
    let env = HashMap::from([(String::from("HOME"), Some(String::from("/tmp/test-home")))]);

    let path = direct
        .well_known_path(dolang_vfs::path::WellKnownPath::HomeDir, None, &env)
        .await
        .unwrap();

    assert_eq!(path.as_str(), "/tmp/test-home");
}

#[cfg(unix)]
#[tokio::test]
async fn direct_well_known_temp_dir_prefers_tmpdir_override() {
    let direct = Direct::new().unwrap();
    let env = HashMap::from([(String::from("TMPDIR"), Some(String::from("/tmp/test-temp")))]);

    let path = direct
        .well_known_path(dolang_vfs::path::WellKnownPath::TempDir, None, &env)
        .await
        .unwrap();

    assert_eq!(path.as_str(), "/tmp/test-temp");
}

#[cfg(unix)]
#[tokio::test]
async fn direct_well_known_temp_dir_falls_back_to_tmp() {
    let direct = Direct::new().unwrap();
    let env = HashMap::from([(String::from("TMPDIR"), None)]);

    let path = direct
        .well_known_path(dolang_vfs::path::WellKnownPath::TempDir, None, &env)
        .await
        .unwrap();

    assert_eq!(path.as_str(), "/tmp");
}

#[cfg(unix)]
#[tokio::test]
async fn direct_well_known_home_dir_rejects_relative_home_override() {
    let direct = Direct::new().unwrap();
    let env = HashMap::from([(String::from("HOME"), Some(String::from("relative-home")))]);

    let err = direct
        .well_known_path(dolang_vfs::path::WellKnownPath::HomeDir, None, &env)
        .await
        .unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[cfg(all(unix, not(target_os = "macos")))]
#[tokio::test]
async fn direct_well_known_cache_dir_prefers_xdg_override() {
    let direct = Direct::new().unwrap();
    let env = HashMap::from([
        (
            String::from("XDG_CACHE_HOME"),
            Some(String::from("/tmp/test-cache")),
        ),
        (String::from("HOME"), Some(String::from("/tmp/test-home"))),
    ]);

    let path = direct
        .well_known_path(dolang_vfs::path::WellKnownPath::CacheDir, None, &env)
        .await
        .unwrap();

    assert_eq!(path.as_str(), "/tmp/test-cache");
}

#[cfg(all(unix, not(target_os = "macos")))]
#[tokio::test]
async fn direct_well_known_cache_dir_falls_back_to_home() {
    let direct = Direct::new().unwrap();
    let env = HashMap::from([
        (String::from("HOME"), Some(String::from("/tmp/test-home"))),
        (String::from("XDG_CACHE_HOME"), None),
    ]);

    let path = direct
        .well_known_path(dolang_vfs::path::WellKnownPath::CacheDir, None, &env)
        .await
        .unwrap();

    assert_eq!(path.as_str(), "/tmp/test-home/.cache");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn direct_well_known_cache_dir_uses_macos_convention() {
    let direct = Direct::new().unwrap();
    let env = HashMap::from([
        (String::from("HOME"), Some(String::from("/tmp/test-home"))),
        (
            String::from("XDG_CACHE_HOME"),
            Some(String::from("/tmp/test-cache")),
        ),
    ]);

    let path = direct
        .well_known_path(dolang_vfs::path::WellKnownPath::CacheDir, None, &env)
        .await
        .unwrap();

    assert_eq!(path.as_str(), "/tmp/test-home/Library/Caches");
}
