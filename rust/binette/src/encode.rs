use facet_core::{Def, Facet, Shape, Type, UserType};
use facet_reflect::{HasFields, Peek, ReflectError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error(transparent)]
    Reflect(#[from] ReflectError),

    #[error("value length {len} exceeds binette u32 length limit")]
    LengthOverflow { len: usize },

    #[error("unsupported encode for {shape}: {reason}")]
    Unsupported {
        shape: &'static Shape,
        reason: &'static str,
    },
}

// r[impl binette.mode.compact]
pub fn encode_to_vec<T: Facet<'static>>(value: &T) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    encode_peek(&mut out, Peek::new(value))?;
    Ok(out)
}

fn encode_peek(out: &mut Vec<u8>, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
    if let Some(string) = peek.as_str() {
        return encode_bytes(out, string.as_bytes());
    }

    if let Some(bytes) = compact_bytes(peek)? {
        return encode_bytes(out, bytes);
    }

    match peek.shape().def {
        Def::Option(_) => encode_option(out, peek),
        Def::List(_) => encode_list(out, peek),
        Def::Array(_) => encode_array(out, peek),
        Def::Slice(_) => encode_list(out, peek),
        Def::Map(_) => Err(unsupported(peek, "map encode is not implemented yet")),
        Def::Set(_) => Err(unsupported(peek, "set encode is not implemented yet")),
        Def::DynamicValue(_) => Err(unsupported(peek, "dynamic encode is not implemented yet")),
        Def::Pointer(pointer) if pointer.pointee().is_some() => {
            let pointer = peek.into_pointer()?;
            let inner = pointer
                .borrow_inner()
                .ok_or_else(|| unsupported(peek, "null pointer cannot be encoded"))?;
            encode_peek(out, inner)
        }
        _ => match peek.shape().ty {
            Type::User(UserType::Struct(_)) => encode_struct(out, peek),
            Type::User(UserType::Enum(_)) => encode_enum(out, peek),
            _ => encode_scalar(out, peek),
        },
    }
}

// r[impl binette.aggregate.struct.compact]
fn encode_struct(out: &mut Vec<u8>, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
    for (_field, value) in peek.into_struct()?.fields_for_binary_serialize() {
        encode_peek(out, value)?;
    }
    Ok(())
}

// r[impl binette.aggregate.enum.compact]
fn encode_enum(out: &mut Vec<u8>, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
    let enum_peek = peek.into_enum()?;
    let variant_index = enum_peek
        .variant_index()
        .map_err(|_| unsupported(peek, "enum variant index is not available"))?;
    write_u32(out, variant_index)?;
    for (_field, value) in enum_peek.fields_for_binary_serialize() {
        encode_peek(out, value)?;
    }
    Ok(())
}

// r[impl binette.aggregate.list]
fn encode_list(out: &mut Vec<u8>, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
    let list = peek.into_list_like()?;
    write_u32(out, list.len())?;
    for item in list.iter() {
        encode_peek(out, item)?;
    }
    Ok(())
}

// r[impl binette.aggregate.array]
fn encode_array(out: &mut Vec<u8>, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
    let array = peek.into_list_like()?;
    for item in array.iter() {
        encode_peek(out, item)?;
    }
    Ok(())
}

// r[impl binette.aggregate.option]
fn encode_option(out: &mut Vec<u8>, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
    let option = peek.into_option()?;
    match option.value() {
        Some(inner) => {
            out.push(0x01);
            encode_peek(out, inner)
        }
        None => {
            out.push(0x00);
            Ok(())
        }
    }
}

fn encode_scalar(out: &mut Vec<u8>, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
    // r[impl binette.scalar.unit]
    // r[impl binette.scalar.bool]
    // r[impl binette.scalar.unsigned]
    // r[impl binette.scalar.signed]
    // r[impl binette.scalar.float]
    // r[impl binette.scalar.char]
    match peek.scalar_type() {
        Some(facet_core::ScalarType::Unit) => Ok(()),
        Some(facet_core::ScalarType::Bool) => {
            out.push(u8::from(*peek.get::<bool>()?));
            Ok(())
        }
        Some(facet_core::ScalarType::Char) => {
            out.extend_from_slice(&(*peek.get::<char>()? as u32).to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::F32) => {
            out.extend_from_slice(&peek.get::<f32>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::F64) => {
            out.extend_from_slice(&peek.get::<f64>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::U8) => {
            out.push(*peek.get::<u8>()?);
            Ok(())
        }
        Some(facet_core::ScalarType::U16) => {
            out.extend_from_slice(&peek.get::<u16>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::U32) => {
            out.extend_from_slice(&peek.get::<u32>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::U64) => {
            out.extend_from_slice(&peek.get::<u64>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::U128) => {
            out.extend_from_slice(&peek.get::<u128>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::I8) => {
            out.extend_from_slice(&peek.get::<i8>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::I16) => {
            out.extend_from_slice(&peek.get::<i16>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::I32) => {
            out.extend_from_slice(&peek.get::<i32>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::I64) => {
            out.extend_from_slice(&peek.get::<i64>()?.to_le_bytes());
            Ok(())
        }
        Some(facet_core::ScalarType::I128) => {
            out.extend_from_slice(&peek.get::<i128>()?.to_le_bytes());
            Ok(())
        }
        _ => Err(unsupported(peek, "unsupported scalar shape")),
    }
}

fn compact_bytes<'mem>(peek: Peek<'mem, 'static>) -> Result<Option<&'mem [u8]>, EncodeError> {
    match peek.shape().def {
        Def::List(_) | Def::Slice(_) => Ok(peek.into_list_like()?.as_bytes()),
        _ => Ok(None),
    }
}

// r[impl binette.length.u32]
fn encode_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), EncodeError> {
    write_u32(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

// r[impl binette.endianness]
// r[impl binette.length.u32]
fn write_u32(out: &mut Vec<u8>, value: usize) -> Result<(), EncodeError> {
    let value = u32::try_from(value).map_err(|_| EncodeError::LengthOverflow { len: value })?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn unsupported(peek: Peek<'_, 'static>, reason: &'static str) -> EncodeError {
    EncodeError::Unsupported {
        shape: peek.shape(),
        reason,
    }
}
