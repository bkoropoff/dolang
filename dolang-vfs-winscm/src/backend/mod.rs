#[cfg(windows)]
pub(crate) mod windows;

#[cfg(windows)]
pub(crate) use self::windows::handle;
