use std::{cell::RefCell, io};

use ::serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use ::serde::{de::Error as _, ser::Error as _};
use bytes::Bytes;
use postcard::ser_flavors::{ExtendFlavor, Flavor};

use crate::{
    Error as RpcError,
    handle::{ErasedHandle, PutHandle, TakeHandle},
    session::{self, Ref},
};

struct Context<T: ?Sized + 'static> {
    handles: &'static mut T,
    error: &'static RefCell<Option<RpcError>>,
}

thread_local! {
    static PUT: RefCell<Option<Context<dyn PutHandle>>> = const { RefCell::new(None) };
    static TAKE: RefCell<Option<Context<dyn TakeHandle>>> = const { RefCell::new(None) };
}

struct Restore<'a, T: ?Sized + 'static> {
    slot: &'a RefCell<Option<Context<T>>>,
    value: Option<Context<T>>,
}

impl<T: ?Sized + 'static> Drop for Restore<'_, T> {
    fn drop(&mut self) {
        self.slot.replace(self.value.take());
    }
}

fn scope<T: ?Sized + 'static, R>(
    slot: &'static std::thread::LocalKey<RefCell<Option<Context<T>>>>,
    handles: &'static mut T,
    f: impl FnOnce() -> R,
) -> (R, Option<RpcError>) {
    let error = RefCell::new(None);
    // SAFETY: the erased references are installed only for this synchronous
    // call. `Restore` removes them on every return and unwind path, and access
    // is confined to an immediately invoked closure that cannot return them.
    let error_ref = unsafe {
        std::mem::transmute::<&RefCell<Option<RpcError>>, &'static RefCell<Option<RpcError>>>(
            &error,
        )
    };
    let value = Context {
        handles,
        error: error_ref,
    };
    let result = slot.with(|slot| {
        let previous = slot.replace(Some(value));
        let _restore = Restore {
            slot,
            value: previous,
        };
        f()
    });
    (result, error.into_inner())
}

fn access<T: ?Sized + 'static, R>(
    slot: &'static std::thread::LocalKey<RefCell<Option<Context<T>>>>,
    f: impl FnOnce(&mut T) -> io::Result<R>,
    map_error: impl FnOnce(String, io::ErrorKind) -> RpcError,
) -> Result<R, String> {
    slot.with(|slot| {
        let current = slot
            .replace(None)
            .expect("RPC-only value used outside an RPC session");
        let mut restore = Restore {
            slot,
            value: Some(current),
        };
        let context = restore.value.as_mut().unwrap();
        match f(context.handles) {
            Ok(value) => Ok(value),
            Err(error) => {
                let message = error.to_string();
                let rpc_error = map_error(message.clone(), error.kind());
                *context.error.borrow_mut() = Some(rpc_error);
                Err(message)
            }
        }
    })
}

pub(crate) fn encode_payload<T: Serialize>(
    value: &T,
    handles: &mut impl PutHandle,
) -> Result<Bytes, RpcError> {
    let handles: &mut dyn PutHandle = handles;
    // SAFETY: `scope` removes this reference from TLS before returning.
    let handles: &'static mut dyn PutHandle = unsafe { std::mem::transmute(handles) };
    let (result, context_error) = scope(&PUT, handles, || {
        let mut postcard = postcard::Serializer {
            output: ExtendFlavor::new(Vec::new()),
        };
        value
            .serialize(&mut postcard)
            .and_then(|()| postcard.output.finalize())
    });
    match result {
        Ok(bytes) => Ok(bytes.into()),
        Err(error) => Err(context_error.unwrap_or_else(|| RpcError::Serialize(error.to_string()))),
    }
}

pub(crate) fn decode_payload<T: de::DeserializeOwned>(
    bytes: &[u8],
    handles: &mut impl TakeHandle,
) -> Result<T, RpcError> {
    let handles: &mut dyn TakeHandle = handles;
    // SAFETY: `scope` removes this reference from TLS before returning.
    let scoped_handles: &'static mut dyn TakeHandle = unsafe { std::mem::transmute(handles) };
    let (result, context_error) = scope(&TAKE, scoped_handles, || {
        let mut postcard = postcard::Deserializer::from_bytes(bytes);
        let value = T::deserialize(&mut postcard)?;
        let remaining = postcard.finalize()?;
        if !remaining.is_empty() {
            return Err(postcard::Error::DeserializeBadEncoding);
        }
        access(
            &TAKE,
            |handles| handles.finish(),
            |message, _| RpcError::Deserialize(message),
        )
        .map_err(|_| postcard::Error::SerdeDeCustom)?;
        Ok(value)
    });
    result
        .map_err(|error| context_error.unwrap_or_else(|| RpcError::Deserialize(error.to_string())))
}

pub(crate) fn serialize_opaque<S: Serializer>(
    opaque: &Ref,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let (owner, id) = access(&PUT, |handles| handles.put_opaque(opaque), encode_error)
        .map_err(S::Error::custom)?;
    serializer.serialize_u64(session::pack_wire(owner, id))
}

pub(crate) fn deserialize_opaque<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Ref, D::Error> {
    let (owner, id) = session::unpack_wire(u64::deserialize(deserializer)?);
    access(
        &TAKE,
        |handles| handles.take_opaque(owner, id),
        |message, _| RpcError::Deserialize(message),
    )
    .map_err(D::Error::custom)
}

#[cfg(unix)]
pub(crate) fn serialize_handle<S: Serializer>(
    handle: &dyn ErasedHandle,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let index = access(&PUT, |handles| handles.put_handle(handle), encode_error)
        .map_err(S::Error::custom)?;
    serializer.serialize_u32(index)
}

#[cfg(unix)]
pub(crate) fn deserialize_handle<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<std::os::fd::OwnedFd, D::Error> {
    let index = u32::deserialize(deserializer)?;
    access(
        &TAKE,
        |handles| handles.take_handle(index),
        |message, _| RpcError::Deserialize(message),
    )
    .map_err(D::Error::custom)
}

#[cfg(windows)]
pub(crate) fn serialize_handle<S: Serializer>(
    handle: &dyn ErasedHandle,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let value = access(&PUT, |handles| handles.put_handle(handle), encode_error)
        .map_err(S::Error::custom)?;
    #[cfg(target_pointer_width = "32")]
    return serializer.serialize_u32(value as u32);
    #[cfg(target_pointer_width = "64")]
    return serializer.serialize_u64(value as u64);
}

#[cfg(windows)]
pub(crate) fn deserialize_handle<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<std::os::windows::io::OwnedHandle, D::Error> {
    #[cfg(target_pointer_width = "32")]
    let value = u32::deserialize(deserializer)? as usize;
    #[cfg(target_pointer_width = "64")]
    let value = u64::deserialize(deserializer)? as usize;
    access(
        &TAKE,
        |handles| handles.take_handle(value),
        |message, _| RpcError::Deserialize(message),
    )
    .map_err(D::Error::custom)
}

fn encode_error(message: String, kind: io::ErrorKind) -> RpcError {
    if kind == io::ErrorKind::Unsupported {
        RpcError::UnsupportedCapability
    } else {
        RpcError::Serialize(message)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::fd::OwnedFd;

    use nix::unistd::pipe;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{handle::OsHandle, transport::EncodeHandles};

    struct Sender(EncodeHandles);

    impl PutHandle for Sender {
        fn put_handle(&mut self, handle: &dyn ErasedHandle) -> io::Result<u32> {
            self.0.put_handle(handle)
        }

        fn put_opaque(&mut self, _: &Ref) -> io::Result<(u8, u64)> {
            unreachable!()
        }
    }

    struct Receiver(Vec<Option<OwnedFd>>);

    impl TakeHandle for Receiver {
        fn take_handle(&mut self, index: u32) -> io::Result<OwnedFd> {
            self.0
                .get_mut(index as usize)
                .and_then(Option::take)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid fd"))
        }

        fn take_opaque(&mut self, _: u8, _: u64) -> io::Result<Ref> {
            unreachable!()
        }
    }

    #[derive(Serialize, Deserialize)]
    struct Message {
        handles: Vec<OsHandle<OwnedFd>>,
    }

    #[test]
    fn handles_round_trip_through_context() {
        let (fd, _) = pipe().unwrap();
        let value = Message {
            handles: vec![OsHandle::new(fd)],
        };
        let mut sender = Sender(EncodeHandles::for_test(1));
        let bytes = encode_payload(&value, &mut sender).unwrap();
        let sent = sender.0.finish().fds;
        let decoded: Message =
            decode_payload(&bytes, &mut Receiver(sent.into_iter().map(Some).collect())).unwrap();
        assert_eq!(decoded.handles.len(), 1);
    }

    #[test]
    fn serializing_one_handle_twice_is_rejected() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let value = (&handle, &handle);
        let mut sender = Sender(EncodeHandles::for_test(2));
        let error = encode_payload(&value, &mut sender).unwrap_err();
        assert!(error.to_string().contains("same handle"));
    }

    #[test]
    fn standalone_handle_serde_panics() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                postcard::to_allocvec(&handle)
            }))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| postcard::from_bytes::<OsHandle<OwnedFd>>(&[0])).is_err()
        );
    }

    #[test]
    fn tls_scope_restores_after_unwind() {
        let mut outer = Sender(EncodeHandles::for_test(0));
        let outer: &mut dyn PutHandle = &mut outer;
        let outer: &'static mut dyn PutHandle = unsafe { std::mem::transmute(outer) };
        let _ = scope(&PUT, outer, || {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut inner = Sender(EncodeHandles::for_test(0));
                    let inner: &mut dyn PutHandle = &mut inner;
                    let inner: &'static mut dyn PutHandle = unsafe { std::mem::transmute(inner) };
                    scope(&PUT, inner, || panic!("test unwind"));
                }))
                .is_err()
            );
            PUT.with(|slot| assert!(slot.borrow().is_some()));
        });
        PUT.with(|slot| assert!(slot.borrow().is_none()));
    }
}
