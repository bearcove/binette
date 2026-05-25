use std::collections::HashMap;

use facet_core::Facet;
use facet_reflect::{AllocError, Partial, ReflectError, ShapeMismatchError};
use thiserror::Error;

use crate::compact::{CompactError, CompactReader};
use crate::hash::primitive_for_type_id;
use crate::plan::{PlanError, PlanNode, ReaderPlan, StructFieldPlan, reader_plan_for};
use crate::registry::SchemaRegistry;
use crate::schema::{Primitive, Schema, SchemaKind, TypeId, TypeRef, VariantPayload};

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error(transparent)]
    Plan(#[from] PlanError),

    #[error(transparent)]
    Compact(#[from] CompactError),

    #[error(transparent)]
    Alloc(#[from] AllocError),

    #[error(transparent)]
    Reflect(#[from] ReflectError),

    #[error(transparent)]
    ShapeMismatch(#[from] ShapeMismatchError),

    #[error("unknown writer type id {type_id:?} at byte {position}")]
    UnknownWriterType { position: usize, type_id: TypeId },

    #[error("unbound writer type parameter {name} at byte {position}")]
    UnboundTypeParameter { position: usize, name: String },

    #[error("unsupported decode at byte {position}: {reason}")]
    Unsupported {
        position: usize,
        reason: &'static str,
    },

    #[error("trailing bytes after compact value at byte {position}: {remaining} bytes remain")]
    TrailingBytes { position: usize, remaining: usize },
}

// r[impl binette.compat.plan]
pub fn decode_from_slice<T: Facet<'static>>(
    input: &[u8],
    writer_root: &TypeRef,
    writer_registry: &SchemaRegistry,
) -> Result<T, DecodeError> {
    let plan = reader_plan_for::<T>(writer_root, writer_registry)?;
    decode_from_slice_with_plan(input, &plan, writer_registry)
}

// r[impl binette.compat.plan]
pub fn decode_from_slice_with_plan<T: Facet<'static>>(
    input: &[u8],
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
) -> Result<T, DecodeError> {
    let mut executor = DecodeExecutor {
        reader: CompactReader::new(input),
        writer_registry,
    };
    let partial = Partial::alloc_owned::<T>()?;
    let partial = executor.decode_node(partial, &plan.root)?;
    if !executor.reader.is_empty() {
        return Err(DecodeError::TrailingBytes {
            position: executor.reader.position(),
            remaining: executor.reader.remaining(),
        });
    }
    Ok(partial.build()?.materialize::<T>()?)
}

struct DecodeExecutor<'input, 'registry> {
    reader: CompactReader<'input>,
    writer_registry: &'registry SchemaRegistry,
}

impl DecodeExecutor<'_, '_> {
    fn decode_node(
        &mut self,
        partial: Partial<'static, false>,
        node: &PlanNode,
    ) -> Result<Partial<'static, false>, DecodeError> {
        match node {
            PlanNode::Direct { writer, .. } => self.decode_type(partial, writer, &Env::default()),
            // r[impl binette.compat.field-matching]
            // r[impl binette.compat.skip-unknown]
            PlanNode::Struct { fields } => self.decode_struct_plan(partial, fields),
            PlanNode::Tuple { elements } => self.decode_tuple_plan(partial, elements),
            PlanNode::List { element } => self.decode_list_plan(partial, element),
            PlanNode::Array {
                dimensions,
                element,
            } => self.decode_array_plan(partial, dimensions, element),
            PlanNode::Option { element } => self.decode_option_plan(partial, element),
            PlanNode::Set { .. } => Err(self.unsupported("set decode is not implemented yet")),
            PlanNode::Map { .. } => Err(self.unsupported("map decode is not implemented yet")),
            PlanNode::Dynamic => Err(self.unsupported("dynamic decode is not implemented yet")),
        }
    }

    fn decode_struct_plan(
        &mut self,
        mut partial: Partial<'static, false>,
        fields: &[StructFieldPlan],
    ) -> Result<Partial<'static, false>, DecodeError> {
        for field in fields {
            match field {
                StructFieldPlan::Read {
                    reader_index, plan, ..
                } => {
                    partial = partial.begin_nth_field(*reader_index)?;
                    partial = self.decode_node(partial, plan)?;
                    partial = partial.end()?;
                }
                StructFieldPlan::Skip { writer_type, .. } => {
                    self.reader.skip_value(writer_type, self.writer_registry)?;
                }
            }
        }
        Ok(partial)
    }

    fn decode_tuple_plan(
        &mut self,
        mut partial: Partial<'static, false>,
        elements: &[PlanNode],
    ) -> Result<Partial<'static, false>, DecodeError> {
        for (index, element) in elements.iter().enumerate() {
            partial = partial.begin_nth_field(index)?;
            partial = self.decode_node(partial, element)?;
            partial = partial.end()?;
        }
        Ok(partial)
    }

    fn decode_list_plan(
        &mut self,
        partial: Partial<'static, false>,
        element: &PlanNode,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let count = self.reader.read_u32()? as usize;
        let mut partial = partial.init_list_with_capacity(count)?;
        for _ in 0..count {
            partial = partial.begin_list_item()?;
            partial = self.decode_node(partial, element)?;
            partial = partial.end()?;
        }
        Ok(partial)
    }

    fn decode_array_plan(
        &mut self,
        partial: Partial<'static, false>,
        dimensions: &[u64],
        element: &PlanNode,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let count = dimensions.iter().try_fold(1usize, |acc, dimension| {
            let dimension = usize::try_from(*dimension).map_err(|_| DecodeError::Unsupported {
                position: self.reader.position(),
                reason: "array dimension exceeds usize",
            })?;
            acc.checked_mul(dimension)
                .ok_or_else(|| DecodeError::Unsupported {
                    position: self.reader.position(),
                    reason: "array element count overflows usize",
                })
        })?;

        let mut partial = partial.init_array()?;
        for index in 0..count {
            partial = partial.begin_nth_field(index)?;
            partial = self.decode_node(partial, element)?;
            partial = partial.end()?;
        }
        Ok(partial)
    }

    fn decode_option_plan(
        &mut self,
        partial: Partial<'static, false>,
        element: &PlanNode,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let position = self.reader.position();
        match self.reader.read_u8()? {
            0x00 => Ok(partial.set_default()?),
            0x01 => {
                let mut partial = partial.begin_some()?;
                partial = self.decode_node(partial, element)?;
                Ok(partial.end()?)
            }
            value => Err(CompactError::InvalidOptionTag { position, value }.into()),
        }
    }

    fn decode_type(
        &mut self,
        partial: Partial<'static, false>,
        type_ref: &TypeRef,
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if let Some(primitive) = primitive_for_type_id(*type_id) {
                    return self.decode_primitive(partial, primitive);
                }

                let schema =
                    self.writer_registry
                        .get(*type_id)
                        .ok_or(DecodeError::UnknownWriterType {
                            position: self.reader.position(),
                            type_id: *type_id,
                        })?;
                let child_env = Env::bind(schema, args);
                self.decode_kind(partial, &schema.kind, &child_env)
            }
            TypeRef::Var { name } => {
                let resolved =
                    env.resolve(name)
                        .ok_or_else(|| DecodeError::UnboundTypeParameter {
                            position: self.reader.position(),
                            name: name.clone(),
                        })?;
                self.decode_type(partial, resolved, env)
            }
        }
    }

    fn decode_kind(
        &mut self,
        partial: Partial<'static, false>,
        kind: &SchemaKind,
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        match kind {
            SchemaKind::Primitive(primitive) => self.decode_primitive(partial, *primitive),
            // r[impl binette.aggregate.struct.compact]
            SchemaKind::Struct { fields, .. } => self.decode_struct_kind(partial, fields, env),
            SchemaKind::Enum { variants, .. } => self.decode_enum_kind(partial, variants, env),
            SchemaKind::Tuple { elements } => self.decode_tuple_kind(partial, elements, env),
            SchemaKind::List { element } => self.decode_list_kind(partial, element, env),
            SchemaKind::Array {
                dimensions,
                element,
            } => self.decode_array_kind(partial, dimensions, element, env),
            SchemaKind::Option { element } => self.decode_option_kind(partial, element, env),
            SchemaKind::Set { .. } => Err(self.unsupported("set decode is not implemented yet")),
            SchemaKind::Map { .. } => Err(self.unsupported("map decode is not implemented yet")),
            SchemaKind::Dynamic => Err(self.unsupported("dynamic decode is not implemented yet")),
            SchemaKind::External { .. } => Ok(partial.set(())?),
        }
    }

    fn decode_struct_kind(
        &mut self,
        mut partial: Partial<'static, false>,
        fields: &[crate::schema::Field],
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        for (index, field) in fields.iter().enumerate() {
            partial = partial.begin_nth_field(index)?;
            partial = self.decode_type(partial, &field.type_ref, env)?;
            partial = partial.end()?;
        }
        Ok(partial)
    }

    fn decode_enum_kind(
        &mut self,
        mut partial: Partial<'static, false>,
        variants: &[crate::schema::Variant],
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let position = self.reader.position();
        let variant_index = self.reader.read_u32()?;
        let variant_position = variants
            .iter()
            .position(|variant| variant.index == variant_index)
            .ok_or(CompactError::UnknownVariantIndex {
                position,
                variant_index,
            })?;
        partial = partial.select_nth_variant(variant_position)?;
        self.decode_variant_payload(partial, &variants[variant_position].payload, env)
    }

    fn decode_variant_payload(
        &mut self,
        mut partial: Partial<'static, false>,
        payload: &VariantPayload,
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        match payload {
            VariantPayload::Unit => Ok(partial),
            VariantPayload::Newtype { type_ref } => {
                partial = partial.begin_nth_field(0)?;
                partial = self.decode_type(partial, type_ref, env)?;
                Ok(partial.end()?)
            }
            VariantPayload::Tuple { elements } => self.decode_tuple_kind(partial, elements, env),
            VariantPayload::Struct { fields } => self.decode_struct_kind(partial, fields, env),
        }
    }

    fn decode_tuple_kind(
        &mut self,
        mut partial: Partial<'static, false>,
        elements: &[TypeRef],
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        for (index, element) in elements.iter().enumerate() {
            partial = partial.begin_nth_field(index)?;
            partial = self.decode_type(partial, element, env)?;
            partial = partial.end()?;
        }
        Ok(partial)
    }

    fn decode_list_kind(
        &mut self,
        partial: Partial<'static, false>,
        element: &TypeRef,
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let count = self.reader.read_u32()? as usize;
        let mut partial = partial.init_list_with_capacity(count)?;
        for _ in 0..count {
            partial = partial.begin_list_item()?;
            partial = self.decode_type(partial, element, env)?;
            partial = partial.end()?;
        }
        Ok(partial)
    }

    fn decode_array_kind(
        &mut self,
        partial: Partial<'static, false>,
        dimensions: &[u64],
        element: &TypeRef,
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let count = dimensions.iter().try_fold(1usize, |acc, dimension| {
            let dimension = usize::try_from(*dimension).map_err(|_| DecodeError::Unsupported {
                position: self.reader.position(),
                reason: "array dimension exceeds usize",
            })?;
            acc.checked_mul(dimension)
                .ok_or_else(|| DecodeError::Unsupported {
                    position: self.reader.position(),
                    reason: "array element count overflows usize",
                })
        })?;

        let mut partial = partial.init_array()?;
        for index in 0..count {
            partial = partial.begin_nth_field(index)?;
            partial = self.decode_type(partial, element, env)?;
            partial = partial.end()?;
        }
        Ok(partial)
    }

    fn decode_option_kind(
        &mut self,
        partial: Partial<'static, false>,
        element: &TypeRef,
        env: &Env,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let position = self.reader.position();
        match self.reader.read_u8()? {
            0x00 => Ok(partial.set_default()?),
            0x01 => {
                let mut partial = partial.begin_some()?;
                partial = self.decode_type(partial, element, env)?;
                Ok(partial.end()?)
            }
            value => Err(CompactError::InvalidOptionTag { position, value }.into()),
        }
    }

    fn decode_primitive(
        &mut self,
        partial: Partial<'static, false>,
        primitive: Primitive,
    ) -> Result<Partial<'static, false>, DecodeError> {
        // r[impl binette.scalar.unit]
        // r[impl binette.scalar.never]
        // r[impl binette.scalar.bool]
        // r[impl binette.scalar.unsigned]
        // r[impl binette.scalar.signed]
        // r[impl binette.scalar.float]
        // r[impl binette.scalar.char]
        // r[impl binette.scalar.string]
        // r[impl binette.scalar.bytes]
        match primitive {
            Primitive::Unit => Ok(partial.set(())?),
            Primitive::Never => Err(CompactError::NeverValue {
                position: self.reader.position(),
            }
            .into()),
            Primitive::Bool => Ok(partial.set(self.reader.read_bool()?)?),
            Primitive::U8 => Ok(partial.set(self.reader.read_u8()?)?),
            Primitive::U16 => Ok(partial.set(self.reader.read_u16()?)?),
            Primitive::U32 => Ok(partial.set(self.reader.read_u32()?)?),
            Primitive::U64 => Ok(partial.set(self.reader.read_u64()?)?),
            Primitive::U128 => Ok(partial.set(self.reader.read_u128()?)?),
            Primitive::I8 => Ok(partial.set(self.reader.read_i8()?)?),
            Primitive::I16 => Ok(partial.set(self.reader.read_i16()?)?),
            Primitive::I32 => Ok(partial.set(self.reader.read_i32()?)?),
            Primitive::I64 => Ok(partial.set(self.reader.read_i64()?)?),
            Primitive::I128 => Ok(partial.set(self.reader.read_i128()?)?),
            Primitive::F32 => Ok(partial.set(self.reader.read_f32()?)?),
            Primitive::F64 => Ok(partial.set(self.reader.read_f64()?)?),
            Primitive::Char => Ok(partial.set(self.reader.read_char()?)?),
            Primitive::String => Ok(partial.set(self.reader.read_string()?)?),
            Primitive::Bytes | Primitive::Payload => Ok(partial.set(self.reader.read_byte_vec()?)?),
        }
    }

    fn unsupported(&self, reason: &'static str) -> DecodeError {
        DecodeError::Unsupported {
            position: self.reader.position(),
            reason,
        }
    }
}

#[derive(Default)]
struct Env {
    bindings: HashMap<String, TypeRef>,
}

impl Env {
    fn bind(schema: &Schema, args: &[TypeRef]) -> Self {
        Self {
            bindings: schema
                .type_params
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect(),
        }
    }

    fn resolve(&self, name: &str) -> Option<&TypeRef> {
        self.bindings.get(name)
    }
}
