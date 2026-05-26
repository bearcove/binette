use thiserror::Error;

use crate::hash::primitive_for_type_id;
use crate::registry::SchemaRegistry;
use crate::schema::{Primitive, SchemaKind, TypeId, TypeRef, VariantPayload};
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExternalAttachmentSlot<'schema> {
    pub byte_position: usize,
    pub kind: &'schema str,
    pub metadata: &'schema Value,
}

#[derive(Debug, Error)]
pub enum CompactError {
    #[error(
        "unexpected end of input at byte {position}: needed {needed} bytes, remaining {remaining}"
    )]
    UnexpectedEof {
        position: usize,
        needed: usize,
        remaining: usize,
    },

    #[error("invalid bool byte {value:#04x} at byte {position}")]
    InvalidBool { position: usize, value: u8 },

    #[error("invalid option tag {value:#04x} at byte {position}")]
    InvalidOptionTag { position: usize, value: u8 },

    #[error("invalid unicode scalar value {value:#010x} at byte {position}")]
    InvalidChar { position: usize, value: u32 },

    #[error("invalid utf-8 string payload at byte {position}")]
    InvalidString {
        position: usize,
        source: std::str::Utf8Error,
    },

    #[error("never type has no compact value at byte {position}")]
    NeverValue { position: usize },

    #[error("unknown compact type id {type_id:?} at byte {position}")]
    UnknownTypeId { position: usize, type_id: TypeId },

    #[error("unbound type parameter {name} at byte {position}")]
    UnboundTypeParameter { position: usize, name: String },

    #[error("array element count overflows u64 at byte {position}")]
    ArrayElementCountOverflow { position: usize },

    #[error("compact enum variant index {variant_index} is out of range at byte {position}")]
    UnknownVariantIndex { position: usize, variant_index: u32 },

    #[error("{aggregate} entries are not in canonical byte order at byte {position}")]
    NonCanonicalOrder {
        position: usize,
        aggregate: &'static str,
    },

    #[error("{aggregate} contains duplicate canonical key bytes at byte {position}")]
    DuplicateCanonicalKey {
        position: usize,
        aggregate: &'static str,
    },

    #[error("NaN is not a valid {aggregate} key payload at byte {position}")]
    NanCanonicalKey {
        position: usize,
        aggregate: &'static str,
    },

    #[error("unsupported compact skip at byte {position}: {reason}")]
    Unsupported {
        position: usize,
        reason: &'static str,
    },
}

pub struct CompactReader<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> CompactReader<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn remaining(&self) -> usize {
        self.input.len() - self.position
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub(crate) fn consumed_from(&self, start: usize) -> &'a [u8] {
        &self.input[start..self.position]
    }

    // r[impl binette.aggregate.schema-driven-skip]
    pub fn skip_value(
        &mut self,
        type_ref: &TypeRef,
        registry: &SchemaRegistry,
    ) -> Result<(), CompactError> {
        let mut ignore_external = |_| {};
        let mut env = Env::default();
        self.walk_type(type_ref, registry, &mut env, &mut ignore_external)
    }

    // r[impl binette.aggregate.external-attachment]
    pub fn external_attachment_slots<'schema>(
        &mut self,
        type_ref: &TypeRef,
        registry: &'schema SchemaRegistry,
    ) -> Result<Vec<ExternalAttachmentSlot<'schema>>, CompactError> {
        let mut slots = Vec::new();
        let mut env = Env::default();
        self.walk_type(type_ref, registry, &mut env, &mut |slot| slots.push(slot))?;
        Ok(slots)
    }

    fn walk_type<'schema>(
        &mut self,
        type_ref: &TypeRef,
        registry: &'schema SchemaRegistry,
        env: &mut Env,
        on_external: &mut impl FnMut(ExternalAttachmentSlot<'schema>),
    ) -> Result<(), CompactError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if let Some(primitive) = primitive_for_type_id(*type_id) {
                    return self.skip_primitive(primitive);
                }

                let schema = registry.get(*type_id).ok_or(CompactError::UnknownTypeId {
                    position: self.position,
                    type_id: *type_id,
                })?;
                let mark = env.push(&schema.type_params, args);
                let result = self.walk_kind(&schema.kind, registry, env, on_external);
                env.truncate(mark);
                result
            }
            TypeRef::Var { name } => {
                let resolved = env.resolve(name).cloned().ok_or_else(|| {
                    CompactError::UnboundTypeParameter {
                        position: self.position,
                        name: name.clone(),
                    }
                })?;
                self.walk_type(&resolved, registry, env, on_external)
            }
        }
    }

    fn walk_kind<'schema>(
        &mut self,
        kind: &'schema SchemaKind,
        registry: &'schema SchemaRegistry,
        env: &mut Env,
        on_external: &mut impl FnMut(ExternalAttachmentSlot<'schema>),
    ) -> Result<(), CompactError> {
        match kind {
            SchemaKind::Primitive(primitive) => self.skip_primitive(*primitive),
            SchemaKind::Struct { fields, .. } => {
                for field in fields {
                    self.walk_type(&field.type_ref, registry, env, on_external)?;
                }
                Ok(())
            }
            SchemaKind::Enum { variants, .. } => {
                let position = self.position;
                let variant_index = self.read_u32()?;
                let variant = variants
                    .iter()
                    .find(|variant| variant.index == variant_index)
                    .ok_or(CompactError::UnknownVariantIndex {
                        position,
                        variant_index,
                    })?;
                self.walk_variant_payload(&variant.payload, registry, env, on_external)
            }
            SchemaKind::Tuple { elements } => {
                for element in elements {
                    self.walk_type(element, registry, env, on_external)?;
                }
                Ok(())
            }
            SchemaKind::List { element } | SchemaKind::Set { element } => {
                let count = self.read_u32()?;
                for _ in 0..count {
                    self.walk_type(element, registry, env, on_external)?;
                }
                Ok(())
            }
            SchemaKind::Map { key, value } => {
                let count = self.read_u32()?;
                for _ in 0..count {
                    self.walk_type(key, registry, env, on_external)?;
                    self.walk_type(value, registry, env, on_external)?;
                }
                Ok(())
            }
            SchemaKind::Array {
                element,
                dimensions,
            } => {
                let count = dimensions.iter().try_fold(1u64, |acc, dimension| {
                    acc.checked_mul(*dimension)
                        .ok_or(CompactError::ArrayElementCountOverflow {
                            position: self.position,
                        })
                })?;
                for _ in 0..count {
                    self.walk_type(element, registry, env, on_external)?;
                }
                Ok(())
            }
            SchemaKind::Option { element } => {
                let position = self.position;
                match self.read_u8()? {
                    0x00 => Ok(()),
                    0x01 => self.walk_type(element, registry, env, on_external),
                    value => Err(CompactError::InvalidOptionTag { position, value }),
                }
            }
            SchemaKind::Dynamic => Err(CompactError::Unsupported {
                position: self.position,
                reason: "dynamic compact values require self-describing skip",
            }),
            SchemaKind::External { kind, metadata } => {
                on_external(ExternalAttachmentSlot {
                    byte_position: self.position,
                    kind,
                    metadata,
                });
                Ok(())
            }
        }
    }

    fn walk_variant_payload<'schema>(
        &mut self,
        payload: &'schema VariantPayload,
        registry: &'schema SchemaRegistry,
        env: &mut Env,
        on_external: &mut impl FnMut(ExternalAttachmentSlot<'schema>),
    ) -> Result<(), CompactError> {
        match payload {
            VariantPayload::Unit => Ok(()),
            VariantPayload::Newtype { type_ref } => {
                self.walk_type(type_ref, registry, env, on_external)
            }
            VariantPayload::Tuple { elements } => {
                for element in elements {
                    self.walk_type(element, registry, env, on_external)?;
                }
                Ok(())
            }
            VariantPayload::Struct { fields } => {
                for field in fields {
                    self.walk_type(&field.type_ref, registry, env, on_external)?;
                }
                Ok(())
            }
        }
    }

    fn skip_primitive(&mut self, primitive: Primitive) -> Result<(), CompactError> {
        match primitive {
            // r[impl binette.scalar.unit]
            Primitive::Unit => Ok(()),
            // r[impl binette.scalar.never]
            Primitive::Never => Err(CompactError::NeverValue {
                position: self.position,
            }),
            // r[impl binette.scalar.bool]
            Primitive::Bool => {
                let position = self.position;
                match self.read_u8()? {
                    0x00 | 0x01 => Ok(()),
                    value => Err(CompactError::InvalidBool { position, value }),
                }
            }
            // r[impl binette.scalar.unsigned]
            Primitive::U8 => self.skip_bytes(1),
            Primitive::U16 => self.skip_bytes(2),
            Primitive::U32 => self.skip_bytes(4),
            Primitive::U64 => self.skip_bytes(8),
            Primitive::U128 => self.skip_bytes(16),
            // r[impl binette.scalar.signed]
            Primitive::I8 => self.skip_bytes(1),
            Primitive::I16 => self.skip_bytes(2),
            Primitive::I32 => self.skip_bytes(4),
            Primitive::I64 => self.skip_bytes(8),
            Primitive::I128 => self.skip_bytes(16),
            // r[impl binette.scalar.float]
            Primitive::F32 => self.skip_bytes(4),
            Primitive::F64 => self.skip_bytes(8),
            // r[impl binette.scalar.char]
            Primitive::Char => {
                let position = self.position;
                let value = self.read_u32()?;
                char::from_u32(value)
                    .map(|_| ())
                    .ok_or(CompactError::InvalidChar { position, value })
            }
            // r[impl binette.scalar.string]
            Primitive::String => {
                let len = self.read_u32()? as usize;
                let position = self.position;
                let bytes = self.read_bytes(len)?;
                std::str::from_utf8(bytes)
                    .map(|_| ())
                    .map_err(|source| CompactError::InvalidString { position, source })
            }
            // r[impl binette.scalar.bytes]
            Primitive::Bytes | Primitive::Payload => {
                let len = self.read_u32()? as usize;
                self.skip_bytes(len)
            }
        }
    }

    pub(crate) fn read_bool(&mut self) -> Result<bool, CompactError> {
        let position = self.position;
        match self.read_u8()? {
            0x00 => Ok(false),
            0x01 => Ok(true),
            value => Err(CompactError::InvalidBool { position, value }),
        }
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, CompactError> {
        self.require(1)?;
        let value = self.input[self.position];
        self.position += 1;
        Ok(value)
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, CompactError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    // r[impl binette.endianness]
    // r[impl binette.length.u32]
    pub(crate) fn read_u32(&mut self) -> Result<u32, CompactError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, CompactError> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_u128(&mut self) -> Result<u128, CompactError> {
        Ok(u128::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_i8(&mut self) -> Result<i8, CompactError> {
        Ok(i8::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_i16(&mut self) -> Result<i16, CompactError> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_i32(&mut self) -> Result<i32, CompactError> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_i64(&mut self) -> Result<i64, CompactError> {
        Ok(i64::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_i128(&mut self) -> Result<i128, CompactError> {
        Ok(i128::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_f32(&mut self) -> Result<f32, CompactError> {
        Ok(f32::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_f64(&mut self) -> Result<f64, CompactError> {
        Ok(f64::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_char(&mut self) -> Result<char, CompactError> {
        let position = self.position;
        let value = self.read_u32()?;
        char::from_u32(value).ok_or(CompactError::InvalidChar { position, value })
    }

    pub(crate) fn read_string(&mut self) -> Result<String, CompactError> {
        let len = self.read_u32()? as usize;
        let position = self.position;
        let bytes = self.read_bytes(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|source| CompactError::InvalidString { position, source })
    }

    pub(crate) fn read_byte_vec(&mut self) -> Result<Vec<u8>, CompactError> {
        let len = self.read_u32()? as usize;
        Ok(self.read_bytes(len)?.to_vec())
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], CompactError> {
        self.require(N)?;
        let mut bytes = [0; N];
        bytes.copy_from_slice(&self.input[self.position..self.position + N]);
        self.position += N;
        Ok(bytes)
    }

    pub(crate) fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], CompactError> {
        self.require(len)?;
        let bytes = &self.input[self.position..self.position + len];
        self.position += len;
        Ok(bytes)
    }

    fn skip_bytes(&mut self, len: usize) -> Result<(), CompactError> {
        self.require(len)?;
        self.position += len;
        Ok(())
    }

    fn require(&self, needed: usize) -> Result<(), CompactError> {
        if self.remaining() < needed {
            Err(CompactError::UnexpectedEof {
                position: self.position,
                needed,
                remaining: self.remaining(),
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct Env {
    bindings: Vec<(String, TypeRef)>,
}

impl Env {
    fn push(&mut self, type_params: &[String], args: &[TypeRef]) -> usize {
        let mark = self.bindings.len();
        self.bindings
            .extend(type_params.iter().cloned().zip(args.iter().cloned()));
        mark
    }

    fn truncate(&mut self, mark: usize) {
        self.bindings.truncate(mark);
    }

    fn resolve(&self, name: &str) -> Option<&TypeRef> {
        self.bindings
            .iter()
            .rev()
            .find_map(|(param, type_ref)| (param == name).then_some(type_ref))
    }
}
