use std::{borrow::Cow, collections::HashMap};

use facet_core::{Facet, OpaqueDeserialize, PtrUninit, ScalarType, Shape};
use facet_reflect::{AllocError, Partial, ReflectError, ShapeMismatchError};
use thiserror::Error;

use crate::compact::{CompactError, CompactReader};
use crate::hash::primitive_for_type_id;
use crate::plan::{
    EnumPayloadPlan, EnumVariantPlan, PlanError, PlanNode, ReaderPlan, StructFieldPlan,
    reader_plan_for,
};
use crate::registry::SchemaRegistry;
use crate::schema::{Primitive, Schema, SchemaKind, TypeId, TypeRef, VariantPayload};
use crate::value::{Value, decode_dynamic_value_from_reader};

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error(transparent)]
    Plan(#[from] PlanError),

    #[error(transparent)]
    Compact(#[from] CompactError),

    #[error(transparent)]
    SelfDescribing(#[from] crate::value::SelfDescribingError),

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

    #[error("writer enum variant {variant} ({variant_index}) cannot be read at byte {position}")]
    UnreadableWriterVariant {
        position: usize,
        variant_index: u32,
        variant: String,
    },

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
        plan_nodes: plan.nodes(),
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

pub(crate) unsafe fn decode_plan_node_into_raw(
    input: &[u8],
    node: &PlanNode,
    plan_nodes: &[PlanNode],
    writer_registry: &SchemaRegistry,
    reader_shape: &'static Shape,
    out: *mut u8,
) -> Result<usize, DecodeError> {
    let mut executor = DecodeExecutor {
        reader: CompactReader::new(input),
        writer_registry,
        plan_nodes,
    };
    let ptr = PtrUninit::new(out);
    let partial = unsafe { Partial::<'static, false>::from_raw_with_shape(ptr, reader_shape)? };
    let partial = executor.decode_node(partial, node)?;
    partial.finish_in_place()?;
    Ok(executor.reader.position())
}

struct DecodeExecutor<'input, 'registry> {
    reader: CompactReader<'input>,
    writer_registry: &'registry SchemaRegistry,
    plan_nodes: &'registry [PlanNode],
}

impl DecodeExecutor<'_, '_> {
    fn decode_node(
        &mut self,
        partial: Partial<'static, false>,
        node: &PlanNode,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let shape = partial.shape();
        if let facet_core::Def::Pointer(pointer) = shape.def
            && pointer.constructible_from_pointee()
            && shape.scalar_type().is_none()
        {
            let partial = partial.begin_smart_ptr()?;
            let partial = self.decode_node(partial, node)?;
            return Ok(partial.end()?);
        }

        if shape.builder_shape.is_some() || (shape.is_transparent() && shape.inner.is_some()) {
            let partial = partial.begin_inner()?;
            let partial = self.decode_node(partial, node)?;
            return Ok(partial.end()?);
        }

        match node {
            PlanNode::Ref { node_index } => {
                let node =
                    self.plan_nodes
                        .get(*node_index)
                        .ok_or_else(|| DecodeError::Unsupported {
                            position: self.reader.position(),
                            reason: "recursive reader plan node reference is out of range",
                        })?;
                self.decode_node(partial, node)
            }
            PlanNode::Primitive { primitive } => self.decode_primitive(partial, *primitive),
            // r[impl binette.compat.field-matching]
            // r[impl binette.compat.skip-unknown]
            PlanNode::Struct { fields } => self.decode_struct_plan(partial, fields),
            PlanNode::Tuple { elements } => self.decode_tuple_plan(partial, elements),
            PlanNode::List { element } => self.decode_list_plan(partial, element),
            PlanNode::Set { element } => self.decode_set_plan(partial, element),
            PlanNode::Map { key, value } => self.decode_map_plan(partial, key, value),
            PlanNode::Array {
                dimensions,
                element,
            } => self.decode_array_plan(partial, dimensions, element),
            PlanNode::Enum { variants } => self.decode_enum_plan(partial, variants),
            PlanNode::Option { element } => self.decode_option_plan(partial, element),
            PlanNode::Dynamic => self.decode_dynamic(partial),
            PlanNode::External { .. } => Ok(partial.set(())?),
        }
    }

    fn decode_dynamic(
        &mut self,
        partial: Partial<'static, false>,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let position = self.reader.position();
        let value = decode_dynamic_value_from_reader(&mut self.reader)?;
        let value = facet_value_from_binette_value(value)
            .map_err(|reason| DecodeError::Unsupported { position, reason })?;
        Ok(partial.set(value)?)
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

    // r[impl binette.aggregate.tuple]
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

    // r[impl binette.aggregate.set]
    // r[impl binette.aggregate.set-map.canonical]
    // r[impl binette.aggregate.set-map.decode-policy]
    fn decode_set_plan(
        &mut self,
        partial: Partial<'static, false>,
        element: &PlanNode,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let count = self.reader.read_u32()? as usize;
        let mut partial = partial.init_set()?;
        let mut previous = None;
        for _ in 0..count {
            let element_start = self.reader.position();
            partial = partial.begin_set_item()?;
            partial = self.decode_node(partial, element)?;
            partial = partial.end()?;
            self.validate_no_nan_plan_key(element_start, element, "set")?;
            self.validate_canonical_key_bytes(&mut previous, element_start, "set")?;
        }
        Ok(partial)
    }

    // r[impl binette.aggregate.map]
    // r[impl binette.aggregate.set-map.canonical]
    // r[impl binette.aggregate.set-map.decode-policy]
    fn decode_map_plan(
        &mut self,
        partial: Partial<'static, false>,
        key: &PlanNode,
        value: &PlanNode,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let count = self.reader.read_u32()? as usize;
        let mut partial = partial.init_map()?;
        let mut previous = None;
        for _ in 0..count {
            let key_start = self.reader.position();
            partial = partial.begin_key()?;
            partial = self.decode_node(partial, key)?;
            partial = partial.end()?;
            self.validate_no_nan_plan_key(key_start, key, "map")?;
            self.validate_canonical_key_bytes(&mut previous, key_start, "map")?;

            partial = partial.begin_value()?;
            partial = self.decode_node(partial, value)?;
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

    // r[impl binette.compat.enum]
    // r[impl binette.compat.enum.unknown-variant]
    fn decode_enum_plan(
        &mut self,
        mut partial: Partial<'static, false>,
        variants: &[EnumVariantPlan],
    ) -> Result<Partial<'static, false>, DecodeError> {
        if matches!(partial.shape().def, facet_core::Def::Result(_)) {
            return self.decode_result_plan(partial, variants);
        }

        let position = self.reader.position();
        let variant_index = self.reader.read_u32()?;
        let variant = variants
            .iter()
            .find(|variant| match variant {
                EnumVariantPlan::Read { writer_index, .. }
                | EnumVariantPlan::Reject { writer_index, .. } => *writer_index == variant_index,
            })
            .ok_or(CompactError::UnknownVariantIndex {
                position,
                variant_index,
            })?;

        match variant {
            EnumVariantPlan::Read {
                reader_index,
                payload,
                ..
            } => {
                partial = partial.select_nth_variant(*reader_index)?;
                self.decode_enum_payload_plan(partial, payload)
            }
            EnumVariantPlan::Reject { name, .. } => Err(DecodeError::UnreadableWriterVariant {
                position,
                variant_index,
                variant: name.clone(),
            }),
        }
    }

    fn decode_result_plan(
        &mut self,
        partial: Partial<'static, false>,
        variants: &[EnumVariantPlan],
    ) -> Result<Partial<'static, false>, DecodeError> {
        let position = self.reader.position();
        let variant_index = self.reader.read_u32()?;
        let variant = variants
            .iter()
            .find(|variant| match variant {
                EnumVariantPlan::Read { writer_index, .. }
                | EnumVariantPlan::Reject { writer_index, .. } => *writer_index == variant_index,
            })
            .ok_or(CompactError::UnknownVariantIndex {
                position,
                variant_index,
            })?;

        match variant {
            EnumVariantPlan::Read {
                reader_index,
                payload,
                ..
            } => {
                let partial = match *reader_index {
                    0 => partial.begin_ok()?,
                    1 => partial.begin_err()?,
                    _ => {
                        return Err(DecodeError::Unsupported {
                            position,
                            reason: "Result reader variant index must be 0 or 1",
                        });
                    }
                };
                let partial = self.decode_result_payload_plan(partial, payload, position)?;
                Ok(partial.end()?)
            }
            EnumVariantPlan::Reject { name, .. } => Err(DecodeError::UnreadableWriterVariant {
                position,
                variant_index,
                variant: name.clone(),
            }),
        }
    }

    fn decode_result_payload_plan(
        &mut self,
        partial: Partial<'static, false>,
        payload: &EnumPayloadPlan,
        position: usize,
    ) -> Result<Partial<'static, false>, DecodeError> {
        match payload {
            EnumPayloadPlan::Unit => Ok(partial),
            EnumPayloadPlan::Newtype(element) => self.decode_node(partial, element),
            EnumPayloadPlan::Tuple(_) | EnumPayloadPlan::Struct(_) => {
                Err(DecodeError::Unsupported {
                    position,
                    reason: "Result variants must be unit or newtype payloads",
                })
            }
        }
    }

    // r[impl binette.compat.enum.payload]
    fn decode_enum_payload_plan(
        &mut self,
        mut partial: Partial<'static, false>,
        payload: &EnumPayloadPlan,
    ) -> Result<Partial<'static, false>, DecodeError> {
        match payload {
            EnumPayloadPlan::Unit => Ok(partial),
            EnumPayloadPlan::Newtype(element) => {
                partial = partial.begin_nth_field(0)?;
                partial = self.decode_node(partial, element)?;
                Ok(partial.end()?)
            }
            EnumPayloadPlan::Tuple(elements) => self.decode_tuple_plan(partial, elements),
            EnumPayloadPlan::Struct(fields) => self.decode_struct_plan(partial, fields),
        }
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
            Primitive::String => {
                let value = self.reader.read_string()?;
                if partial.shape().scalar_type() == Some(ScalarType::CowStr) {
                    Ok(partial.set(Cow::<'static, str>::Owned(value))?)
                } else {
                    Ok(partial.set(value)?)
                }
            }
            Primitive::Bytes => Ok(partial.set(self.reader.read_byte_vec()?)?),
            Primitive::Payload => self.decode_payload(partial),
        }
    }

    fn decode_payload(
        &mut self,
        partial: Partial<'static, false>,
    ) -> Result<Partial<'static, false>, DecodeError> {
        let len = self.reader.read_u32()? as usize;
        let bytes = self.reader.read_bytes(len)?;
        let Some(adapter) = partial.shape().opaque_adapter else {
            return Ok(partial.set(bytes.to_vec())?);
        };

        let input = OpaqueDeserialize::Borrowed(bytes);
        let deserialize = adapter.deserialize;
        Ok(unsafe {
            partial.set_from_function(move |target_ptr| {
                deserialize(input, target_ptr).map(|_| ()).map_err(|error| {
                    facet_reflect::ReflectErrorKind::InvariantViolation {
                        invariant: Box::leak(
                            format!("opaque adapter deserialize failed: {error}").into_boxed_str(),
                        ),
                    }
                })
            })?
        })
    }

    fn validate_canonical_key_bytes(
        &self,
        previous: &mut Option<Vec<u8>>,
        start: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        let current = self.reader.consumed_from(start);
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

    // r[impl binette.aggregate.set-map.float-keys]
    fn validate_no_nan_plan_key(
        &self,
        start: usize,
        node: &PlanNode,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        let mut reader = CompactReader::new(self.reader.consumed_from(start));
        self.scan_no_nan_plan(&mut reader, node, start, aggregate)
    }

    fn scan_no_nan_plan(
        &self,
        reader: &mut CompactReader<'_>,
        node: &PlanNode,
        base: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        match node {
            PlanNode::Ref { node_index } => {
                let node =
                    self.plan_nodes
                        .get(*node_index)
                        .ok_or_else(|| DecodeError::Unsupported {
                            position: base + reader.position(),
                            reason: "recursive reader plan node reference is out of range",
                        })?;
                self.scan_no_nan_plan(reader, node, base, aggregate)
            }
            PlanNode::Primitive { primitive } => {
                self.scan_no_nan_primitive(reader, *primitive, base, aggregate)
            }
            PlanNode::Struct { fields } => {
                for field in fields {
                    self.scan_no_nan_struct_field(reader, field, base, aggregate)?;
                }
                Ok(())
            }
            PlanNode::Tuple { elements } => {
                for element in elements {
                    self.scan_no_nan_plan(reader, element, base, aggregate)?;
                }
                Ok(())
            }
            PlanNode::List { element } | PlanNode::Set { element } => {
                let count = reader.read_u32()? as usize;
                for _ in 0..count {
                    self.scan_no_nan_plan(reader, element, base, aggregate)?;
                }
                Ok(())
            }
            PlanNode::Map { key, value } => {
                let count = reader.read_u32()? as usize;
                for _ in 0..count {
                    self.scan_no_nan_plan(reader, key, base, aggregate)?;
                    self.scan_no_nan_plan(reader, value, base, aggregate)?;
                }
                Ok(())
            }
            PlanNode::Array {
                dimensions,
                element,
            } => {
                let count = self.array_element_count(dimensions, base + reader.position())?;
                for _ in 0..count {
                    self.scan_no_nan_plan(reader, element, base, aggregate)?;
                }
                Ok(())
            }
            PlanNode::Enum { variants } => {
                let position = reader.position();
                let variant_index = reader.read_u32()?;
                let variant = variants
                    .iter()
                    .find(|variant| match variant {
                        EnumVariantPlan::Read { writer_index, .. }
                        | EnumVariantPlan::Reject { writer_index, .. } => {
                            *writer_index == variant_index
                        }
                    })
                    .ok_or(CompactError::UnknownVariantIndex {
                        position: base + position,
                        variant_index,
                    })?;

                match variant {
                    EnumVariantPlan::Read { payload, .. } => {
                        self.scan_no_nan_enum_payload(reader, payload, base, aggregate)
                    }
                    EnumVariantPlan::Reject { name, .. } => {
                        Err(DecodeError::UnreadableWriterVariant {
                            position: base + position,
                            variant_index,
                            variant: name.clone(),
                        })
                    }
                }
            }
            PlanNode::Option { element } => self.scan_no_nan_option_plan(
                reader,
                |reader| self.scan_no_nan_plan(reader, element, base, aggregate),
                base,
            ),
            PlanNode::Dynamic => Err(DecodeError::Unsupported {
                position: base + reader.position(),
                reason: "dynamic decode is not implemented yet",
            }),
            PlanNode::External { .. } => Ok(()),
        }
    }

    fn scan_no_nan_struct_field(
        &self,
        reader: &mut CompactReader<'_>,
        field: &StructFieldPlan,
        base: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        match field {
            StructFieldPlan::Read { plan, .. } => {
                self.scan_no_nan_plan(reader, plan, base, aggregate)
            }
            StructFieldPlan::Skip { writer_type, .. } => {
                self.scan_no_nan_type(reader, writer_type, &Env::default(), base, aggregate)
            }
        }
    }

    fn scan_no_nan_enum_payload(
        &self,
        reader: &mut CompactReader<'_>,
        payload: &EnumPayloadPlan,
        base: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        match payload {
            EnumPayloadPlan::Unit => Ok(()),
            EnumPayloadPlan::Newtype(element) => {
                self.scan_no_nan_plan(reader, element, base, aggregate)
            }
            EnumPayloadPlan::Tuple(elements) => {
                for element in elements {
                    self.scan_no_nan_plan(reader, element, base, aggregate)?;
                }
                Ok(())
            }
            EnumPayloadPlan::Struct(fields) => {
                for field in fields {
                    self.scan_no_nan_struct_field(reader, field, base, aggregate)?;
                }
                Ok(())
            }
        }
    }

    fn scan_no_nan_type(
        &self,
        reader: &mut CompactReader<'_>,
        type_ref: &TypeRef,
        env: &Env,
        base: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if let Some(primitive) = primitive_for_type_id(*type_id) {
                    return self.scan_no_nan_primitive(reader, primitive, base, aggregate);
                }

                let schema =
                    self.writer_registry
                        .get(*type_id)
                        .ok_or(DecodeError::UnknownWriterType {
                            position: base + reader.position(),
                            type_id: *type_id,
                        })?;
                let child_env = Env::bind(schema, args);
                self.scan_no_nan_kind(reader, &schema.kind, &child_env, base, aggregate)
            }
            TypeRef::Var { name } => {
                let resolved =
                    env.resolve(name)
                        .ok_or_else(|| DecodeError::UnboundTypeParameter {
                            position: base + reader.position(),
                            name: name.clone(),
                        })?;
                self.scan_no_nan_type(reader, resolved, env, base, aggregate)
            }
        }
    }

    fn scan_no_nan_kind(
        &self,
        reader: &mut CompactReader<'_>,
        kind: &SchemaKind,
        env: &Env,
        base: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        match kind {
            SchemaKind::Primitive(primitive) => {
                self.scan_no_nan_primitive(reader, *primitive, base, aggregate)
            }
            SchemaKind::Struct { fields, .. } => {
                for field in fields {
                    self.scan_no_nan_type(reader, &field.type_ref, env, base, aggregate)?;
                }
                Ok(())
            }
            SchemaKind::Enum { variants, .. } => {
                let position = reader.position();
                let variant_index = reader.read_u32()?;
                let variant = variants
                    .iter()
                    .find(|variant| variant.index == variant_index)
                    .ok_or(CompactError::UnknownVariantIndex {
                        position: base + position,
                        variant_index,
                    })?;
                self.scan_no_nan_variant_payload(reader, &variant.payload, env, base, aggregate)
            }
            SchemaKind::Tuple { elements } => {
                for element in elements {
                    self.scan_no_nan_type(reader, element, env, base, aggregate)?;
                }
                Ok(())
            }
            SchemaKind::List { element } | SchemaKind::Set { element } => {
                let count = reader.read_u32()? as usize;
                for _ in 0..count {
                    self.scan_no_nan_type(reader, element, env, base, aggregate)?;
                }
                Ok(())
            }
            SchemaKind::Map { key, value } => {
                let count = reader.read_u32()? as usize;
                for _ in 0..count {
                    self.scan_no_nan_type(reader, key, env, base, aggregate)?;
                    self.scan_no_nan_type(reader, value, env, base, aggregate)?;
                }
                Ok(())
            }
            SchemaKind::Array {
                element,
                dimensions,
            } => {
                let count = self.array_element_count(dimensions, base + reader.position())?;
                for _ in 0..count {
                    self.scan_no_nan_type(reader, element, env, base, aggregate)?;
                }
                Ok(())
            }
            SchemaKind::Option { element } => self.scan_no_nan_option_plan(
                reader,
                |reader| self.scan_no_nan_type(reader, element, env, base, aggregate),
                base,
            ),
            SchemaKind::Dynamic => Err(DecodeError::Unsupported {
                position: base + reader.position(),
                reason: "dynamic decode is not implemented yet",
            }),
            SchemaKind::External { .. } => Ok(()),
        }
    }

    fn scan_no_nan_variant_payload(
        &self,
        reader: &mut CompactReader<'_>,
        payload: &VariantPayload,
        env: &Env,
        base: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        match payload {
            VariantPayload::Unit => Ok(()),
            VariantPayload::Newtype { type_ref } => {
                self.scan_no_nan_type(reader, type_ref, env, base, aggregate)
            }
            VariantPayload::Tuple { elements } => {
                for element in elements {
                    self.scan_no_nan_type(reader, element, env, base, aggregate)?;
                }
                Ok(())
            }
            VariantPayload::Struct { fields } => {
                for field in fields {
                    self.scan_no_nan_type(reader, &field.type_ref, env, base, aggregate)?;
                }
                Ok(())
            }
        }
    }

    fn scan_no_nan_primitive(
        &self,
        reader: &mut CompactReader<'_>,
        primitive: Primitive,
        base: usize,
        aggregate: &'static str,
    ) -> Result<(), DecodeError> {
        match primitive {
            Primitive::F32 => {
                let position = reader.position();
                if reader.read_f32()?.is_nan() {
                    return Err(CompactError::NanCanonicalKey {
                        position: base + position,
                        aggregate,
                    }
                    .into());
                }
            }
            Primitive::F64 => {
                let position = reader.position();
                if reader.read_f64()?.is_nan() {
                    return Err(CompactError::NanCanonicalKey {
                        position: base + position,
                        aggregate,
                    }
                    .into());
                }
            }
            Primitive::Unit => {}
            Primitive::Never => {
                return Err(CompactError::NeverValue {
                    position: base + reader.position(),
                }
                .into());
            }
            Primitive::Bool => {
                reader.read_bool()?;
            }
            Primitive::U8 => {
                reader.read_u8()?;
            }
            Primitive::U16 => {
                reader.read_u16()?;
            }
            Primitive::U32 => {
                reader.read_u32()?;
            }
            Primitive::U64 => {
                reader.read_u64()?;
            }
            Primitive::U128 => {
                reader.read_u128()?;
            }
            Primitive::I8 => {
                reader.read_i8()?;
            }
            Primitive::I16 => {
                reader.read_i16()?;
            }
            Primitive::I32 => {
                reader.read_i32()?;
            }
            Primitive::I64 => {
                reader.read_i64()?;
            }
            Primitive::I128 => {
                reader.read_i128()?;
            }
            Primitive::Char => {
                reader.read_char()?;
            }
            Primitive::String => {
                reader.read_string()?;
            }
            Primitive::Bytes | Primitive::Payload => {
                reader.read_byte_vec()?;
            }
        }
        Ok(())
    }

    fn scan_no_nan_option_plan(
        &self,
        reader: &mut CompactReader<'_>,
        scan_some: impl FnOnce(&mut CompactReader<'_>) -> Result<(), DecodeError>,
        base: usize,
    ) -> Result<(), DecodeError> {
        let position = reader.position();
        match reader.read_u8()? {
            0x00 => Ok(()),
            0x01 => scan_some(reader),
            value => Err(CompactError::InvalidOptionTag {
                position: base + position,
                value,
            }
            .into()),
        }
    }

    fn array_element_count(
        &self,
        dimensions: &[u64],
        position: usize,
    ) -> Result<usize, DecodeError> {
        dimensions.iter().try_fold(1usize, |acc, dimension| {
            let dimension = usize::try_from(*dimension).map_err(|_| DecodeError::Unsupported {
                position,
                reason: "array dimension exceeds usize",
            })?;
            acc.checked_mul(dimension).ok_or(DecodeError::Unsupported {
                position,
                reason: "array element count overflows usize",
            })
        })
    }
}

fn facet_value_from_binette_value(value: Value) -> Result<facet_value::Value, &'static str> {
    match value {
        Value::Unit => Ok(facet_value::Value::NULL),
        Value::Bool(value) => Ok(facet_value::Value::from(value)),
        Value::U8(value) => Ok(facet_value::Value::from(u64::from(value))),
        Value::U16(value) => Ok(facet_value::Value::from(u64::from(value))),
        Value::U32(value) => Ok(facet_value::Value::from(u64::from(value))),
        Value::U64(value) => Ok(facet_value::Value::from(value)),
        Value::U128(value) => u64::try_from(value)
            .map(facet_value::Value::from)
            .map_err(|_| "u128 dynamic value does not fit facet_value number"),
        Value::I8(value) => Ok(facet_value::Value::from(i64::from(value))),
        Value::I16(value) => Ok(facet_value::Value::from(i64::from(value))),
        Value::I32(value) => Ok(facet_value::Value::from(i64::from(value))),
        Value::I64(value) => Ok(facet_value::Value::from(value)),
        Value::I128(value) => i64::try_from(value)
            .map(facet_value::Value::from)
            .map_err(|_| "i128 dynamic value does not fit facet_value number"),
        Value::F32(value) => Ok(facet_value::Value::from(f64::from(value))),
        Value::F64(value) => Ok(facet_value::Value::from(value)),
        Value::Char(value) => Ok(facet_value::Value::from(value.to_string())),
        Value::String(value) => Ok(facet_value::Value::from(value)),
        Value::Bytes(value) | Value::Payload(value) => Ok(facet_value::Value::from(value)),
        Value::List(values) => facet_array_from_binette_values(values),
        Value::Struct(fields) => {
            let mut object = facet_value::VObject::with_capacity(fields.len());
            for field in fields {
                object.insert(field.name, facet_value_from_binette_value(field.value)?);
            }
            Ok(object.into())
        }
        Value::Option(None) => Ok(facet_value::Value::NULL),
        Value::Option(Some(value)) | Value::Dynamic(value) => {
            facet_value_from_binette_value(*value)
        }
        Value::Set(_)
        | Value::Map(_)
        | Value::Array(_)
        | Value::Tuple(_)
        | Value::Enum(_)
        | Value::ExternalAttachment
        | Value::Extension(_) => {
            Err("binette dynamic value kind cannot be represented as facet_value")
        }
    }
}

fn facet_array_from_binette_values(values: Vec<Value>) -> Result<facet_value::Value, &'static str> {
    let mut array = facet_value::VArray::with_capacity(values.len());
    for value in values {
        array.push(facet_value_from_binette_value(value)?);
    }
    Ok(array.into())
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
