#[cfg(unix)]
use std::os::fd::IntoRawFd;
#[cfg(windows)]
use std::os::windows::io::IntoRawHandle;
use std::{cell::RefCell, fmt, io, marker::PhantomData, ptr};

use ::serde::{
    Deserialize, Serialize,
    de::{
        self, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess,
        Visitor,
    },
    ser::{self},
};
use bytes::Bytes;
use postcard::ser_flavors::{ExtendFlavor, Flavor};

use crate::{
    Error as RpcError,
    handle::{HandleRef, OS_HANDLE_TYPE, PutHandle, TakeHandle},
};

pub(crate) fn encode_payload<'handle, T: Serialize, H: PutHandle<'handle>>(
    value: &'handle T,
    handles: &mut H,
) -> Result<Bytes, RpcError> {
    let buffer = to_extend(value, handles, Vec::new()).map_err(|error| match error {
        Error::UnsupportedCapability => RpcError::UnsupportedCapability,
        error => RpcError::Serialize(error.to_string()),
    })?;
    Ok(buffer.into())
}

pub(crate) fn decode_payload<T: de::DeserializeOwned>(
    bytes: &[u8],
    handles: &mut impl TakeHandle,
) -> Result<T, RpcError> {
    let value = from_bytes(bytes, &mut *handles)
        .map_err(|error| RpcError::Deserialize(error.to_string()))?;
    if let Err(error) = handles.finish() {
        drop(value);
        return Err(RpcError::Deserialize(error.to_string()));
    }
    Ok(value)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("postcard error: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("{0}")]
    Message(String),
    /// A handle was serialized over a transport that cannot carry one.
    #[error("transport does not support direct handles")]
    UnsupportedCapability,
}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Message(msg.to_string())
    }
}
impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Message(msg.to_string())
    }
}

// `Serializer`/`Deserializer` below use our own rich `Error` as their
// associated error type throughout, so ordinary structural failures (a full
// buffer, a malformed tag, ...) just convert via `Display` and nothing is
// lost. But a handful of methods (`WithFrame::serialize`,
// `SeedWrap::deserialize`, and the `VisitorWrap` methods that hand back a
// fresh (de)serializer) are constrained by the external `serde` trait
// signature that invokes them to return the *caller's* generic `S::Error`/
// `D::Error` — ultimately postcard's own error type once the recursion
// bottoms out. postcard's `custom` constructor for that type discards its
// message and returns a fixed generic variant
// (`SerdeSerCustom`/`SerdeDeCustom`), so a rich error can't just be converted
// there. Instead we stash it on the `RefCell` threaded through the
// (de)serialization context and raise a placeholder in its place;
// `to_extend`/`from_bytes` prefer the stashed error over whatever
// placeholder bubbles up through postcard.
fn convert_ser_error<E: ser::Error>(err: E) -> Error {
    Error::Message(err.to_string())
}
fn convert_de_error<E: de::Error>(err: E) -> Error {
    Error::Message(err.to_string())
}
fn stash_ser_error<E: ser::Error>(slot: &RefCell<Option<Error>>, err: Error) -> E {
    let message = err.to_string();
    *slot.borrow_mut() = Some(err);
    E::custom(message)
}
fn stash_de_error<E: de::Error>(slot: &RefCell<Option<Error>>, err: Error) -> E {
    let message = err.to_string();
    *slot.borrow_mut() = Some(err);
    E::custom(message)
}

pub(crate) fn to_extend<'frame, T, F>(
    value: &'frame T,
    frame: &mut F,
    output: Vec<u8>,
) -> Result<Vec<u8>, Error>
where
    T: Serialize + ?Sized,
    F: PutHandle<'frame>,
{
    let mut postcard = postcard::Serializer {
        output: ExtendFlavor::new(output),
    };
    let frame = RefCell::new(frame);
    let error = RefCell::new(None);
    if let Err(err) = value.serialize(Serializer {
        inner: &mut postcard,
        frame: &frame,
        error: &error,
        marker: PhantomData,
    }) {
        return Err(error.into_inner().unwrap_or(err));
    }
    Ok(postcard.output.finalize()?)
}

pub(crate) fn from_bytes<'de, T, H>(bytes: &'de [u8], handles: &mut H) -> Result<T, Error>
where
    T: Deserialize<'de>,
    H: TakeHandle,
{
    let mut postcard = postcard::Deserializer::from_bytes(bytes);
    let error = RefCell::new(None);
    let value = match T::deserialize(Deserializer {
        inner: &mut postcard,
        handles,
        error: &error,
    }) {
        Ok(value) => value,
        Err(err) => return Err(error.into_inner().unwrap_or(err)),
    };
    let remaining = postcard.finalize()?;
    if !remaining.is_empty() {
        return Err(Error::Message("trailing bytes in payload".into()));
    }
    Ok(value)
}

struct Serializer<'cell, 'borrow, 'frame, S, F> {
    inner: S,
    frame: &'cell RefCell<&'borrow mut F>,
    error: &'cell RefCell<Option<Error>>,
    marker: PhantomData<&'frame ()>,
}
struct WithFrame<'value, 'cell, 'borrow, 'frame, T: ?Sized, F> {
    value: &'value T,
    frame: &'cell RefCell<&'borrow mut F>,
    error: &'cell RefCell<Option<Error>>,
    marker: PhantomData<&'frame ()>,
}
struct Compound<'cell, 'borrow, 'frame, C, F> {
    inner: C,
    frame: &'cell RefCell<&'borrow mut F>,
    error: &'cell RefCell<Option<Error>>,
    marker: PhantomData<&'frame ()>,
}

impl<'frame, T: Serialize + ?Sized, F: PutHandle<'frame>> Serialize
    for WithFrame<'_, '_, '_, 'frame, T, F>
{
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value
            .serialize(Serializer {
                inner: serializer,
                frame: self.frame,
                error: self.error,
                marker: PhantomData,
            })
            .map_err(|err| stash_ser_error(self.error, err))
    }
}

macro_rules! forward_ser {
    ($($name:ident($ty:ty)),* $(,)?) => {$(
        fn $name(self, value: $ty) -> Result<Self::Ok, Self::Error> {
            self.inner.$name(value).map_err(convert_ser_error)
        }
    )*};
}

impl<'cell, 'borrow, 'frame, S, F> ser::Serializer for Serializer<'cell, 'borrow, 'frame, S, F>
where
    F: PutHandle<'frame>,
    S: ser::Serializer,
{
    type Ok = S::Ok;
    type Error = Error;
    type SerializeSeq = Compound<'cell, 'borrow, 'frame, S::SerializeSeq, F>;
    type SerializeTuple = Compound<'cell, 'borrow, 'frame, S::SerializeTuple, F>;
    type SerializeTupleStruct = Compound<'cell, 'borrow, 'frame, S::SerializeTupleStruct, F>;
    type SerializeTupleVariant = Compound<'cell, 'borrow, 'frame, S::SerializeTupleVariant, F>;
    type SerializeMap = Compound<'cell, 'borrow, 'frame, S::SerializeMap, F>;
    type SerializeStruct = Compound<'cell, 'borrow, 'frame, S::SerializeStruct, F>;
    type SerializeStructVariant = Compound<'cell, 'borrow, 'frame, S::SerializeStructVariant, F>;

    forward_ser!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char)
    );
    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_str(v).map_err(convert_ser_error)
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_bytes(v).map_err(convert_ser_error)
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_none().map_err(convert_ser_error)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        self.inner
            .serialize_some(&WithFrame {
                value,
                frame: self.frame,
                error: self.error,
                marker: PhantomData,
            })
            .map_err(convert_ser_error)
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit().map_err(convert_ser_error)
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.inner
            .serialize_unit_struct(name)
            .map_err(convert_ser_error)
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.inner
            .serialize_unit_variant(name, index, variant)
            .map_err(convert_ser_error)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        if ptr::eq(name, OS_HANDLE_TYPE) {
            let raw = value.serialize(RawHandleSerializer)?;
            // SAFETY: only `OsHandle::serialize` can supply the private marker
            // by identity. It passes a live `HandleRef` whose referent belongs
            // to the message borrowed for `'frame`.
            let handle = unsafe { (*(raw as *const HandleRef<'frame>)).0 };
            let value = self.frame.borrow_mut().put_handle(handle).map_err(|err| {
                if err.kind() == io::ErrorKind::Unsupported {
                    Error::UnsupportedCapability
                } else {
                    Error::Message(err.to_string())
                }
            })?;
            #[cfg(unix)]
            return self.inner.serialize_u32(value).map_err(convert_ser_error);
            #[cfg(all(windows, target_pointer_width = "32"))]
            return self
                .inner
                .serialize_u32(value as u32)
                .map_err(convert_ser_error);
            #[cfg(all(windows, target_pointer_width = "64"))]
            return self
                .inner
                .serialize_u64(value as u64)
                .map_err(convert_ser_error);
        }
        self.inner
            .serialize_newtype_struct(
                name,
                &WithFrame {
                    value,
                    frame: self.frame,
                    error: self.error,
                    marker: PhantomData,
                },
            )
            .map_err(convert_ser_error)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        self.inner
            .serialize_newtype_variant(
                name,
                index,
                variant,
                &WithFrame {
                    value,
                    frame: self.frame,
                    error: self.error,
                    marker: PhantomData,
                },
            )
            .map_err(convert_ser_error)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(Compound {
            inner: self.inner.serialize_seq(len).map_err(convert_ser_error)?,
            frame: self.frame,
            error: self.error,
            marker: PhantomData,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(Compound {
            inner: self.inner.serialize_tuple(len).map_err(convert_ser_error)?,
            frame: self.frame,
            error: self.error,
            marker: PhantomData,
        })
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(Compound {
            inner: self
                .inner
                .serialize_tuple_struct(name, len)
                .map_err(convert_ser_error)?,
            frame: self.frame,
            error: self.error,
            marker: PhantomData,
        })
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(Compound {
            inner: self
                .inner
                .serialize_tuple_variant(name, index, variant, len)
                .map_err(convert_ser_error)?,
            frame: self.frame,
            error: self.error,
            marker: PhantomData,
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(Compound {
            inner: self.inner.serialize_map(len).map_err(convert_ser_error)?,
            frame: self.frame,
            error: self.error,
            marker: PhantomData,
        })
    }
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(Compound {
            inner: self
                .inner
                .serialize_struct(name, len)
                .map_err(convert_ser_error)?,
            frame: self.frame,
            error: self.error,
            marker: PhantomData,
        })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(Compound {
            inner: self
                .inner
                .serialize_struct_variant(name, index, variant, len)
                .map_err(convert_ser_error)?,
            frame: self.frame,
            error: self.error,
            marker: PhantomData,
        })
    }
    fn collect_str<T: ?Sized + fmt::Display>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        self.inner.collect_str(value).map_err(convert_ser_error)
    }
    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

macro_rules! compound {
    ($trait:ident,$method:ident) => {
        impl<'frame, C: ser::$trait, F: PutHandle<'frame>> ser::$trait
            for Compound<'_, '_, 'frame, C, F>
        {
            type Ok = C::Ok;
            type Error = Error;
            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
                self.inner
                    .$method(&WithFrame {
                        value,
                        frame: self.frame,
                        error: self.error,
                        marker: PhantomData,
                    })
                    .map_err(convert_ser_error)
            }
            fn end(self) -> Result<Self::Ok, Self::Error> {
                self.inner.end().map_err(convert_ser_error)
            }
        }
    };
}
compound!(SerializeSeq, serialize_element);
compound!(SerializeTuple, serialize_element);
compound!(SerializeTupleStruct, serialize_field);
compound!(SerializeTupleVariant, serialize_field);
impl<'frame, C: ser::SerializeMap, F: PutHandle<'frame>> ser::SerializeMap
    for Compound<'_, '_, 'frame, C, F>
{
    type Ok = C::Ok;
    type Error = Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.inner
            .serialize_key(&WithFrame {
                value: v,
                frame: self.frame,
                error: self.error,
                marker: PhantomData,
            })
            .map_err(convert_ser_error)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.inner
            .serialize_value(&WithFrame {
                value: v,
                frame: self.frame,
                error: self.error,
                marker: PhantomData,
            })
            .map_err(convert_ser_error)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end().map_err(convert_ser_error)
    }
}
impl<'frame, C: ser::SerializeStruct, F: PutHandle<'frame>> ser::SerializeStruct
    for Compound<'_, '_, 'frame, C, F>
{
    type Ok = C::Ok;
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), Self::Error> {
        self.inner
            .serialize_field(
                key,
                &WithFrame {
                    value: v,
                    frame: self.frame,
                    error: self.error,
                    marker: PhantomData,
                },
            )
            .map_err(convert_ser_error)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end().map_err(convert_ser_error)
    }
}
impl<'frame, C: ser::SerializeStructVariant, F: PutHandle<'frame>> ser::SerializeStructVariant
    for Compound<'_, '_, 'frame, C, F>
{
    type Ok = C::Ok;
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), Self::Error> {
        self.inner
            .serialize_field(
                key,
                &WithFrame {
                    value: v,
                    frame: self.frame,
                    error: self.error,
                    marker: PhantomData,
                },
            )
            .map_err(convert_ser_error)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end().map_err(convert_ser_error)
    }
}

struct RawHandleSerializer;

impl ser::Serializer for RawHandleSerializer {
    type Ok = usize;
    type Error = Error;
    type SerializeSeq = ser::Impossible<usize, Error>;
    type SerializeTuple = Self::SerializeSeq;
    type SerializeTupleStruct = Self::SerializeSeq;
    type SerializeTupleVariant = Self::SerializeSeq;
    type SerializeMap = Self::SerializeSeq;
    type SerializeStruct = Self::SerializeSeq;
    type SerializeStructVariant = Self::SerializeSeq;
    fn serialize_i32(self, v: i32) -> Result<usize, Error> {
        let _ = v;
        #[cfg(unix)]
        return Ok(v as usize);
        #[cfg(windows)]
        return raw_error();
    }
    fn is_human_readable(&self) -> bool {
        false
    }
    fn serialize_bool(self, _: bool) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_i8(self, _: i8) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_i16(self, _: i16) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_i64(self, _: i64) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_i128(self, _: i128) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_u8(self, _: u8) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_u16(self, _: u16) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_u32(self, v: u32) -> Result<usize, Error> {
        let _ = v;
        #[cfg(target_pointer_width = "32")]
        return Ok(v as usize);
        #[cfg(not(target_pointer_width = "32"))]
        return raw_error();
    }
    fn serialize_u64(self, v: u64) -> Result<usize, Error> {
        let _ = v;
        #[cfg(target_pointer_width = "64")]
        return Ok(v as usize);
        #[cfg(not(target_pointer_width = "64"))]
        return raw_error();
    }
    fn serialize_u128(self, _: u128) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_f32(self, _: f32) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_f64(self, _: f64) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_char(self, _: char) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_str(self, _: &str) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_none(self) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_unit(self) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<usize, Error> {
        raw_error()
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        raw_error()
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        raw_error()
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        raw_error()
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        raw_error()
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        raw_error()
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        raw_error()
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        raw_error()
    }
}

fn raw_error<T>() -> Result<T, Error> {
    Err(Error::Message("invalid OsHandle representation".into()))
}

struct Deserializer<'a, D, H> {
    inner: D,
    handles: &'a mut H,
    error: &'a RefCell<Option<Error>>,
}
struct VisitorWrap<'a, V, H> {
    inner: V,
    handles: &'a mut H,
    error: &'a RefCell<Option<Error>>,
}
struct SeedWrap<'a, S, H> {
    inner: S,
    handles: &'a mut H,
    error: &'a RefCell<Option<Error>>,
}
struct SeqWrap<'a, A, H> {
    inner: A,
    handles: &'a mut H,
    error: &'a RefCell<Option<Error>>,
}
struct MapWrap<'a, A, H> {
    inner: A,
    handles: &'a mut H,
    error: &'a RefCell<Option<Error>>,
}
struct EnumWrap<'a, A, H> {
    inner: A,
    handles: &'a mut H,
    error: &'a RefCell<Option<Error>>,
}
struct VariantWrap<'a, A, H> {
    inner: A,
    handles: &'a mut H,
    error: &'a RefCell<Option<Error>>,
}

macro_rules! forward_de {
    ($($method:ident),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            self.inner
                .$method(VisitorWrap { inner: visitor, handles: self.handles, error: self.error })
                .map_err(convert_de_error)
        }
    )*};
}

impl<'de, D, H> de::Deserializer<'de> for Deserializer<'_, D, H>
where
    D: de::Deserializer<'de>,
    H: TakeHandle,
{
    type Error = Error;
    forward_de!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_i128,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_u128,
        deserialize_f32,
        deserialize_f64,
        deserialize_char,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_seq,
        deserialize_map,
        deserialize_identifier,
        deserialize_ignored_any
    );
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner
            .deserialize_unit_struct(
                name,
                VisitorWrap {
                    inner: visitor,
                    handles: self.handles,
                    error: self.error,
                },
            )
            .map_err(convert_de_error)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if ptr::eq(name, OS_HANDLE_TYPE) {
            #[cfg(unix)]
            {
                let index = u32::deserialize(self.inner).map_err(convert_de_error)?;
                let fd = self
                    .handles
                    .take_handle(index)
                    .map_err(|err| Error::Message(err.to_string()))?;
                // The private OsHandle visitor immediately adopts this raw fd.
                // No other visitor can request the reserved newtype identity.
                let raw = fd.into_raw_fd();
                return visitor
                    .visit_newtype_struct(IntoDeserializer::<Error>::into_deserializer(raw))
                    .map_err(convert_de_error);
            }
            #[cfg(windows)]
            {
                #[cfg(target_pointer_width = "32")]
                let value = u32::deserialize(self.inner).map_err(convert_de_error)? as usize;
                #[cfg(target_pointer_width = "64")]
                let value = u64::deserialize(self.inner).map_err(convert_de_error)? as usize;
                let handle = self
                    .handles
                    .take_handle(value)
                    .map_err(|err| Error::Message(err.to_string()))?;
                let raw = handle.into_raw_handle() as usize;
                #[cfg(target_pointer_width = "32")]
                return visitor
                    .visit_newtype_struct(IntoDeserializer::<Error>::into_deserializer(raw as u32))
                    .map_err(convert_de_error);
                #[cfg(target_pointer_width = "64")]
                return visitor
                    .visit_newtype_struct(IntoDeserializer::<Error>::into_deserializer(raw as u64))
                    .map_err(convert_de_error);
            }
        }
        self.inner
            .deserialize_newtype_struct(
                name,
                VisitorWrap {
                    inner: visitor,
                    handles: self.handles,
                    error: self.error,
                },
            )
            .map_err(convert_de_error)
    }
    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner
            .deserialize_tuple(
                len,
                VisitorWrap {
                    inner: visitor,
                    handles: self.handles,
                    error: self.error,
                },
            )
            .map_err(convert_de_error)
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner
            .deserialize_tuple_struct(
                name,
                len,
                VisitorWrap {
                    inner: visitor,
                    handles: self.handles,
                    error: self.error,
                },
            )
            .map_err(convert_de_error)
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner
            .deserialize_struct(
                name,
                fields,
                VisitorWrap {
                    inner: visitor,
                    handles: self.handles,
                    error: self.error,
                },
            )
            .map_err(convert_de_error)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner
            .deserialize_enum(
                name,
                variants,
                VisitorWrap {
                    inner: visitor,
                    handles: self.handles,
                    error: self.error,
                },
            )
            .map_err(convert_de_error)
    }
    fn is_human_readable(&self) -> bool {
        false
    }
}

impl<'de, S, H> DeserializeSeed<'de> for SeedWrap<'_, S, H>
where
    S: DeserializeSeed<'de>,
    H: TakeHandle,
{
    type Value = S::Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        self.inner
            .deserialize(Deserializer {
                inner: d,
                handles: self.handles,
                error: self.error,
            })
            .map_err(|err| stash_de_error(self.error, err))
    }
}

macro_rules! visit_scalar { ($($name:ident($ty:ty)),* $(,)?) => {$(
 fn $name<E:de::Error>(self,v:$ty)->Result<Self::Value,E>{self.inner.$name(v)}
 )*}; }
impl<'de, V: Visitor<'de>, H: TakeHandle> Visitor<'de> for VisitorWrap<'_, V, H> {
    type Value = V::Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(f)
    }
    visit_scalar!(
        visit_bool(bool),
        visit_i8(i8),
        visit_i16(i16),
        visit_i32(i32),
        visit_i64(i64),
        visit_i128(i128),
        visit_u8(u8),
        visit_u16(u16),
        visit_u32(u32),
        visit_u64(u64),
        visit_u128(u128),
        visit_f32(f32),
        visit_f64(f64),
        visit_char(char)
    );
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.inner.visit_str(v)
    }
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        self.inner.visit_borrowed_str(v)
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        self.inner.visit_string(v)
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        self.inner.visit_bytes(v)
    }
    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        self.inner.visit_borrowed_bytes(v)
    }
    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        self.inner.visit_byte_buf(v)
    }
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }
    fn visit_some<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        self.inner
            .visit_some(Deserializer {
                inner: d,
                handles: self.handles,
                error: self.error,
            })
            .map_err(|err| stash_de_error(self.error, err))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }
    fn visit_newtype_struct<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        self.inner
            .visit_newtype_struct(Deserializer {
                inner: d,
                handles: self.handles,
                error: self.error,
            })
            .map_err(|err| stash_de_error(self.error, err))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_seq(SeqWrap {
            inner: a,
            handles: self.handles,
            error: self.error,
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_map(MapWrap {
            inner: a,
            handles: self.handles,
            error: self.error,
        })
    }
    fn visit_enum<A: EnumAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_enum(EnumWrap {
            inner: a,
            handles: self.handles,
            error: self.error,
        })
    }
}
impl<'de, A: SeqAccess<'de>, H: TakeHandle> SeqAccess<'de> for SeqWrap<'_, A, H> {
    type Error = A::Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, A::Error> {
        self.inner.next_element_seed(SeedWrap {
            inner: seed,
            handles: self.handles,
            error: self.error,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}
impl<'de, A: MapAccess<'de>, H: TakeHandle> MapAccess<'de> for MapWrap<'_, A, H> {
    type Error = A::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.inner.next_key_seed(SeedWrap {
            inner: seed,
            handles: self.handles,
            error: self.error,
        })
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, A::Error> {
        self.inner.next_value_seed(SeedWrap {
            inner: seed,
            handles: self.handles,
            error: self.error,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}
impl<'a, 'de, A: EnumAccess<'de>, H: TakeHandle> EnumAccess<'de> for EnumWrap<'a, A, H> {
    type Error = A::Error;
    type Variant = VariantWrap<'a, A::Variant, H>;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), A::Error> {
        let (v, a) = self.inner.variant_seed(SeedWrap {
            inner: seed,
            handles: self.handles,
            error: self.error,
        })?;
        Ok((
            v,
            VariantWrap {
                inner: a,
                handles: self.handles,
                error: self.error,
            },
        ))
    }
}
impl<'de, A: VariantAccess<'de>, H: TakeHandle> VariantAccess<'de> for VariantWrap<'_, A, H> {
    type Error = A::Error;
    fn unit_variant(self) -> Result<(), A::Error> {
        self.inner.unit_variant()
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, A::Error> {
        self.inner.newtype_variant_seed(SeedWrap {
            inner: seed,
            handles: self.handles,
            error: self.error,
        })
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, A::Error> {
        self.inner.tuple_variant(
            len,
            VisitorWrap {
                inner: v,
                handles: self.handles,
                error: self.error,
            },
        )
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, A::Error> {
        self.inner.struct_variant(
            fields,
            VisitorWrap {
                inner: v,
                handles: self.handles,
                error: self.error,
            },
        )
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{handle::OsHandle, transport::EncodeHandles};
    use ::serde::{Deserialize, Serialize, ser::SerializeStruct};
    use nix::unistd::pipe;
    use std::io;
    use std::os::fd::OwnedFd;

    struct Frame(Vec<i32>);
    impl<'a> PutHandle<'a> for Frame {
        fn put_handle(&mut self, _fd: &'a dyn crate::handle::ErasedHandle) -> io::Result<u32> {
            let index = self.0.len() as u32;
            self.0.push(index as i32);
            Ok(index)
        }
    }

    struct TestReceiver(Vec<Option<OwnedFd>>);
    impl TakeHandle for TestReceiver {
        fn take_handle(&mut self, index: u32) -> io::Result<OwnedFd> {
            self.0
                .get_mut(index as usize)
                .and_then(Option::take)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid fd"))
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Ordinary {
        Unit,
        Tuple(u32, String),
        Struct { values: Vec<u8> },
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Newtype(u64);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Compatibility {
        some: Option<Newtype>,
        none: Option<u8>,
        map: std::collections::BTreeMap<String, Ordinary>,
        variants: Vec<Ordinary>,
    }

    #[derive(Serialize)]
    struct OneHandle<'a> {
        handle: &'a OsHandle<OwnedFd>,
    }

    #[derive(Serialize)]
    struct RepeatedHandle<'a> {
        first: &'a OsHandle<OwnedFd>,
        second: &'a OsHandle<OwnedFd>,
    }

    struct FailsAfterHandle<'a>(&'a OsHandle<OwnedFd>);

    impl Serialize for FailsAfterHandle<'_> {
        fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut state = serializer.serialize_struct("FailsAfterHandle", 2)?;
            state.serialize_field("handle", self.0)?;
            Err(::serde::ser::Error::custom("intentional failure"))
        }
    }

    #[derive(Debug)]
    struct AlwaysFailsToDeserialize;

    impl<'de> Deserialize<'de> for AlwaysFailsToDeserialize {
        fn deserialize<D: ::serde::Deserializer<'de>>(_d: D) -> Result<Self, D::Error> {
            Err(::serde::de::Error::custom(
                "intentional deserialize failure",
            ))
        }
    }

    #[derive(Debug, Deserialize)]
    struct WrapsFailingField {
        #[allow(dead_code)]
        inner: AlwaysFailsToDeserialize,
    }

    #[test]
    fn ordinary_values_are_postcard_compatible() {
        let value = Compatibility {
            some: Some(Newtype(42)),
            none: None,
            map: [("key".into(), Ordinary::Tuple(7, "value".into()))]
                .into_iter()
                .collect(),
            variants: vec![
                Ordinary::Unit,
                Ordinary::Struct {
                    values: vec![1, 2, 3],
                },
            ],
        };
        let mut frame = Frame(Vec::new());
        let encoded = to_extend(&value, &mut frame, Vec::new()).unwrap();
        assert_eq!(encoded, postcard::to_stdvec(&value).unwrap());
        let mut handles = TestReceiver(Vec::new());
        assert_eq!(
            from_bytes::<Compatibility, _>(&encoded, &mut handles).unwrap(),
            value
        );
    }

    #[derive(Serialize)]
    struct Sending {
        handles: Option<Vec<OsHandle<OwnedFd>>>,
    }
    #[derive(Deserialize)]
    struct Receiving {
        handles: Option<Vec<OsHandle<OwnedFd>>>,
    }

    #[test]
    fn nested_handles_round_trip_through_context() {
        let (fd, _) = pipe().unwrap();
        let value = Sending {
            handles: Some(vec![OsHandle::new(fd)]),
        };
        let mut frame = Frame(Vec::new());
        let encoded = to_extend(&value, &mut frame, Vec::new()).unwrap();
        assert_eq!(frame.0.len(), 1);
        let mut handles = TestReceiver(vec![Some(
            value.handles.unwrap().pop().unwrap().into_inner(),
        )]);
        let decoded: Receiving = from_bytes(&encoded, &mut handles).unwrap();
        assert_eq!(decoded.handles.unwrap().len(), 1);
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut encoded = postcard::to_stdvec(&42u32).unwrap();
        encoded.push(0);
        let mut handles = TestReceiver(Vec::new());
        assert!(
            matches!(from_bytes::<u32, _>(&encoded, &mut handles), Err(Error::Message(message)) if message == "trailing bytes in payload")
        );
    }

    #[test]
    fn successful_serialization_steals_handle() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let value = OneHandle { handle: &handle };
        let mut handles = EncodeHandles::for_test(usize::MAX);
        encode_payload(&value, &mut handles).unwrap();
        let owned = handles.finish();
        assert_eq!(owned.fds.len(), 1);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { handle.into_inner() }))
                .is_err()
        );
    }

    #[test]
    fn failed_serialization_does_not_steal_handle() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let mut handles = EncodeHandles::for_test(usize::MAX);
        let error = encode_payload(&FailsAfterHandle(&handle), &mut handles).unwrap_err();
        // Regression check for the postcard-discards-custom-errors plumbing:
        // the message from the value's own `Serialize` impl must survive,
        // not just a generic "something went wrong" from postcard.
        assert!(error.to_string().contains("intentional failure"));
        drop(handle.into_inner());
    }

    #[test]
    fn custom_deserialize_error_message_is_preserved() {
        // Regression check for the postcard-discards-custom-errors plumbing,
        // deserialize side: the field's own `Deserialize` impl calls
        // `Error::custom` deep inside the recursive `SeedWrap` boundary, and
        // the message must still make it back to the caller.
        let mut handles = TestReceiver(Vec::new());
        let error = from_bytes::<WrapsFailingField, _>(&[0u8], &mut handles).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("intentional deserialize failure")
        );
    }

    #[test]
    fn handle_limit_is_checked_before_stealing() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let mut handles = EncodeHandles::for_test(0);
        assert!(encode_payload(&OneHandle { handle: &handle }, &mut handles).is_err());
        drop(handle.into_inner());
    }

    #[test]
    fn repeated_handle_is_rejected_without_being_stolen() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let value = RepeatedHandle {
            first: &handle,
            second: &handle,
        };
        let mut handles = EncodeHandles::for_test(usize::MAX);
        assert!(encode_payload(&value, &mut handles).is_err());
        drop(handle.into_inner());
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::io;
    use std::os::windows::io::{FromRawHandle, IntoRawHandle, OwnedHandle};

    use ::serde::{Deserialize, Serialize};

    use super::*;
    use crate::handle::OsHandle;

    struct Frame(Option<usize>);

    impl PutHandle<'_> for Frame {
        fn put_handle(&mut self, handle: &dyn crate::handle::ErasedHandle) -> io::Result<usize> {
            let raw = handle.raw_handle() as usize;
            self.0 = Some(raw);
            Ok(raw)
        }
    }

    struct TestReceiver(Option<OwnedHandle>);

    impl TakeHandle for TestReceiver {
        fn take_handle(&mut self, value: usize) -> io::Result<OwnedHandle> {
            use std::os::windows::io::AsRawHandle;
            let handle = self.0.take().unwrap();
            assert_eq!(handle.as_raw_handle() as usize, value);
            Ok(handle)
        }
    }

    #[derive(Serialize)]
    struct Sending {
        handle: OsHandle<OwnedHandle>,
    }

    #[derive(Deserialize)]
    struct Receiving {
        handle: OsHandle<OwnedHandle>,
    }

    #[test]
    fn handle_round_trips_through_context() {
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let value = Sending {
            handle: OsHandle::new(OwnedHandle::from(file)),
        };
        let mut frame = Frame(None);
        let encoded = to_extend(&value, &mut frame, Vec::new()).unwrap();
        let raw = value.handle.into_inner().into_raw_handle();
        assert_eq!(frame.0, Some(raw as usize));
        let mut receiver = TestReceiver(Some(unsafe { OwnedHandle::from_raw_handle(raw) }));
        let received: Receiving = from_bytes(&encoded, &mut receiver).unwrap();
        drop(received.handle);
    }
}
