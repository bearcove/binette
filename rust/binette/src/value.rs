use thiserror::Error;

use crate::compact::{CompactError, CompactReader};

const TAG_UNIT: u8 = 0x00;
const TAG_BOOL: u8 = 0x01;
const TAG_U8: u8 = 0x02;
const TAG_U16: u8 = 0x03;
const TAG_U32: u8 = 0x04;
const TAG_U64: u8 = 0x05;
const TAG_U128: u8 = 0x06;
const TAG_I8: u8 = 0x07;
const TAG_I16: u8 = 0x08;
const TAG_I32: u8 = 0x09;
const TAG_I64: u8 = 0x0A;
const TAG_I128: u8 = 0x0B;
const TAG_F32: u8 = 0x0C;
const TAG_F64: u8 = 0x0D;
const TAG_CHAR: u8 = 0x0E;
const TAG_STRING: u8 = 0x0F;
const TAG_BYTES: u8 = 0x10;
const TAG_PAYLOAD: u8 = 0x11;
const TAG_LIST: u8 = 0x12;
const TAG_SET: u8 = 0x13;
const TAG_MAP: u8 = 0x14;
const TAG_ARRAY: u8 = 0x15;
const TAG_TUPLE: u8 = 0x16;
const TAG_STRUCT: u8 = 0x17;
const TAG_ENUM: u8 = 0x18;
const TAG_OPTION_NONE: u8 = 0x19;
const TAG_OPTION_SOME: u8 = 0x1A;
const TAG_DYNAMIC: u8 = 0x1B;
const FIRST_EXTENSION_TAG: u8 = 0x80;

// r[impl binette.value-model]
// r[impl binette.value-kind.preserved]
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    Bytes(Vec<u8>),
    Payload(Vec<u8>),
    List(Vec<Value>),
    Set(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Array(ArrayValue),
    Tuple(Vec<Value>),
    Struct(Vec<FieldValue>),
    Enum(EnumValue),
    Option(Option<Box<Value>>),
    Dynamic(Box<Value>),
    // r[impl binette.value-model.external-form]
    ExternalAttachment,
    // r[impl binette.value-model.extension-form]
    Extension(ExtensionValue),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayValue {
    pub dimensions: Vec<u64>,
    pub elements: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldValue {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumValue {
    pub variant: String,
    pub payload: Box<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionValue {
    pub tag: u8,
    pub id: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum SelfDescribingError {
    #[error(transparent)]
    Compact(#[from] CompactError),

    #[error(
        "trailing bytes after self-described value at byte {position}: {remaining} bytes remain"
    )]
    TrailingBytes { position: usize, remaining: usize },

    #[error("reserved self-describing tag {tag:#04x} at byte {position}")]
    ReservedTag { position: usize, tag: u8 },

    #[error("length {len} exceeds u32::MAX")]
    LengthOverflow { len: usize },

    #[error("array rank must be at least one")]
    ArrayRankZero,

    #[error("array element count overflows usize")]
    ArrayElementCountOverflow,

    #[error("array dimensions require {expected} elements, value has {actual}")]
    ArrayElementCountMismatch { expected: usize, actual: usize },

    #[error("tuple must contain at least one element")]
    EmptyTuple,

    #[error("extension tag must be in 0x80..0xFF, got {tag:#04x}")]
    InvalidExtensionTag { tag: u8 },

    #[error("NaN is not a valid {aggregate} key payload")]
    NanCanonicalKey { aggregate: &'static str },

    #[error("{aggregate} entries are not in canonical byte order")]
    NonCanonicalOrder { aggregate: &'static str },

    #[error("{aggregate} contains duplicate canonical key bytes")]
    DuplicateCanonicalKey { aggregate: &'static str },

    #[error("external attachment values do not have a core self-describing tag")]
    ExternalAttachmentNotSelfDescribing,
}

// r[impl binette.forms]
// r[impl binette.mode.self-describing]
pub fn encode_self_described_to_vec(value: &Value) -> Result<Vec<u8>, SelfDescribingError> {
    let mut out = Vec::new();
    encode_self_described_into(&mut out, value)?;
    Ok(out)
}

// r[impl binette.aggregate.dynamic-value]
pub fn encode_dynamic_value_to_vec(value: &Value) -> Result<Vec<u8>, SelfDescribingError> {
    encode_self_described_to_vec(value)
}

// r[impl binette.forms]
// r[impl binette.mode.self-describing]
pub fn decode_self_described_from_slice(input: &[u8]) -> Result<Value, SelfDescribingError> {
    let mut reader = CompactReader::new(input);
    let value = decode_self_described_value(&mut reader)?;
    if !reader.is_empty() {
        return Err(SelfDescribingError::TrailingBytes {
            position: reader.position(),
            remaining: reader.remaining(),
        });
    }
    Ok(value)
}

// r[impl binette.aggregate.dynamic-value]
pub fn decode_dynamic_value_from_slice(input: &[u8]) -> Result<Value, SelfDescribingError> {
    decode_self_described_from_slice(input)
}

pub(crate) fn decode_dynamic_value_from_reader(
    reader: &mut CompactReader<'_>,
) -> Result<Value, SelfDescribingError> {
    decode_self_described_value(reader)
}

// r[impl binette.tags]
// r[impl binette.tags.scalar-payload]
// r[impl binette.tags.aggregate-payload]
fn encode_self_described_into(out: &mut Vec<u8>, value: &Value) -> Result<(), SelfDescribingError> {
    match value {
        Value::Unit => out.push(TAG_UNIT),
        Value::Bool(value) => {
            out.push(TAG_BOOL);
            out.push(u8::from(*value));
        }
        Value::U8(value) => {
            out.push(TAG_U8);
            out.push(*value);
        }
        Value::U16(value) => {
            out.push(TAG_U16);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::U32(value) => {
            out.push(TAG_U32);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::U64(value) => {
            out.push(TAG_U64);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::U128(value) => {
            out.push(TAG_U128);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::I8(value) => {
            out.push(TAG_I8);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::I16(value) => {
            out.push(TAG_I16);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::I32(value) => {
            out.push(TAG_I32);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::I64(value) => {
            out.push(TAG_I64);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::I128(value) => {
            out.push(TAG_I128);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::F32(value) => {
            out.push(TAG_F32);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::F64(value) => {
            out.push(TAG_F64);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::Char(value) => {
            out.push(TAG_CHAR);
            out.extend_from_slice(&u32::from(*value).to_le_bytes());
        }
        Value::String(value) => {
            out.push(TAG_STRING);
            encode_bytes(out, value.as_bytes())?;
        }
        Value::Bytes(value) => {
            out.push(TAG_BYTES);
            encode_bytes(out, value)?;
        }
        Value::Payload(value) => {
            out.push(TAG_PAYLOAD);
            encode_bytes(out, value)?;
        }
        Value::List(elements) => {
            out.push(TAG_LIST);
            write_u32(out, elements.len())?;
            for element in elements {
                encode_self_described_into(out, element)?;
            }
        }
        Value::Set(elements) => encode_set(out, elements)?,
        Value::Map(entries) => encode_map(out, entries)?,
        Value::Array(array) => encode_array(out, array)?,
        Value::Tuple(elements) => {
            if elements.is_empty() {
                return Err(SelfDescribingError::EmptyTuple);
            }
            out.push(TAG_TUPLE);
            write_u32(out, elements.len())?;
            for element in elements {
                encode_self_described_into(out, element)?;
            }
        }
        Value::Struct(fields) => encode_struct(out, fields)?,
        Value::Enum(value) => encode_enum(out, value)?,
        Value::Option(None) => out.push(TAG_OPTION_NONE),
        Value::Option(Some(value)) => {
            out.push(TAG_OPTION_SOME);
            encode_self_described_into(out, value)?;
        }
        Value::Dynamic(value) => {
            out.push(TAG_DYNAMIC);
            encode_self_described_into(out, value)?;
        }
        Value::ExternalAttachment => {
            return Err(SelfDescribingError::ExternalAttachmentNotSelfDescribing);
        }
        Value::Extension(extension) => encode_extension(out, extension)?,
    }
    Ok(())
}

// r[impl binette.aggregate.set]
// r[impl binette.aggregate.set-map.canonical]
// r[impl binette.aggregate.set-map.float-keys]
fn encode_set(out: &mut Vec<u8>, elements: &[Value]) -> Result<(), SelfDescribingError> {
    let mut encoded = Vec::with_capacity(elements.len());
    for element in elements {
        reject_nan_key(element, "set")?;
        encoded.push(encode_self_described_to_vec(element)?);
    }

    encoded.sort();
    reject_duplicate_bytes("set", encoded.iter().map(Vec::as_slice))?;

    out.push(TAG_SET);
    write_u32(out, encoded.len())?;
    for element in encoded {
        out.extend_from_slice(&element);
    }
    Ok(())
}

// r[impl binette.aggregate.map]
// r[impl binette.aggregate.set-map.canonical]
// r[impl binette.aggregate.set-map.float-keys]
fn encode_map(out: &mut Vec<u8>, entries: &[(Value, Value)]) -> Result<(), SelfDescribingError> {
    let mut encoded = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        reject_nan_key(key, "map")?;
        encoded.push((
            encode_self_described_to_vec(key)?,
            encode_self_described_to_vec(value)?,
        ));
    }

    encoded.sort_by(|left, right| left.0.cmp(&right.0));
    reject_duplicate_bytes("map", encoded.iter().map(|entry| entry.0.as_slice()))?;

    out.push(TAG_MAP);
    write_u32(out, encoded.len())?;
    for (key, value) in encoded {
        out.extend_from_slice(&key);
        out.extend_from_slice(&value);
    }
    Ok(())
}

// r[impl binette.aggregate.array]
fn encode_array(out: &mut Vec<u8>, array: &ArrayValue) -> Result<(), SelfDescribingError> {
    let count = array_element_count(&array.dimensions)?;
    if count != array.elements.len() {
        return Err(SelfDescribingError::ArrayElementCountMismatch {
            expected: count,
            actual: array.elements.len(),
        });
    }

    out.push(TAG_ARRAY);
    write_u32(out, array.dimensions.len())?;
    for dimension in &array.dimensions {
        out.extend_from_slice(&dimension.to_le_bytes());
    }
    for element in &array.elements {
        encode_self_described_into(out, element)?;
    }
    Ok(())
}

// r[impl binette.aggregate.struct.self-describing]
fn encode_struct(out: &mut Vec<u8>, fields: &[FieldValue]) -> Result<(), SelfDescribingError> {
    out.push(TAG_STRUCT);
    write_u32(out, fields.len())?;
    for field in fields {
        encode_bytes(out, field.name.as_bytes())?;
        encode_self_described_into(out, &field.value)?;
    }
    Ok(())
}

// r[impl binette.aggregate.enum.self-describing]
fn encode_enum(out: &mut Vec<u8>, value: &EnumValue) -> Result<(), SelfDescribingError> {
    out.push(TAG_ENUM);
    encode_bytes(out, value.variant.as_bytes())?;
    encode_self_described_into(out, &value.payload)
}

// r[impl binette.tags.extension]
// r[impl binette.tags.forward-contract]
fn encode_extension(
    out: &mut Vec<u8>,
    extension: &ExtensionValue,
) -> Result<(), SelfDescribingError> {
    if extension.tag < FIRST_EXTENSION_TAG {
        return Err(SelfDescribingError::InvalidExtensionTag { tag: extension.tag });
    }
    out.push(extension.tag);
    out.extend_from_slice(&extension.id.to_le_bytes());
    write_u32(out, extension.payload.len())?;
    out.extend_from_slice(&extension.payload);
    Ok(())
}

fn decode_self_described_value(
    reader: &mut CompactReader<'_>,
) -> Result<Value, SelfDescribingError> {
    let position = reader.position();
    let tag = reader.read_u8()?;
    match tag {
        TAG_UNIT => Ok(Value::Unit),
        TAG_BOOL => Ok(Value::Bool(reader.read_bool()?)),
        TAG_U8 => Ok(Value::U8(reader.read_u8()?)),
        TAG_U16 => Ok(Value::U16(reader.read_u16()?)),
        TAG_U32 => Ok(Value::U32(reader.read_u32()?)),
        TAG_U64 => Ok(Value::U64(reader.read_u64()?)),
        TAG_U128 => Ok(Value::U128(reader.read_u128()?)),
        TAG_I8 => Ok(Value::I8(reader.read_i8()?)),
        TAG_I16 => Ok(Value::I16(reader.read_i16()?)),
        TAG_I32 => Ok(Value::I32(reader.read_i32()?)),
        TAG_I64 => Ok(Value::I64(reader.read_i64()?)),
        TAG_I128 => Ok(Value::I128(reader.read_i128()?)),
        TAG_F32 => Ok(Value::F32(reader.read_f32()?)),
        TAG_F64 => Ok(Value::F64(reader.read_f64()?)),
        TAG_CHAR => Ok(Value::Char(reader.read_char()?)),
        TAG_STRING => Ok(Value::String(reader.read_string()?)),
        TAG_BYTES => Ok(Value::Bytes(reader.read_byte_vec()?)),
        TAG_PAYLOAD => Ok(Value::Payload(reader.read_byte_vec()?)),
        TAG_LIST => decode_list(reader),
        TAG_SET => decode_set(reader),
        TAG_MAP => decode_map(reader),
        TAG_ARRAY => decode_array(reader),
        TAG_TUPLE => decode_tuple(reader),
        TAG_STRUCT => decode_struct(reader),
        TAG_ENUM => decode_enum(reader),
        TAG_OPTION_NONE => Ok(Value::Option(None)),
        TAG_OPTION_SOME => Ok(Value::Option(Some(Box::new(decode_self_described_value(
            reader,
        )?)))),
        TAG_DYNAMIC => Ok(Value::Dynamic(Box::new(decode_self_described_value(
            reader,
        )?))),
        0x1C..=0x7F => Err(SelfDescribingError::ReservedTag { position, tag }),
        FIRST_EXTENSION_TAG..=u8::MAX => decode_extension(reader, tag),
    }
}

fn decode_list(reader: &mut CompactReader<'_>) -> Result<Value, SelfDescribingError> {
    let count = reader.read_u32()? as usize;
    let mut elements = Vec::with_capacity(count);
    for _ in 0..count {
        elements.push(decode_self_described_value(reader)?);
    }
    Ok(Value::List(elements))
}

fn decode_set(reader: &mut CompactReader<'_>) -> Result<Value, SelfDescribingError> {
    let count = reader.read_u32()? as usize;
    let mut elements = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let start = reader.position();
        let element = decode_self_described_value(reader)?;
        reject_nan_key(&element, "set")?;
        validate_canonical_bytes(reader, &mut previous, start, "set")?;
        elements.push(element);
    }
    Ok(Value::Set(elements))
}

fn decode_map(reader: &mut CompactReader<'_>) -> Result<Value, SelfDescribingError> {
    let count = reader.read_u32()? as usize;
    let mut entries = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let key_start = reader.position();
        let key = decode_self_described_value(reader)?;
        reject_nan_key(&key, "map")?;
        validate_canonical_bytes(reader, &mut previous, key_start, "map")?;
        let value = decode_self_described_value(reader)?;
        entries.push((key, value));
    }
    Ok(Value::Map(entries))
}

fn decode_array(reader: &mut CompactReader<'_>) -> Result<Value, SelfDescribingError> {
    let rank = reader.read_u32()? as usize;
    if rank == 0 {
        return Err(SelfDescribingError::ArrayRankZero);
    }

    let mut dimensions = Vec::with_capacity(rank);
    for _ in 0..rank {
        dimensions.push(reader.read_u64()?);
    }

    let count = array_element_count(&dimensions)?;
    let mut elements = Vec::with_capacity(count);
    for _ in 0..count {
        elements.push(decode_self_described_value(reader)?);
    }
    Ok(Value::Array(ArrayValue {
        dimensions,
        elements,
    }))
}

fn decode_tuple(reader: &mut CompactReader<'_>) -> Result<Value, SelfDescribingError> {
    let count = reader.read_u32()? as usize;
    if count == 0 {
        return Err(SelfDescribingError::EmptyTuple);
    }
    let mut elements = Vec::with_capacity(count);
    for _ in 0..count {
        elements.push(decode_self_described_value(reader)?);
    }
    Ok(Value::Tuple(elements))
}

fn decode_struct(reader: &mut CompactReader<'_>) -> Result<Value, SelfDescribingError> {
    let count = reader.read_u32()? as usize;
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        fields.push(FieldValue {
            name: reader.read_string()?,
            value: decode_self_described_value(reader)?,
        });
    }
    Ok(Value::Struct(fields))
}

fn decode_enum(reader: &mut CompactReader<'_>) -> Result<Value, SelfDescribingError> {
    Ok(Value::Enum(EnumValue {
        variant: reader.read_string()?,
        payload: Box::new(decode_self_described_value(reader)?),
    }))
}

fn decode_extension(reader: &mut CompactReader<'_>, tag: u8) -> Result<Value, SelfDescribingError> {
    let id = reader.read_u32()?;
    let len = reader.read_u32()? as usize;
    let payload = reader.read_bytes(len)?.to_vec();
    Ok(Value::Extension(ExtensionValue { tag, id, payload }))
}

fn validate_canonical_bytes(
    reader: &CompactReader<'_>,
    previous: &mut Option<Vec<u8>>,
    start: usize,
    aggregate: &'static str,
) -> Result<(), SelfDescribingError> {
    let current = reader.consumed_from(start);
    if let Some(previous) = previous {
        match previous.as_slice().cmp(current) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => {
                return Err(CompactError::DuplicateCanonicalKey {
                    position: start,
                    aggregate,
                }
                .into());
            }
            std::cmp::Ordering::Greater => {
                return Err(CompactError::NonCanonicalOrder {
                    position: start,
                    aggregate,
                }
                .into());
            }
        }
    }
    *previous = Some(current.to_vec());
    Ok(())
}

fn reject_duplicate_bytes<'a>(
    aggregate: &'static str,
    keys: impl Iterator<Item = &'a [u8]>,
) -> Result<(), SelfDescribingError> {
    let mut previous = None;
    for key in keys {
        if previous == Some(key) {
            return Err(SelfDescribingError::DuplicateCanonicalKey { aggregate });
        }
        previous = Some(key);
    }
    Ok(())
}

fn reject_nan_key(value: &Value, aggregate: &'static str) -> Result<(), SelfDescribingError> {
    match value {
        Value::F32(value) if value.is_nan() => {
            Err(SelfDescribingError::NanCanonicalKey { aggregate })
        }
        Value::F64(value) if value.is_nan() => {
            Err(SelfDescribingError::NanCanonicalKey { aggregate })
        }
        Value::List(elements) | Value::Set(elements) | Value::Tuple(elements) => {
            for element in elements {
                reject_nan_key(element, aggregate)?;
            }
            Ok(())
        }
        Value::Map(entries) => {
            for (key, value) in entries {
                reject_nan_key(key, aggregate)?;
                reject_nan_key(value, aggregate)?;
            }
            Ok(())
        }
        Value::Array(array) => {
            for element in &array.elements {
                reject_nan_key(element, aggregate)?;
            }
            Ok(())
        }
        Value::Struct(fields) => {
            for field in fields {
                reject_nan_key(&field.value, aggregate)?;
            }
            Ok(())
        }
        Value::Enum(value) => reject_nan_key(&value.payload, aggregate),
        Value::Option(Some(value)) | Value::Dynamic(value) => reject_nan_key(value, aggregate),
        Value::Unit
        | Value::Bool(_)
        | Value::U8(_)
        | Value::U16(_)
        | Value::U32(_)
        | Value::U64(_)
        | Value::U128(_)
        | Value::I8(_)
        | Value::I16(_)
        | Value::I32(_)
        | Value::I64(_)
        | Value::I128(_)
        | Value::F32(_)
        | Value::F64(_)
        | Value::Char(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Payload(_)
        | Value::Option(None)
        | Value::ExternalAttachment
        | Value::Extension(_) => Ok(()),
    }
}

fn array_element_count(dimensions: &[u64]) -> Result<usize, SelfDescribingError> {
    if dimensions.is_empty() {
        return Err(SelfDescribingError::ArrayRankZero);
    }
    let count = dimensions.iter().try_fold(1u64, |acc, dimension| {
        acc.checked_mul(*dimension)
            .ok_or(SelfDescribingError::ArrayElementCountOverflow)
    })?;
    usize::try_from(count).map_err(|_| SelfDescribingError::ArrayElementCountOverflow)
}

fn encode_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), SelfDescribingError> {
    write_u32(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

// r[impl binette.length.canonical-width]
fn write_u32(out: &mut Vec<u8>, value: usize) -> Result<(), SelfDescribingError> {
    let value =
        u32::try_from(value).map_err(|_| SelfDescribingError::LengthOverflow { len: value })?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}
