use std::{cell::Cell, fmt, fmt::Formatter, io, marker::PhantomData, str};

#[cfg(unix)]
use std::os::fd::{AsFd, OwnedFd};

#[cfg(windows)]
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

// This must have a unique address: the serde wrappers use pointer identity to
// ensure only this private newtype can invoke the unsafe handle path.
static OS_HANDLE_TYPE_BYTES: [u8; 20] = *b"dolang_rpc::OsHandle";
pub(crate) static OS_HANDLE_TYPE: &str = match str::from_utf8(&OS_HANDLE_TYPE_BYTES) {
    Ok(value) => value,
    Err(_) => panic!("invalid handle type marker"),
};

/// The platform's default owned native handle type.
#[cfg(unix)]
pub type DefaultHandle = OwnedFd;

/// The platform's default owned native handle type.
#[cfg(windows)]
pub type DefaultHandle = std::os::windows::io::OwnedHandle;

/// Supplies native handles encountered during serialization.
pub(crate) trait PutHandle<'handle> {
    #[cfg(unix)]
    fn put_handle(&mut self, handle: &'handle dyn ErasedHandle) -> io::Result<u32>;
    #[cfg(windows)]
    fn put_handle(&mut self, handle: &'handle dyn ErasedHandle) -> io::Result<usize>;
}

pub(crate) trait ErasedHandle {
    #[cfg(unix)]
    fn steal_handle(&self) -> OwnedFd;
    #[cfg(windows)]
    fn raw_handle(&self) -> RawHandle;
    #[cfg(windows)]
    fn steal_handle(&self) -> OwnedHandle;
}

pub(crate) struct HandleRef<'handle>(pub(crate) &'handle dyn ErasedHandle);

impl Serialize for HandleRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[cfg(target_pointer_width = "32")]
        return serializer.serialize_u32(self as *const Self as usize as u32);
        #[cfg(target_pointer_width = "64")]
        return serializer.serialize_u64(self as *const Self as usize as u64);
    }
}

/// Consumes native handles encountered during deserialization.
pub(crate) trait TakeHandle {
    #[cfg(unix)]
    fn take_handle(&mut self, index: u32) -> io::Result<OwnedFd>;
    #[cfg(windows)]
    fn take_handle(&mut self, value: usize) -> io::Result<OwnedHandle>;

    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A native operating-system resource transferred as a frame attachment.
///
/// Serialize this type only over an attachment-capable session transport:
/// [`Builder::client_unix`](crate::Builder::client_unix) or
/// [`Builder::server_unix`](crate::Builder::server_unix) on Unix, and the
/// named-pipe constructors on Windows. Serializing it over a generic byte
/// stream currently panics; use [`Opaque`](crate::session::Opaque) for resources that
/// must work over every transport.
pub struct OsHandle<T = DefaultHandle>(Cell<Option<T>>);

impl<T> fmt::Debug for OsHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OsHandle(..)")
    }
}

impl<T> OsHandle<T> {
    /// Wraps a native handle-like value for direct attachment serialization.
    pub fn new(value: T) -> Self {
        Self(Cell::new(Some(value)))
    }

    /// Returns the wrapped value.
    ///
    /// # Panics
    ///
    /// Panics if successful serialization already consumed the handle.
    pub fn into_inner(self) -> T {
        self.0
            .into_inner()
            .expect("operating-system handle was already consumed")
    }
}

impl<T> From<T> for OsHandle<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

#[cfg(unix)]
impl<T: AsFd + Into<OwnedFd>> ErasedHandle for OsHandle<T> {
    fn steal_handle(&self) -> OwnedFd {
        self.0
            .take()
            .expect("operating-system handle was already consumed")
            .into()
    }
}

#[cfg(unix)]
impl<T: AsFd + Into<OwnedFd>> Serialize for OsHandle<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(OS_HANDLE_TYPE, &HandleRef(self))
    }
}

#[cfg(unix)]
impl<'de, T: From<OwnedFd>> Deserialize<'de> for OsHandle<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(PhantomData<T>);

        impl<'de, T: From<OwnedFd>> de::Visitor<'de> for Visitor<T> {
            type Value = OsHandle<T>;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an operating-system handle")
            }

            fn visit_newtype_struct<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                use std::os::fd::FromRawFd;
                let raw = i32::deserialize(deserializer)?;
                Ok(OsHandle::new(T::from(unsafe { OwnedFd::from_raw_fd(raw) })))
            }
        }

        deserializer.deserialize_newtype_struct(OS_HANDLE_TYPE, Visitor(PhantomData))
    }
}

#[cfg(windows)]
impl<T: AsHandle + Into<OwnedHandle>> ErasedHandle for OsHandle<T> {
    fn raw_handle(&self) -> RawHandle {
        let value = self
            .0
            .take()
            .expect("operating-system handle was already consumed");
        let raw = value.as_handle().as_raw_handle();
        self.0.set(Some(value));
        raw
    }

    fn steal_handle(&self) -> OwnedHandle {
        self.0
            .take()
            .expect("operating-system handle was already consumed")
            .into()
    }
}

#[cfg(windows)]
impl<T: AsHandle + Into<OwnedHandle>> Serialize for OsHandle<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(OS_HANDLE_TYPE, &HandleRef(self))
    }
}

#[cfg(windows)]
impl<'de, T: From<OwnedHandle>> Deserialize<'de> for OsHandle<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(PhantomData<T>);

        impl<'de, T: From<OwnedHandle>> de::Visitor<'de> for Visitor<T> {
            type Value = OsHandle<T>;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an operating-system handle")
            }

            fn visit_newtype_struct<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                #[cfg(target_pointer_width = "32")]
                let raw = u32::deserialize(deserializer)? as usize;
                #[cfg(target_pointer_width = "64")]
                let raw = u64::deserialize(deserializer)? as usize;
                Ok(OsHandle::new(T::from(unsafe {
                    OwnedHandle::from_raw_handle(raw as _)
                })))
            }
        }

        deserializer.deserialize_newtype_struct(OS_HANDLE_TYPE, Visitor(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SAME_TYPE_BYTES: [u8; 20] = *b"dolang_rpc::OsHandle";

    #[test]
    fn handle_type_uses_identity_not_contents() {
        let same = unsafe { std::str::from_utf8_unchecked(&SAME_TYPE_BYTES) };
        assert_eq!(same, OS_HANDLE_TYPE);
        assert!(!std::ptr::eq(same, OS_HANDLE_TYPE));
    }
}
