use std::collections::HashMap;

use facet_core::{Def, DynValueKind, Facet, FieldError, Shape, Type, UserType};
use facet_reflect::{Peek, PeekDynamicValue, ReflectError};
use thiserror::Error;

use crate::error::SchemaError;
use crate::facet::schema_bundle_for_shape;
use crate::hash::primitive_for_type_id;
use crate::registry::SchemaRegistry;
use crate::schema::{
    Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef, Variant, VariantPayload,
};
use crate::value::{Value, encode_dynamic_value_to_vec};

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error(transparent)]
    Schema(#[from] SchemaError),

    #[error(transparent)]
    SelfDescribing(#[from] crate::value::SelfDescribingError),

    #[error(transparent)]
    Reflect(#[from] ReflectError),

    #[error("value length {len} exceeds binette u32 length limit")]
    LengthOverflow { len: usize },

    #[error("writer type id {type_id:?} is not installed in the writer schema")]
    UnknownWriterType { type_id: TypeId },

    #[error("unbound writer type parameter {name}")]
    UnboundTypeParameter { name: String },

    #[error("writer plan field {field} is not present on {shape}")]
    MissingField {
        shape: &'static Shape,
        field: String,
    },

    #[error("writer plan enum variant {variant} is not present on {shape}")]
    MissingVariant {
        shape: &'static Shape,
        variant: String,
    },

    #[error("NaN is not a valid set element or map key for {shape}")]
    NanCanonicalKey { shape: &'static Shape },

    #[error("{aggregate} contains duplicate canonical key bytes for {shape}")]
    DuplicateCanonicalKey {
        shape: &'static Shape,
        aggregate: &'static str,
    },

    #[error("unsupported encode for {shape}: {reason}")]
    Unsupported {
        shape: &'static Shape,
        reason: &'static str,
    },

    #[error("invalid writer plan: {reason}")]
    InvalidPlan { reason: &'static str },
}

pub struct WriterPlan {
    bundle: SchemaBundle,
    root: WriterNode,
    nodes: Vec<WriterNode>,
}

impl WriterPlan {
    pub fn schema_bundle(&self) -> &SchemaBundle {
        &self.bundle
    }

    pub fn root(&self) -> &TypeRef {
        &self.bundle.root
    }

    pub(crate) fn root_node(&self) -> &WriterNode {
        &self.root
    }

    pub(crate) fn nodes(&self) -> &[WriterNode] {
        &self.nodes
    }
}

// r[impl binette.schema.model]
// r[impl binette.mode.compact]
pub fn writer_plan_for<T: Facet<'static>>() -> Result<WriterPlan, EncodeError> {
    writer_plan_for_shape(T::SHAPE)
}

// r[impl binette.schema.model]
// r[impl binette.mode.compact]
pub fn writer_plan_for_shape(shape: &'static Shape) -> Result<WriterPlan, EncodeError> {
    let bundle = schema_bundle_for_shape(shape)?;
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(&bundle)?;

    let mut builder = WriterPlanBuilder {
        registry: &registry,
        nodes: Vec::new(),
        active: HashMap::new(),
    };
    let root = builder.plan_type(&bundle.root, &Env::default(), shape)?;

    Ok(WriterPlan {
        bundle,
        root,
        nodes: builder.nodes,
    })
}

// r[impl binette.mode.compact]
pub fn encode_to_vec<T: Facet<'static>>(value: &T) -> Result<Vec<u8>, EncodeError> {
    let plan = writer_plan_for::<T>()?;
    encode_to_vec_with_plan(value, &plan)
}

// r[impl binette.mode.compact]
pub fn encode_to_vec_with_plan<T: Facet<'static>>(
    value: &T,
    plan: &WriterPlan,
) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    encode_with_plan(&mut out, Peek::new(value), plan)?;
    Ok(out)
}

fn encode_with_plan(
    out: &mut Vec<u8>,
    peek: Peek<'_, 'static>,
    plan: &WriterPlan,
) -> Result<(), EncodeError> {
    WriterPlanExecutor {
        out,
        nodes: &plan.nodes,
    }
    .encode_node(peek, &plan.root)
}

pub(crate) fn encode_node_with_writer_node(
    out: &mut Vec<u8>,
    peek: Peek<'_, 'static>,
    node: &WriterNode,
    nodes: &[WriterNode],
) -> Result<(), EncodeError> {
    WriterPlanExecutor { out, nodes }.encode_node(peek, node)
}

#[derive(Debug, Clone)]
pub(crate) enum WriterNode {
    Ref {
        node_index: usize,
    },
    Primitive(Primitive),
    Struct {
        fields: Vec<WriterFieldPlan>,
    },
    Enum {
        variants: Vec<WriterVariantPlan>,
    },
    Tuple {
        elements: Vec<WriterTupleElementPlan>,
    },
    List {
        element: Box<WriterNode>,
    },
    Set {
        element: Box<WriterNode>,
    },
    Map {
        key: Box<WriterNode>,
        value: Box<WriterNode>,
    },
    Array {
        dimensions: Vec<u64>,
        element: Box<WriterNode>,
    },
    Option {
        element: Box<WriterNode>,
    },
    Dynamic,
    External,
}

#[derive(Debug, Clone)]
pub(crate) struct WriterFieldPlan {
    pub(crate) facet_index: usize,
    pub(crate) name: String,
    pub(crate) node: WriterNode,
}

#[derive(Debug, Clone)]
pub(crate) struct WriterTupleElementPlan {
    pub(crate) facet_index: usize,
    pub(crate) node: WriterNode,
}

#[derive(Debug, Clone)]
pub(crate) struct WriterVariantPlan {
    pub(crate) facet_index: usize,
    pub(crate) wire_index: u32,
    pub(crate) payload: WriterVariantPayloadPlan,
}

#[derive(Debug, Clone)]
pub(crate) enum WriterVariantPayloadPlan {
    Unit,
    Newtype(WriterTupleElementPlan),
    Tuple(Vec<WriterTupleElementPlan>),
    Struct(Vec<WriterFieldPlan>),
}

struct WriterPlanBuilder<'a> {
    registry: &'a SchemaRegistry,
    nodes: Vec<WriterNode>,
    active: HashMap<TypeRef, usize>,
}

impl WriterPlanBuilder<'_> {
    fn plan_type(
        &mut self,
        type_ref: &TypeRef,
        env: &Env,
        shape: &'static Shape,
    ) -> Result<WriterNode, EncodeError> {
        let type_ref = self.resolve_type_ref(type_ref, env)?;
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if let Some(primitive) = primitive_for_type_id(type_id) {
                    if !args.is_empty() {
                        return Err(unsupported_shape(
                            shape,
                            "primitive type reference has type arguments",
                        ));
                    }
                    return Ok(WriterNode::Primitive(primitive));
                }

                let resolved = TypeRef::Concrete {
                    type_id,
                    args: args.clone(),
                };
                if let Some(node_index) = self.active.get(&resolved) {
                    return Ok(WriterNode::Ref {
                        node_index: *node_index,
                    });
                }

                let schema = self
                    .registry
                    .get(type_id)
                    .ok_or(EncodeError::UnknownWriterType { type_id })?;
                let env = Env::bind(schema, &args);
                let node_index = self.nodes.len();
                self.nodes.push(WriterNode::Dynamic);
                self.active.insert(resolved.clone(), node_index);
                let node = self.plan_kind(&schema.kind, &env, shape);
                self.active.remove(&resolved);
                let node = node?;
                self.nodes[node_index] = node.clone();
                Ok(node)
            }
            TypeRef::Var { name } => Err(EncodeError::UnboundTypeParameter { name }),
        }
    }

    fn resolve_type_ref(&self, type_ref: &TypeRef, env: &Env) -> Result<TypeRef, EncodeError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => Ok(TypeRef::Concrete {
                type_id: *type_id,
                args: args
                    .iter()
                    .map(|arg| self.resolve_type_ref(arg, env))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            TypeRef::Var { name } => env
                .resolve(name)
                .cloned()
                .ok_or_else(|| EncodeError::UnboundTypeParameter { name: name.clone() }),
        }
    }

    fn plan_kind(
        &mut self,
        kind: &SchemaKind,
        env: &Env,
        shape: &'static Shape,
    ) -> Result<WriterNode, EncodeError> {
        let shape = schema_shape(shape);
        match kind {
            SchemaKind::Primitive(primitive) => Ok(WriterNode::Primitive(*primitive)),
            SchemaKind::Struct { fields, .. } => self.plan_struct(fields, env, shape),
            SchemaKind::Enum { variants, .. } => self.plan_enum(variants, env, shape),
            SchemaKind::Tuple { elements } => self.plan_tuple(elements, env, shape),
            SchemaKind::List { element } => Ok(WriterNode::List {
                element: Box::new(self.plan_type(element, env, list_element_shape(shape)?)?),
            }),
            SchemaKind::Set { element } => Ok(WriterNode::Set {
                element: Box::new(self.plan_type(element, env, set_element_shape(shape)?)?),
            }),
            SchemaKind::Map { key, value } => {
                let (key_shape, value_shape) = map_shapes(shape)?;
                Ok(WriterNode::Map {
                    key: Box::new(self.plan_type(key, env, key_shape)?),
                    value: Box::new(self.plan_type(value, env, value_shape)?),
                })
            }
            SchemaKind::Array {
                dimensions,
                element,
            } => Ok(WriterNode::Array {
                dimensions: dimensions.clone(),
                element: Box::new(self.plan_type(element, env, array_element_shape(shape)?)?),
            }),
            SchemaKind::Option { element } => Ok(WriterNode::Option {
                element: Box::new(self.plan_type(element, env, option_element_shape(shape)?)?),
            }),
            SchemaKind::Dynamic => Ok(WriterNode::Dynamic),
            SchemaKind::External { .. } => Ok(WriterNode::External),
        }
    }

    // r[impl binette.aggregate.struct.compact]
    fn plan_struct(
        &mut self,
        schema_fields: &[Field],
        env: &Env,
        shape: &'static Shape,
    ) -> Result<WriterNode, EncodeError> {
        let Type::User(UserType::Struct(struct_type)) = shape.ty else {
            return Err(unsupported_shape(
                shape,
                "schema struct requires Facet struct shape",
            ));
        };

        let mut fields = Vec::with_capacity(schema_fields.len());
        for field in schema_fields {
            let (facet_index, facet_field) = struct_type
                .fields
                .iter()
                .enumerate()
                .find(|(_, facet_field)| {
                    !facet_field.should_skip_serializing_unconditional()
                        && facet_field.effective_name() == field.name
                })
                .ok_or_else(|| EncodeError::MissingField {
                    shape,
                    field: field.name.clone(),
                })?;
            fields.push(WriterFieldPlan {
                facet_index,
                name: field.name.clone(),
                node: self.plan_type(&field.type_ref, env, facet_field.shape())?,
            });
        }

        Ok(WriterNode::Struct { fields })
    }

    // r[impl binette.aggregate.enum.compact]
    fn plan_enum(
        &mut self,
        schema_variants: &[Variant],
        env: &Env,
        shape: &'static Shape,
    ) -> Result<WriterNode, EncodeError> {
        let Type::User(UserType::Enum(enum_type)) = shape.ty else {
            return Err(unsupported_shape(
                shape,
                "schema enum requires Facet enum shape",
            ));
        };

        let mut variants = Vec::with_capacity(schema_variants.len());
        for schema_variant in schema_variants {
            let (facet_index, facet_variant) = enum_type
                .variants
                .iter()
                .enumerate()
                .find(|(_, facet_variant)| facet_variant.effective_name() == schema_variant.name)
                .ok_or_else(|| EncodeError::MissingVariant {
                    shape,
                    variant: schema_variant.name.clone(),
                })?;
            variants.push(WriterVariantPlan {
                facet_index,
                wire_index: schema_variant.index,
                payload: self.plan_variant_payload(
                    &schema_variant.payload,
                    env,
                    shape,
                    facet_variant.data,
                )?,
            });
        }

        Ok(WriterNode::Enum { variants })
    }

    fn plan_variant_payload(
        &mut self,
        payload: &VariantPayload,
        env: &Env,
        owner_shape: &'static Shape,
        data: facet_core::StructType,
    ) -> Result<WriterVariantPayloadPlan, EncodeError> {
        match payload {
            VariantPayload::Unit => Ok(WriterVariantPayloadPlan::Unit),
            VariantPayload::Newtype { type_ref } => {
                let field = data.fields.first().ok_or_else(|| {
                    unsupported_shape(owner_shape, "newtype variant has no Facet payload field")
                })?;
                Ok(WriterVariantPayloadPlan::Newtype(WriterTupleElementPlan {
                    facet_index: 0,
                    node: self.plan_type(type_ref, env, field.shape())?,
                }))
            }
            VariantPayload::Tuple { elements } => Ok(WriterVariantPayloadPlan::Tuple(
                self.plan_tuple_elements(elements, env, owner_shape, data)?,
            )),
            VariantPayload::Struct { fields } => Ok(WriterVariantPayloadPlan::Struct(
                self.plan_struct_fields(fields, env, owner_shape, data)?,
            )),
        }
    }

    fn plan_tuple(
        &mut self,
        elements: &[TypeRef],
        env: &Env,
        shape: &'static Shape,
    ) -> Result<WriterNode, EncodeError> {
        let Type::User(UserType::Struct(struct_type)) = shape.ty else {
            return Err(unsupported_shape(
                shape,
                "schema tuple requires Facet tuple shape",
            ));
        };

        Ok(WriterNode::Tuple {
            elements: self.plan_tuple_elements(elements, env, shape, struct_type)?,
        })
    }

    fn plan_tuple_elements(
        &mut self,
        elements: &[TypeRef],
        env: &Env,
        owner_shape: &'static Shape,
        struct_type: facet_core::StructType,
    ) -> Result<Vec<WriterTupleElementPlan>, EncodeError> {
        if struct_type.fields.len() != elements.len() {
            return Err(unsupported_shape(
                owner_shape,
                "tuple arity does not match schema",
            ));
        }

        elements
            .iter()
            .zip(struct_type.fields)
            .enumerate()
            .map(|(facet_index, (element, field))| {
                Ok(WriterTupleElementPlan {
                    facet_index,
                    node: self.plan_type(element, env, field.shape())?,
                })
            })
            .collect()
    }

    fn plan_struct_fields(
        &mut self,
        schema_fields: &[Field],
        env: &Env,
        owner_shape: &'static Shape,
        struct_type: facet_core::StructType,
    ) -> Result<Vec<WriterFieldPlan>, EncodeError> {
        let mut fields = Vec::with_capacity(schema_fields.len());
        for field in schema_fields {
            let (facet_index, facet_field) = struct_type
                .fields
                .iter()
                .enumerate()
                .find(|(_, facet_field)| facet_field.effective_name() == field.name)
                .ok_or_else(|| EncodeError::MissingField {
                    shape: owner_shape,
                    field: field.name.clone(),
                })?;
            fields.push(WriterFieldPlan {
                facet_index,
                name: field.name.clone(),
                node: self.plan_type(&field.type_ref, env, facet_field.shape())?,
            });
        }
        Ok(fields)
    }
}

struct WriterPlanExecutor<'a> {
    out: &'a mut Vec<u8>,
    nodes: &'a [WriterNode],
}

impl WriterPlanExecutor<'_> {
    fn encode_node(
        &mut self,
        peek: Peek<'_, 'static>,
        node: &WriterNode,
    ) -> Result<(), EncodeError> {
        let peek = peek.innermost_peek();
        match node {
            WriterNode::Ref { node_index } => {
                let node = self
                    .nodes
                    .get(*node_index)
                    .ok_or(EncodeError::InvalidPlan {
                        reason: "recursive writer node reference is out of range",
                    })?;
                self.encode_node(peek, node)
            }
            WriterNode::Primitive(primitive) => self.encode_primitive(peek, *primitive),
            WriterNode::Struct { fields } => self.encode_struct(peek, fields),
            WriterNode::Enum { variants } => self.encode_enum(peek, variants),
            WriterNode::Tuple { elements } => self.encode_tuple(peek, elements),
            WriterNode::List { element } => self.encode_list(peek, element),
            WriterNode::Set { element } => self.encode_set(peek, element),
            WriterNode::Map { key, value } => self.encode_map(peek, key, value),
            WriterNode::Array {
                dimensions,
                element,
            } => self.encode_array(peek, dimensions, element),
            WriterNode::Option { element } => self.encode_option(peek, element),
            WriterNode::Dynamic => self.encode_dynamic(peek),
            WriterNode::External => self.encode_primitive(peek, Primitive::Unit),
        }
    }

    fn encode_dynamic(&mut self, peek: Peek<'_, 'static>) -> Result<(), EncodeError> {
        let value = value_from_dynamic_peek(peek)?;
        self.out
            .extend_from_slice(&encode_dynamic_value_to_vec(&value)?);
        Ok(())
    }

    // r[impl binette.aggregate.struct.compact]
    fn encode_struct(
        &mut self,
        peek: Peek<'_, 'static>,
        fields: &[WriterFieldPlan],
    ) -> Result<(), EncodeError> {
        let struct_peek = peek.into_struct()?;
        for field in fields {
            let field_peek = struct_peek
                .field(field.facet_index)
                .map_err(|source| field_error(peek.shape(), &field.name, source))?;
            self.encode_node(field_peek, &field.node)?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.enum.compact]
    fn encode_enum(
        &mut self,
        peek: Peek<'_, 'static>,
        variants: &[WriterVariantPlan],
    ) -> Result<(), EncodeError> {
        let enum_peek = peek.into_enum()?;
        let facet_index = enum_peek
            .variant_index()
            .map_err(|_| unsupported_peek(peek, "enum variant index is not available"))?;
        let variant = variants
            .iter()
            .find(|variant| variant.facet_index == facet_index)
            .ok_or_else(|| unsupported_peek(peek, "active enum variant is not in writer plan"))?;

        write_u32(self.out, variant.wire_index as usize)?;
        match &variant.payload {
            WriterVariantPayloadPlan::Unit => Ok(()),
            WriterVariantPayloadPlan::Newtype(element) => {
                let field = enum_peek
                    .field(element.facet_index)
                    .map_err(|_| unsupported_peek(peek, "enum payload field is not available"))?
                    .ok_or_else(|| unsupported_peek(peek, "enum payload field is missing"))?;
                self.encode_node(field, &element.node)
            }
            WriterVariantPayloadPlan::Tuple(elements) => {
                for element in elements {
                    let field = enum_peek
                        .field(element.facet_index)
                        .map_err(|_| unsupported_peek(peek, "enum tuple field is not available"))?
                        .ok_or_else(|| unsupported_peek(peek, "enum tuple field is missing"))?;
                    self.encode_node(field, &element.node)?;
                }
                Ok(())
            }
            WriterVariantPayloadPlan::Struct(fields) => {
                for field in fields {
                    let field_peek = enum_peek
                        .field(field.facet_index)
                        .map_err(|_| unsupported_peek(peek, "enum struct field is not available"))?
                        .ok_or_else(|| unsupported_peek(peek, "enum struct field is missing"))?;
                    self.encode_node(field_peek, &field.node)?;
                }
                Ok(())
            }
        }
    }

    // r[impl binette.aggregate.tuple]
    fn encode_tuple(
        &mut self,
        peek: Peek<'_, 'static>,
        elements: &[WriterTupleElementPlan],
    ) -> Result<(), EncodeError> {
        let tuple_peek = peek.into_tuple()?;
        for element in elements {
            let element_peek = tuple_peek
                .field(element.facet_index)
                .ok_or_else(|| unsupported_peek(peek, "tuple field is missing"))?;
            self.encode_node(element_peek, &element.node)?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.list]
    fn encode_list(
        &mut self,
        peek: Peek<'_, 'static>,
        element: &WriterNode,
    ) -> Result<(), EncodeError> {
        let list = peek.into_list_like()?;
        write_u32(self.out, list.len())?;
        for item in list.iter() {
            self.encode_node(item, element)?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.set]
    // r[impl binette.aggregate.set-map.canonical]
    // r[impl binette.aggregate.set-map.float-keys]
    fn encode_set(
        &mut self,
        peek: Peek<'_, 'static>,
        element: &WriterNode,
    ) -> Result<(), EncodeError> {
        let set = peek.into_set()?;
        let mut elements = Vec::with_capacity(set.len());
        for item in set.iter() {
            validate_canonical_key(item, element, self.nodes)?;
            elements.push(encode_to_canonical_bytes(item, element, self.nodes)?);
        }

        elements.sort();
        reject_duplicate_canonical_keys(peek, "set", elements.iter().map(Vec::as_slice))?;

        write_u32(self.out, elements.len())?;
        for element in elements {
            self.out.extend_from_slice(&element);
        }
        Ok(())
    }

    // r[impl binette.aggregate.map]
    // r[impl binette.aggregate.set-map.canonical]
    // r[impl binette.aggregate.set-map.float-keys]
    fn encode_map(
        &mut self,
        peek: Peek<'_, 'static>,
        key_node: &WriterNode,
        value_node: &WriterNode,
    ) -> Result<(), EncodeError> {
        let map = peek.into_map()?;
        let mut entries = Vec::with_capacity(map.len());
        for (key, value) in map.iter() {
            validate_canonical_key(key, key_node, self.nodes)?;
            entries.push((
                encode_to_canonical_bytes(key, key_node, self.nodes)?,
                encode_to_canonical_bytes(value, value_node, self.nodes)?,
            ));
        }

        entries.sort_by(|left, right| left.0.cmp(&right.0));
        reject_duplicate_canonical_keys(
            peek,
            "map",
            entries.iter().map(|entry| entry.0.as_slice()),
        )?;

        write_u32(self.out, entries.len())?;
        for (key, value) in entries {
            self.out.extend_from_slice(&key);
            self.out.extend_from_slice(&value);
        }
        Ok(())
    }

    // r[impl binette.aggregate.array]
    fn encode_array(
        &mut self,
        peek: Peek<'_, 'static>,
        dimensions: &[u64],
        element: &WriterNode,
    ) -> Result<(), EncodeError> {
        let array = peek.into_list_like()?;
        let expected = dimensions.iter().try_fold(1usize, |acc, dimension| {
            let dimension = usize::try_from(*dimension)
                .map_err(|_| unsupported_peek(peek, "array dimension exceeds usize"))?;
            acc.checked_mul(dimension)
                .ok_or_else(|| unsupported_peek(peek, "array element count overflows usize"))
        })?;
        if array.len() != expected {
            return Err(unsupported_peek(
                peek,
                "array length does not match writer schema",
            ));
        }
        for item in array.iter() {
            self.encode_node(item, element)?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.option]
    fn encode_option(
        &mut self,
        peek: Peek<'_, 'static>,
        element: &WriterNode,
    ) -> Result<(), EncodeError> {
        let option = peek.into_option()?;
        match option.value() {
            Some(inner) => {
                self.out.push(0x01);
                self.encode_node(inner, element)
            }
            None => {
                self.out.push(0x00);
                Ok(())
            }
        }
    }

    fn encode_primitive(
        &mut self,
        peek: Peek<'_, 'static>,
        primitive: Primitive,
    ) -> Result<(), EncodeError> {
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
            Primitive::Unit => Ok(()),
            Primitive::Never => Err(unsupported_peek(peek, "never has no value")),
            Primitive::Bool => {
                self.out.push(u8::from(*peek.get::<bool>()?));
                Ok(())
            }
            Primitive::U8 => {
                self.out.push(*peek.get::<u8>()?);
                Ok(())
            }
            Primitive::U16 => self.write_bytes(&peek.get::<u16>()?.to_le_bytes()),
            Primitive::U32 => self.write_bytes(&peek.get::<u32>()?.to_le_bytes()),
            Primitive::U64 => self.write_bytes(&peek.get::<u64>()?.to_le_bytes()),
            Primitive::U128 => self.write_bytes(&peek.get::<u128>()?.to_le_bytes()),
            Primitive::I8 => self.write_bytes(&peek.get::<i8>()?.to_le_bytes()),
            Primitive::I16 => self.write_bytes(&peek.get::<i16>()?.to_le_bytes()),
            Primitive::I32 => self.write_bytes(&peek.get::<i32>()?.to_le_bytes()),
            Primitive::I64 => self.write_bytes(&peek.get::<i64>()?.to_le_bytes()),
            Primitive::I128 => self.write_bytes(&peek.get::<i128>()?.to_le_bytes()),
            Primitive::F32 => self.write_bytes(&peek.get::<f32>()?.to_le_bytes()),
            Primitive::F64 => self.write_bytes(&peek.get::<f64>()?.to_le_bytes()),
            Primitive::Char => self.write_bytes(&(*peek.get::<char>()? as u32).to_le_bytes()),
            Primitive::String => encode_bytes(
                self.out,
                peek.as_str()
                    .ok_or_else(|| {
                        unsupported_peek(peek, "schema string requires string-like value")
                    })?
                    .as_bytes(),
            ),
            Primitive::Bytes | Primitive::Payload => {
                let bytes = compact_bytes(peek)?
                    .ok_or_else(|| unsupported_peek(peek, "schema bytes requires u8 sequence"))?;
                encode_bytes(self.out, bytes)
            }
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        self.out.extend_from_slice(bytes);
        Ok(())
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

fn schema_shape(mut shape: &'static Shape) -> &'static Shape {
    loop {
        if shape.is_transparent()
            && let Some(inner) = shape.inner
        {
            shape = inner;
            continue;
        }

        if let Def::Pointer(pointer) = shape.def
            && let Some(pointee) = pointer.pointee()
        {
            shape = pointee;
            continue;
        }

        return shape;
    }
}

fn list_element_shape(shape: &'static Shape) -> Result<&'static Shape, EncodeError> {
    match schema_shape(shape).def {
        Def::List(list) => Ok(list.t()),
        Def::Slice(slice) => Ok(slice.t()),
        _ => Err(unsupported_shape(
            shape,
            "schema list requires Facet list shape",
        )),
    }
}

fn set_element_shape(shape: &'static Shape) -> Result<&'static Shape, EncodeError> {
    match schema_shape(shape).def {
        Def::Set(set) => Ok(set.t()),
        _ => Err(unsupported_shape(
            shape,
            "schema set requires Facet set shape",
        )),
    }
}

fn map_shapes(shape: &'static Shape) -> Result<(&'static Shape, &'static Shape), EncodeError> {
    match schema_shape(shape).def {
        Def::Map(map) => Ok((map.k(), map.v())),
        _ => Err(unsupported_shape(
            shape,
            "schema map requires Facet map shape",
        )),
    }
}

fn array_element_shape(shape: &'static Shape) -> Result<&'static Shape, EncodeError> {
    match schema_shape(shape).def {
        Def::Array(array) => Ok(array.t()),
        _ => Err(unsupported_shape(
            shape,
            "schema array requires Facet array shape",
        )),
    }
}

fn option_element_shape(shape: &'static Shape) -> Result<&'static Shape, EncodeError> {
    match schema_shape(shape).def {
        Def::Option(option) => Ok(option.t()),
        _ => Err(unsupported_shape(
            shape,
            "schema option requires Facet option shape",
        )),
    }
}

fn compact_bytes<'mem>(peek: Peek<'mem, 'static>) -> Result<Option<&'mem [u8]>, EncodeError> {
    match peek.shape().def {
        Def::List(_) | Def::Slice(_) => Ok(peek.into_list_like()?.as_bytes()),
        _ => Ok(None),
    }
}

fn encode_to_canonical_bytes(
    peek: Peek<'_, 'static>,
    node: &WriterNode,
    nodes: &[WriterNode],
) -> Result<Vec<u8>, EncodeError> {
    let mut bytes = Vec::new();
    WriterPlanExecutor {
        out: &mut bytes,
        nodes,
    }
    .encode_node(peek, node)?;
    Ok(bytes)
}

fn reject_duplicate_canonical_keys<'a>(
    peek: Peek<'_, 'static>,
    aggregate: &'static str,
    keys: impl Iterator<Item = &'a [u8]>,
) -> Result<(), EncodeError> {
    let mut previous = None;
    for key in keys {
        if previous == Some(key) {
            return Err(EncodeError::DuplicateCanonicalKey {
                shape: peek.shape(),
                aggregate,
            });
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_canonical_key(
    peek: Peek<'_, 'static>,
    node: &WriterNode,
    nodes: &[WriterNode],
) -> Result<(), EncodeError> {
    let peek = peek.innermost_peek();
    match node {
        WriterNode::Ref { node_index } => {
            let node = nodes.get(*node_index).ok_or(EncodeError::InvalidPlan {
                reason: "recursive writer node reference is out of range",
            })?;
            validate_canonical_key(peek, node, nodes)?;
        }
        WriterNode::Primitive(Primitive::F32) => {
            if peek.get::<f32>()?.is_nan() {
                return Err(EncodeError::NanCanonicalKey {
                    shape: peek.shape(),
                });
            }
        }
        WriterNode::Primitive(Primitive::F64) => {
            if peek.get::<f64>()?.is_nan() {
                return Err(EncodeError::NanCanonicalKey {
                    shape: peek.shape(),
                });
            }
        }
        WriterNode::Primitive(_) | WriterNode::Dynamic | WriterNode::External => {}
        WriterNode::Struct { fields } => {
            let struct_peek = peek.into_struct()?;
            for field in fields {
                validate_canonical_key(
                    struct_peek
                        .field(field.facet_index)
                        .map_err(|source| field_error(peek.shape(), &field.name, source))?,
                    &field.node,
                    nodes,
                )?;
            }
        }
        WriterNode::Enum { variants } => {
            let enum_peek = peek.into_enum()?;
            let facet_index = enum_peek
                .variant_index()
                .map_err(|_| unsupported_peek(peek, "enum variant index is not available"))?;
            if let Some(variant) = variants
                .iter()
                .find(|variant| variant.facet_index == facet_index)
            {
                validate_variant_payload_key(peek, enum_peek, &variant.payload, nodes)?;
            }
        }
        WriterNode::Tuple { elements } => {
            let tuple_peek = peek.into_tuple()?;
            for element in elements {
                let element_peek = tuple_peek
                    .field(element.facet_index)
                    .ok_or_else(|| unsupported_peek(peek, "tuple field is missing"))?;
                validate_canonical_key(element_peek, &element.node, nodes)?;
            }
        }
        WriterNode::List { element } => {
            for item in peek.into_list_like()?.iter() {
                validate_canonical_key(item, element, nodes)?;
            }
        }
        WriterNode::Set { element } => {
            for item in peek.into_set()?.iter() {
                validate_canonical_key(item, element, nodes)?;
            }
        }
        WriterNode::Map { key, value } => {
            for (item_key, item_value) in peek.into_map()?.iter() {
                validate_canonical_key(item_key, key, nodes)?;
                validate_canonical_key(item_value, value, nodes)?;
            }
        }
        WriterNode::Array { element, .. } => {
            for item in peek.into_list_like()?.iter() {
                validate_canonical_key(item, element, nodes)?;
            }
        }
        WriterNode::Option { element } => {
            if let Some(inner) = peek.into_option()?.value() {
                validate_canonical_key(inner, element, nodes)?;
            }
        }
    }
    Ok(())
}

fn validate_variant_payload_key(
    enum_shape: Peek<'_, 'static>,
    enum_peek: facet_reflect::PeekEnum<'_, 'static>,
    payload: &WriterVariantPayloadPlan,
    nodes: &[WriterNode],
) -> Result<(), EncodeError> {
    match payload {
        WriterVariantPayloadPlan::Unit => Ok(()),
        WriterVariantPayloadPlan::Newtype(element) => {
            let field = enum_peek
                .field(element.facet_index)
                .map_err(|_| unsupported_peek(enum_shape, "enum payload field is not available"))?
                .ok_or_else(|| unsupported_peek(enum_shape, "enum payload field is missing"))?;
            validate_canonical_key(field, &element.node, nodes)
        }
        WriterVariantPayloadPlan::Tuple(elements) => {
            for element in elements {
                let field = enum_peek
                    .field(element.facet_index)
                    .map_err(|_| unsupported_peek(enum_shape, "enum tuple field is not available"))?
                    .ok_or_else(|| unsupported_peek(enum_shape, "enum tuple field is missing"))?;
                validate_canonical_key(field, &element.node, nodes)?;
            }
            Ok(())
        }
        WriterVariantPayloadPlan::Struct(fields) => {
            for field in fields {
                let field_peek = enum_peek
                    .field(field.facet_index)
                    .map_err(|_| {
                        unsupported_peek(enum_shape, "enum struct field is not available")
                    })?
                    .ok_or_else(|| unsupported_peek(enum_shape, "enum struct field is missing"))?;
                validate_canonical_key(field_peek, &field.node, nodes)?;
            }
            Ok(())
        }
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

fn field_error(shape: &'static Shape, field: &str, source: FieldError) -> EncodeError {
    match source {
        FieldError::NoSuchField | FieldError::IndexOutOfBounds { .. } => {
            EncodeError::MissingField {
                shape,
                field: field.to_owned(),
            }
        }
        _ => unsupported_shape(shape, "planned field is not available"),
    }
}

fn unsupported_shape(shape: &'static Shape, reason: &'static str) -> EncodeError {
    EncodeError::Unsupported { shape, reason }
}

fn unsupported_peek(peek: Peek<'_, 'static>, reason: &'static str) -> EncodeError {
    unsupported_shape(peek.shape(), reason)
}

fn value_from_dynamic_peek(peek: Peek<'_, 'static>) -> Result<Value, EncodeError> {
    value_from_dynamic(peek.into_dynamic_value()?)
}

fn value_from_dynamic(dynamic: PeekDynamicValue<'_, 'static>) -> Result<Value, EncodeError> {
    match dynamic.kind() {
        DynValueKind::Null => Ok(Value::Unit),
        DynValueKind::Bool => dynamic
            .as_bool()
            .map(Value::Bool)
            .ok_or_else(|| unsupported_peek(dynamic.peek(), "dynamic bool is unavailable")),
        DynValueKind::Number => {
            if let Some(value) = dynamic.as_i64() {
                Ok(Value::I64(value))
            } else if let Some(value) = dynamic.as_u64() {
                Ok(Value::U64(value))
            } else if let Some(value) = dynamic.as_f64() {
                Ok(Value::F64(value))
            } else {
                Err(unsupported_peek(
                    dynamic.peek(),
                    "dynamic number is unavailable",
                ))
            }
        }
        DynValueKind::String => dynamic
            .as_str()
            .map(|value| Value::String(value.to_owned()))
            .ok_or_else(|| unsupported_peek(dynamic.peek(), "dynamic string is unavailable")),
        DynValueKind::Bytes => dynamic
            .as_bytes()
            .map(|value| Value::Bytes(value.to_vec()))
            .ok_or_else(|| unsupported_peek(dynamic.peek(), "dynamic bytes are unavailable")),
        DynValueKind::Array => {
            let iter = dynamic
                .array_iter()
                .ok_or_else(|| unsupported_peek(dynamic.peek(), "dynamic array is unavailable"))?;
            Ok(Value::List(
                iter.map(value_from_dynamic_peek)
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        DynValueKind::Object => {
            let iter = dynamic
                .object_iter()
                .ok_or_else(|| unsupported_peek(dynamic.peek(), "dynamic object is unavailable"))?;
            Ok(Value::Struct(
                iter.map(|(name, value)| {
                    Ok(crate::value::FieldValue {
                        name: name.to_owned(),
                        value: value_from_dynamic_peek(value)?,
                    })
                })
                .collect::<Result<Vec<_>, EncodeError>>()?,
            ))
        }
        DynValueKind::DateTime | DynValueKind::QName | DynValueKind::Uuid => Err(unsupported_peek(
            dynamic.peek(),
            "dynamic extended value kind is not supported yet",
        )),
    }
}
