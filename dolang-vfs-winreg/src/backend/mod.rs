#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use self::windows::handle;
