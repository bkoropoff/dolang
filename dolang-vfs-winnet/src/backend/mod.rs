#[cfg(windows)]
pub(crate) mod windows;

#[cfg(windows)]
pub(crate) use windows::handle;
