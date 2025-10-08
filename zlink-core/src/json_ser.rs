//! JSON serializer adapted from serde_json::ser for no-std environments.
//!
//! This module is based on serde_json's serializer but modified to:
//! - Write directly to byte slices instead of using std::io::Write
//! - Work in no-std environments
//! - Use `to_slice` API instead of `to_writer`

use core::{
    fmt::{self, Display},
    hint,
    num::FpCategory,
    str,
};
use serde::ser::{self, Impossible, Serialize};

/// Serialize the given data structure as JSON into the provided byte slice.
///
/// Returns the number of bytes written.
pub(crate) fn to_slice<T>(value: &T, buf: &mut [u8]) -> Result<usize>
where
    T: ?Sized + Serialize,
{
    let mut ser = Serializer::new(buf);
    value.serialize(&mut ser)?;
    Ok(ser.writer.pos)
}

/// Errors that can occur during serialization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Error {
    /// Key must be a string.
    KeyMustBeAString,
    /// Buffer is too small to hold the serialized value.
    BufferTooSmall,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::KeyMustBeAString => write!(f, "key must be a string"),
            Error::BufferTooSmall => write!(f, "buffer too small"),
        }
    }
}

impl ser::Error for Error {
    fn custom<T: Display>(_msg: T) -> Self {
        // For simplicity in no-std, we don't support custom messages.
        // Default to KeyMustBeAString for generic errors.
        Error::KeyMustBeAString
    }
}

impl core::error::Error for Error {}

/// Result type for serialization operations.
pub(crate) type Result<T> = core::result::Result<T, Error>;

/// Adapted io::Result for our byte slice writer.
type IoResult<T> = core::result::Result<T, Error>;

/// A structure for serializing Rust values into JSON written to a byte slice.
#[derive(Debug)]
struct Serializer<'a, F = CompactFormatter> {
    writer: ByteSliceWriter<'a>,
    formatter: F,
}

impl<'a> Serializer<'a> {
    /// Creates a new JSON serializer.
    #[inline]
    fn new(writer: &'a mut [u8]) -> Self {
        Serializer::with_formatter(writer, CompactFormatter)
    }
}

impl<'a, F> Serializer<'a, F>
where
    F: Formatter,
{
    /// Creates a new JSON visitor whose output will be written to the writer specified.
    #[inline]
    fn with_formatter(writer: &'a mut [u8], formatter: F) -> Self {
        Serializer {
            writer: ByteSliceWriter::new(writer),
            formatter,
        }
    }
}

// Copied macro from serde_json
macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(val) => val,
            Err(err) => return Err(err),
        }
    };
}

impl<'a, 'b, F> ser::Serializer for &'a mut Serializer<'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Compound<'a, 'b, F>;
    type SerializeTuple = Compound<'a, 'b, F>;
    type SerializeTupleStruct = Compound<'a, 'b, F>;
    type SerializeTupleVariant = Compound<'a, 'b, F>;
    type SerializeMap = Compound<'a, 'b, F>;
    type SerializeStruct = Compound<'a, 'b, F>;
    type SerializeStructVariant = Compound<'a, 'b, F>;

    #[inline]
    fn serialize_bool(self, value: bool) -> Result<()> {
        self.formatter.write_bool(&mut self.writer, value)
    }

    #[inline]
    fn serialize_i8(self, value: i8) -> Result<()> {
        self.formatter.write_i8(&mut self.writer, value)
    }

    #[inline]
    fn serialize_i16(self, value: i16) -> Result<()> {
        self.formatter.write_i16(&mut self.writer, value)
    }

    #[inline]
    fn serialize_i32(self, value: i32) -> Result<()> {
        self.formatter.write_i32(&mut self.writer, value)
    }

    #[inline]
    fn serialize_i64(self, value: i64) -> Result<()> {
        self.formatter.write_i64(&mut self.writer, value)
    }

    fn serialize_i128(self, value: i128) -> Result<()> {
        self.formatter.write_i128(&mut self.writer, value)
    }

    #[inline]
    fn serialize_u8(self, value: u8) -> Result<()> {
        self.formatter.write_u8(&mut self.writer, value)
    }

    #[inline]
    fn serialize_u16(self, value: u16) -> Result<()> {
        self.formatter.write_u16(&mut self.writer, value)
    }

    #[inline]
    fn serialize_u32(self, value: u32) -> Result<()> {
        self.formatter.write_u32(&mut self.writer, value)
    }

    #[inline]
    fn serialize_u64(self, value: u64) -> Result<()> {
        self.formatter.write_u64(&mut self.writer, value)
    }

    fn serialize_u128(self, value: u128) -> Result<()> {
        self.formatter.write_u128(&mut self.writer, value)
    }

    #[inline]
    fn serialize_f32(self, value: f32) -> Result<()> {
        match value.classify() {
            FpCategory::Nan | FpCategory::Infinite => self.formatter.write_null(&mut self.writer),
            _ => self.formatter.write_f32(&mut self.writer, value),
        }
    }

    #[inline]
    fn serialize_f64(self, value: f64) -> Result<()> {
        match value.classify() {
            FpCategory::Nan | FpCategory::Infinite => self.formatter.write_null(&mut self.writer),
            _ => self.formatter.write_f64(&mut self.writer, value),
        }
    }

    #[inline]
    fn serialize_char(self, value: char) -> Result<()> {
        // A char encoded as UTF-8 takes 4 bytes at most.
        let mut buf = [0; 4];
        self.serialize_str(value.encode_utf8(&mut buf))
    }

    #[inline]
    fn serialize_str(self, value: &str) -> Result<()> {
        format_escaped_str(&mut self.writer, &mut self.formatter, value)
    }

    #[inline]
    fn serialize_bytes(self, value: &[u8]) -> Result<()> {
        self.formatter.write_byte_array(&mut self.writer, value)
    }

    #[inline]
    fn serialize_unit(self) -> Result<()> {
        self.formatter.write_null(&mut self.writer)
    }

    #[inline]
    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        self.serialize_unit()
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<()> {
        self.serialize_str(variant)
    }

    #[inline]
    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        tri!(self.formatter.begin_object(&mut self.writer));
        tri!(self.formatter.begin_object_key(&mut self.writer, true));
        tri!(self.serialize_str(variant));
        tri!(self.formatter.end_object_key(&mut self.writer));
        tri!(self.formatter.begin_object_value(&mut self.writer));
        tri!(value.serialize(&mut *self));
        tri!(self.formatter.end_object_value(&mut self.writer));
        self.formatter.end_object(&mut self.writer)
    }

    #[inline]
    fn serialize_none(self) -> Result<()> {
        self.serialize_unit()
    }

    #[inline]
    fn serialize_some<T>(self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        tri!(self.formatter.begin_array(&mut self.writer));
        if len == Some(0) {
            tri!(self.formatter.end_array(&mut self.writer));
            Ok(Compound::Map {
                ser: self,
                state: State::Empty,
            })
        } else {
            Ok(Compound::Map {
                ser: self,
                state: State::First,
            })
        }
    }

    #[inline]
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        tri!(self.formatter.begin_object(&mut self.writer));
        tri!(self.formatter.begin_object_key(&mut self.writer, true));
        tri!(self.serialize_str(variant));
        tri!(self.formatter.end_object_key(&mut self.writer));
        tri!(self.formatter.begin_object_value(&mut self.writer));
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        tri!(self.formatter.begin_object(&mut self.writer));
        if len == Some(0) {
            tri!(self.formatter.end_object(&mut self.writer));
            Ok(Compound::Map {
                ser: self,
                state: State::Empty,
            })
        } else {
            Ok(Compound::Map {
                ser: self,
                state: State::First,
            })
        }
    }

    #[inline]
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }

    #[inline]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        tri!(self.formatter.begin_object(&mut self.writer));
        tri!(self.formatter.begin_object_key(&mut self.writer, true));
        tri!(self.serialize_str(variant));
        tri!(self.formatter.end_object_key(&mut self.writer));
        tri!(self.formatter.begin_object_value(&mut self.writer));
        self.serialize_map(Some(len))
    }
}

/// A writer that writes to a byte slice.
#[derive(Debug)]
struct ByteSliceWriter<'a> {
    output: &'a mut [u8],
    pos: usize,
}

impl<'a> ByteSliceWriter<'a> {
    fn new(output: &'a mut [u8]) -> Self {
        ByteSliceWriter { output, pos: 0 }
    }

    fn write_all(&mut self, buf: &[u8]) -> IoResult<()> {
        if self.pos + buf.len() > self.output.len() {
            return Err(Error::BufferTooSmall);
        }
        self.output[self.pos..self.pos + buf.len()].copy_from_slice(buf);
        self.pos += buf.len();
        Ok(())
    }
}

/// Represents the state of a compound serialization.
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Empty,
    First,
    Rest,
}

/// Compound serialization helper.
#[derive(Debug)]
enum Compound<'a, 'b, F> {
    Map {
        ser: &'a mut Serializer<'b, F>,
        state: State,
    },
}

impl<'a, 'b, F> ser::SerializeSeq for Compound<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::Map { ser, state } => {
                tri!(ser
                    .formatter
                    .begin_array_value(&mut ser.writer, *state == State::First));
                *state = State::Rest;
                tri!(value.serialize(&mut **ser));
                ser.formatter.end_array_value(&mut ser.writer)
            }
        }
    }

    #[inline]
    fn end(self) -> Result<()> {
        match self {
            Compound::Map { ser, state } => match state {
                State::Empty => Ok(()),
                _ => ser.formatter.end_array(&mut ser.writer),
            },
        }
    }
}

impl<'a, 'b, F> ser::SerializeTuple for Compound<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    #[inline]
    fn end(self) -> Result<()> {
        ser::SerializeSeq::end(self)
    }
}

impl<'a, 'b, F> ser::SerializeTupleStruct for Compound<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    #[inline]
    fn end(self) -> Result<()> {
        ser::SerializeSeq::end(self)
    }
}

impl<'a, 'b, F> ser::SerializeTupleVariant for Compound<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    #[inline]
    fn end(self) -> Result<()> {
        match self {
            Compound::Map { ser, state } => {
                match state {
                    State::Empty => {}
                    _ => tri!(ser.formatter.end_array(&mut ser.writer)),
                }
                tri!(ser.formatter.end_object_value(&mut ser.writer));
                ser.formatter.end_object(&mut ser.writer)
            }
        }
    }
}

impl<'a, 'b, F> ser::SerializeMap for Compound<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_key<T>(&mut self, key: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::Map { ser, state } => {
                tri!(ser
                    .formatter
                    .begin_object_key(&mut ser.writer, *state == State::First));
                *state = State::Rest;

                tri!(key.serialize(MapKeySerializer { ser: *ser }));

                ser.formatter.end_object_key(&mut ser.writer)
            }
        }
    }

    #[inline]
    fn serialize_value<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::Map { ser, .. } => {
                tri!(ser.formatter.begin_object_value(&mut ser.writer));
                tri!(value.serialize(&mut **ser));
                ser.formatter.end_object_value(&mut ser.writer)
            }
        }
    }

    #[inline]
    fn end(self) -> Result<()> {
        match self {
            Compound::Map { ser, state } => match state {
                State::Empty => Ok(()),
                _ => ser.formatter.end_object(&mut ser.writer),
            },
        }
    }
}

impl<'a, 'b, F> ser::SerializeStruct for Compound<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeMap::serialize_entry(self, key, value)
    }

    #[inline]
    fn end(self) -> Result<()> {
        ser::SerializeMap::end(self)
    }
}

impl<'a, 'b, F> ser::SerializeStructVariant for Compound<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeMap::serialize_entry(self, key, value)
    }

    #[inline]
    fn end(self) -> Result<()> {
        match self {
            Compound::Map { ser, state } => {
                match state {
                    State::Empty => {}
                    _ => tri!(ser.formatter.end_object(&mut ser.writer)),
                }
                tri!(ser.formatter.end_object_value(&mut ser.writer));
                ser.formatter.end_object(&mut ser.writer)
            }
        }
    }
}

/// Serializer for map keys.
struct MapKeySerializer<'a, 'b, F> {
    ser: &'a mut Serializer<'b, F>,
}

impl<'a, 'b, F> ser::Serializer for MapKeySerializer<'a, 'b, F>
where
    F: Formatter,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Impossible<(), Error>;
    type SerializeTuple = Impossible<(), Error>;
    type SerializeTupleStruct = Impossible<(), Error>;
    type SerializeTupleVariant = Impossible<(), Error>;
    type SerializeMap = Impossible<(), Error>;
    type SerializeStruct = Impossible<(), Error>;
    type SerializeStructVariant = Impossible<(), Error>;

    #[inline]
    fn serialize_str(self, value: &str) -> Result<()> {
        format_escaped_str(&mut self.ser.writer, &mut self.ser.formatter, value)
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<()> {
        self.serialize_str(variant)
    }

    #[inline]
    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_bool(self, _value: bool) -> Result<()> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_i8(self, value: i8) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_i8(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_i16(self, value: i16) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_i16(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_i32(self, value: i32) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_i32(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_i64(self, value: i64) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_i64(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_i128(self, value: i128) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_i128(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_u8(self, value: u8) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_u8(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_u16(self, value: u16) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_u16(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_u32(self, value: u32) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_u32(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_u64(self, value: u64) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_u64(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_u128(self, value: u128) -> Result<()> {
        tri!(self.ser.formatter.begin_string(&mut self.ser.writer));
        tri!(self.ser.formatter.write_u128(&mut self.ser.writer, value));
        self.ser.formatter.end_string(&mut self.ser.writer)
    }

    fn serialize_f32(self, _value: f32) -> Result<()> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_f64(self, _value: f64) -> Result<()> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_char(self, value: char) -> Result<()> {
        // Encode char directly without allocation, like serde_json does
        let mut buf = [0u8; 4];
        self.serialize_str(value.encode_utf8(&mut buf))
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<()> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_unit(self) -> Result<()> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_none(self) -> Result<()> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_some<T>(self, _value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Err(Error::KeyMustBeAString)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(Error::KeyMustBeAString)
    }
}

/// CharEscape represents a character escape sequence.
#[derive(Debug)]
enum CharEscape {
    /// An escaped quote `"`
    Quote,
    /// An escaped reverse solidus `\`
    ReverseSolidus,
    /// An escaped solidus `/`
    #[allow(dead_code)] // Optional escape in JSON, not in ESCAPE table
    Solidus,
    /// An escaped backspace character (usually escaped as `\b`)
    Backspace,
    /// An escaped form feed character (usually escaped as `\f`)
    FormFeed,
    /// An escaped line feed character (usually escaped as `\n`)
    LineFeed,
    /// An escaped carriage return character (usually escaped as `\r`)
    CarriageReturn,
    /// An escaped tab character (usually escaped as `\t`)
    Tab,
    /// An escaped ASCII plane control character (usually escaped as
    /// `\u00XX` where `XX` are two hex characters)
    AsciiControl(u8),
}

/// This trait abstracts away serializing the JSON control characters.
trait Formatter {
    /// Writes a `null` value to the specified writer.
    #[inline]
    fn write_null<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b"null")
    }

    /// Writes a `true` or `false` value to the specified writer.
    #[inline]
    fn write_bool<W>(&mut self, mut writer: &mut W, value: bool) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let s = if value {
            b"true" as &[u8]
        } else {
            b"false" as &[u8]
        };
        writer.write_all(s)
    }

    /// Writes an integer value like `-123` to the specified writer.
    #[inline]
    fn write_i8<W>(&mut self, mut writer: &mut W, value: i8) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `-123` to the specified writer.
    #[inline]
    fn write_i16<W>(&mut self, mut writer: &mut W, value: i16) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `-123` to the specified writer.
    #[inline]
    fn write_i32<W>(&mut self, mut writer: &mut W, value: i32) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `-123` to the specified writer.
    #[inline]
    fn write_i64<W>(&mut self, mut writer: &mut W, value: i64) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `-123` to the specified writer.
    #[inline]
    fn write_i128<W>(&mut self, mut writer: &mut W, value: i128) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `123` to the specified writer.
    #[inline]
    fn write_u8<W>(&mut self, mut writer: &mut W, value: u8) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `123` to the specified writer.
    #[inline]
    fn write_u16<W>(&mut self, mut writer: &mut W, value: u16) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `123` to the specified writer.
    #[inline]
    fn write_u32<W>(&mut self, mut writer: &mut W, value: u32) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `123` to the specified writer.
    #[inline]
    fn write_u64<W>(&mut self, mut writer: &mut W, value: u64) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes an integer value like `123` to the specified writer.
    #[inline]
    fn write_u128<W>(&mut self, mut writer: &mut W, value: u128) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes a floating point value like `-31.26e+12` to the specified writer.
    #[inline]
    fn write_f32<W>(&mut self, mut writer: &mut W, value: f32) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = ryu::Buffer::new();
        let s = buffer.format_finite(value);
        writer.write_all(s.as_bytes())
    }

    /// Writes a floating point value like `-31.26e+12` to the specified writer.
    #[inline]
    fn write_f64<W>(&mut self, mut writer: &mut W, value: f64) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        let mut buffer = ryu::Buffer::new();
        let s = buffer.format_finite(value);
        writer.write_all(s.as_bytes())
    }

    /// Called before each series of `write_string_fragment` and `write_char_escape`.
    #[inline]
    fn begin_string<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b"\"")
    }

    /// Called after each series of `write_string_fragment` and `write_char_escape`.
    #[inline]
    fn end_string<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b"\"")
    }

    /// Writes a string fragment that doesn't need any escaping to the specified writer.
    #[inline]
    fn write_string_fragment<W>(&mut self, mut writer: &mut W, fragment: &str) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(fragment.as_bytes())
    }

    /// Writes a character escape code to the specified writer.
    #[inline]
    fn write_char_escape<W>(&mut self, mut writer: &mut W, char_escape: CharEscape) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        match char_escape {
            CharEscape::Quote => writer.write_all(b"\\\""),
            CharEscape::ReverseSolidus => writer.write_all(b"\\\\"),
            CharEscape::Solidus => writer.write_all(b"\\/"),
            CharEscape::Backspace => writer.write_all(b"\\b"),
            CharEscape::FormFeed => writer.write_all(b"\\f"),
            CharEscape::LineFeed => writer.write_all(b"\\n"),
            CharEscape::CarriageReturn => writer.write_all(b"\\r"),
            CharEscape::Tab => writer.write_all(b"\\t"),
            CharEscape::AsciiControl(byte) => {
                const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";
                writer.write_all(b"\\u00")?;
                writer.write_all(&[HEX_DIGITS[(byte >> 4) as usize]])?;
                writer.write_all(&[HEX_DIGITS[(byte & 0xF) as usize]])
            }
        }
    }

    /// Writes the representation of a byte array.
    fn write_byte_array<W>(&mut self, writer: &mut W, value: &[u8]) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        tri!(self.begin_array(writer));
        let mut first = true;
        for byte in value {
            tri!(self.begin_array_value(writer, first));
            tri!(self.write_u8(writer, *byte));
            tri!(self.end_array_value(writer));
            first = false;
        }
        self.end_array(writer)
    }

    /// Called before every array. Writes a `[` to the specified writer.
    #[inline]
    fn begin_array<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b"[")
    }

    /// Called after every array. Writes a `]` to the specified writer.
    #[inline]
    fn end_array<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b"]")
    }

    /// Called before every array value. Writes a `,` if needed to the specified writer.
    #[inline]
    fn begin_array_value<W>(&mut self, mut writer: &mut W, first: bool) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b",")
        }
    }

    /// Called after every array value.
    #[inline]
    fn end_array_value<W>(&mut self, _writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        Ok(())
    }

    /// Called before every object. Writes a `{` to the specified writer.
    #[inline]
    fn begin_object<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b"{")
    }

    /// Called after every object. Writes a `}` to the specified writer.
    #[inline]
    fn end_object<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b"}")
    }

    /// Called before every object key.
    #[inline]
    fn begin_object_key<W>(&mut self, mut writer: &mut W, first: bool) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b",")
        }
    }

    /// Called after every object key. A `:` should be written to the specified writer.
    #[inline]
    fn end_object_key<W>(&mut self, _writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        Ok(())
    }

    /// Called before every object value. A `:` should be written to the specified writer.
    #[inline]
    fn begin_object_value<W>(&mut self, mut writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        writer.write_all(b":")
    }

    /// Called after every object value.
    #[inline]
    fn end_object_value<W>(&mut self, _writer: &mut W) -> IoResult<()>
    where
        W: ?Sized,
        for<'a> &'a mut W: Write,
    {
        Ok(())
    }
}

/// This structure compacts a JSON value with no extra whitespace.
#[derive(Clone, Debug, Default)]
struct CompactFormatter;

impl Formatter for CompactFormatter {}

// Trait to abstract the writer interface.
trait Write {
    fn write_all(&mut self, buf: &[u8]) -> IoResult<()>;
}

impl Write for &mut ByteSliceWriter<'_> {
    fn write_all(&mut self, buf: &[u8]) -> IoResult<()> {
        (**self).write_all(buf)
    }
}

// Constants from serde_json for character escaping.
const BB: u8 = b'b'; // \x08
const TT: u8 = b't'; // \x09
const NN: u8 = b'n'; // \x0A
const FF: u8 = b'f'; // \x0C
const RR: u8 = b'r'; // \x0D
const QU: u8 = b'"'; // \x22
const BS: u8 = b'\\'; // \x5C
const UU: u8 = b'u'; // \x00...\x1F except the ones above
const __: u8 = 0;

// Lookup table of escape sequences. A value of b'x' at index i means that byte
// i is escaped as "\x" in JSON. A value of 0 means that byte i is not escaped.
static ESCAPE: [u8; 256] = [
    //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    UU, UU, UU, UU, UU, UU, UU, UU, BB, TT, NN, UU, FF, RR, UU, UU, // 0
    UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, // 1
    __, __, QU, __, __, __, __, __, __, __, __, __, __, __, __, __, // 2
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 3
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 4
    __, __, __, __, __, __, __, __, __, __, __, __, BS, __, __, __, // 5
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 6
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 7
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 8
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 9
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // A
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // B
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // C
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // D
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // E
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // F
];

fn format_escaped_str<W, F>(writer: &mut W, formatter: &mut F, value: &str) -> IoResult<()>
where
    W: ?Sized,
    for<'a> &'a mut W: Write,
    F: ?Sized + Formatter,
{
    tri!(formatter.begin_string(writer));
    tri!(format_escaped_str_contents(writer, formatter, value));
    formatter.end_string(writer)
}

fn format_escaped_str_contents<W, F>(writer: &mut W, formatter: &mut F, value: &str) -> IoResult<()>
where
    W: ?Sized,
    for<'a> &'a mut W: Write,
    F: ?Sized + Formatter,
{
    let mut bytes = value.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        let (string_run, rest) = bytes.split_at(i);
        let (&byte, rest) = rest.split_first().unwrap();

        let escape = ESCAPE[byte as usize];

        i += 1;
        if escape == 0 {
            continue;
        }

        bytes = rest;
        i = 0;

        // Safety: string_run is a valid utf8 string, since we only split on ascii sequences.
        let string_run = unsafe { str::from_utf8_unchecked(string_run) };
        if !string_run.is_empty() {
            tri!(formatter.write_string_fragment(writer, string_run));
        }

        let char_escape = match escape {
            self::BB => CharEscape::Backspace,
            self::TT => CharEscape::Tab,
            self::NN => CharEscape::LineFeed,
            self::FF => CharEscape::FormFeed,
            self::RR => CharEscape::CarriageReturn,
            self::QU => CharEscape::Quote,
            self::BS => CharEscape::ReverseSolidus,
            self::UU => CharEscape::AsciiControl(byte),
            // Safety: the escape table does not contain any other type of character.
            _ => unsafe { hint::unreachable_unchecked() },
        };
        tri!(formatter.write_char_escape(writer, char_escape));
    }

    // Safety: bytes is a valid utf8 string, since we only split on ascii sequences.
    let string_run = unsafe { str::from_utf8_unchecked(bytes) };
    if string_run.is_empty() {
        return Ok(());
    }

    formatter.write_string_fragment(writer, string_run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[test]
    fn test_serialize_primitives() {
        let mut buf = [0u8; 128];

        let n = to_slice(&true, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"true");

        let n = to_slice(&false, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"false");

        let n = to_slice(&42i32, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"42");

        let n = to_slice(&-42i64, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"-42");

        let n = to_slice(&3.14f64, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"3.14");

        let n = to_slice(&"hello", &mut buf).unwrap();
        assert_eq!(&buf[..n], b"\"hello\"");
    }

    #[test]
    fn test_serialize_struct() {
        #[derive(Serialize)]
        struct Test {
            name: &'static str,
            value: i32,
        }

        let test = Test {
            name: "test",
            value: 42,
        };

        let mut buf = [0u8; 128];
        let n = to_slice(&test, &mut buf).unwrap();
        let json = core::str::from_utf8(&buf[..n]).unwrap();
        assert_eq!(json, r#"{"name":"test","value":42}"#);
    }

    #[test]
    fn test_serialize_array() {
        let arr = [1, 2, 3, 4, 5];
        let mut buf = [0u8; 128];
        let n = to_slice(&arr, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"[1,2,3,4,5]");
    }

    #[test]
    fn test_serialize_escape() {
        let s = "hello\n\"world\"";
        let mut buf = [0u8; 128];
        let n = to_slice(&s, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"\"hello\\n\\\"world\\\"\"");
    }
}
