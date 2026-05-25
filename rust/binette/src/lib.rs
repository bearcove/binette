//! binette is a compact binary value format with schemas, stable type
//! identities, compatibility tooling, and support for long-lived data.

use std::collections::{HashMap, HashSet};

use facet_core::{Def, Facet, ScalarType, Shape, StructKind, Type, UserType};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TypeId(pub u64);

// r[impl binette.schema.type-ref]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeRef {
    Concrete { type_id: TypeId, args: Vec<TypeRef> },
    Var { name: String },
}

impl TypeRef {
    pub fn concrete(type_id: TypeId) -> Self {
        Self::Concrete {
            type_id,
            args: Vec::new(),
        }
    }

    pub fn generic(type_id: TypeId, args: Vec<TypeRef>) -> Self {
        Self::Concrete { type_id, args }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaBundle {
    pub schemas: Vec<Schema>,
    pub root: TypeRef,
    pub attachments: Vec<AttachmentDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentDeclaration {
    pub kind: String,
    pub metadata_schema: Option<TypeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub id: TypeId,
    pub type_params: Vec<String>,
    pub kind: SchemaKind,
}

// r[impl binette.schema.kinds]
// r[impl binette.schema.array]
// r[impl binette.schema.dynamic]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaKind {
    Primitive(Primitive),
    Struct {
        name: String,
        fields: Vec<Field>,
    },
    Enum {
        name: String,
        variants: Vec<Variant>,
    },
    Tuple {
        elements: Vec<TypeRef>,
    },
    List {
        element: TypeRef,
    },
    Set {
        element: TypeRef,
    },
    Map {
        key: TypeRef,
        value: TypeRef,
    },
    Array {
        element: TypeRef,
        dimensions: Vec<u64>,
    },
    Option {
        element: TypeRef,
    },
    Dynamic,
    External {
        kind: String,
        metadata: facet_value::Value,
    },
}

// r[impl binette.schema.fields]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub type_ref: TypeRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: String,
    pub index: u32,
    pub payload: VariantPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantPayload {
    Unit,
    Newtype { type_ref: TypeRef },
    Tuple { elements: Vec<TypeRef> },
    Struct { fields: Vec<Field> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    Char,
    String,
    Unit,
    Never,
    Bytes,
    Payload,
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("unsupported scalar type {scalar:?} for {type_name}")]
    UnsupportedScalar {
        scalar: ScalarType,
        type_name: &'static str,
    },

    #[error("unsupported shape {type_name}: {reason}")]
    UnsupportedShape {
        type_name: &'static str,
        reason: &'static str,
    },

    #[error("recursive schema extraction is not implemented yet for {type_name}")]
    RecursiveSchemaUnsupported { type_name: &'static str },

    #[error("external attachment metadata hashing is not implemented yet")]
    ExternalMetadataHashUnsupported,
}

// r[impl binette.schema.model]
// r[impl binette.bundle.model]
pub fn schema_bundle_for<T: Facet<'static>>() -> Result<SchemaBundle, SchemaError> {
    schema_bundle_for_shape(T::SHAPE)
}

// r[impl binette.schema.model]
// r[impl binette.bundle.model]
pub fn schema_bundle_for_shape(shape: &'static Shape) -> Result<SchemaBundle, SchemaError> {
    let mut ctx = ExtractCtx::default();
    let root = ctx.extract(shape)?;
    Ok(SchemaBundle {
        schemas: ctx.schemas,
        root,
        attachments: Vec::new(),
    })
}

// r[impl binette.type-id.hash.primitives]
pub fn primitive_type_id(primitive: Primitive) -> TypeId {
    let mut hasher = SchemaHasher::default();
    hasher.feed_string(primitive.hash_tag());
    hasher.finish()
}

// r[impl binette.type-id.hash]
// r[impl binette.type-id.hash.typeref]
// r[impl binette.type-id.hash.struct]
// r[impl binette.type-id.hash.enum]
// r[impl binette.type-id.hash.container]
// r[impl binette.type-id.hash.tuple]
// r[impl binette.type-id.hash.dynamic]
pub fn schema_type_id(schema: &Schema) -> Result<TypeId, SchemaError> {
    type_id_for_kind(&schema.kind, &schema.type_params)
}

#[derive(Default)]
struct ExtractCtx {
    schemas: Vec<Schema>,
    emitted_by_user_decl: HashMap<facet_core::DeclId, TypeId>,
    active_user_decls: HashSet<facet_core::DeclId>,
}

impl ExtractCtx {
    // r[impl binette.type-id.context-free]
    fn extract(&mut self, shape: &'static Shape) -> Result<TypeRef, SchemaError> {
        if shape.is_transparent()
            && let Some(inner) = shape.inner
        {
            return self.extract(inner);
        }

        if let Def::Pointer(pointer) = shape.def
            && let Some(pointee) = pointer.pointee
        {
            return self.extract(pointee);
        }

        if let Some(primitive) = primitive_for_scalar(shape)? {
            return Ok(TypeRef::concrete(primitive_type_id(primitive)));
        }

        match shape.def {
            Def::List(list) if scalar_primitive(list.t()) == Some(Primitive::U8) => {
                Ok(TypeRef::concrete(primitive_type_id(Primitive::Bytes)))
            }
            Def::Slice(slice) if scalar_primitive(slice.t()) == Some(Primitive::U8) => {
                Ok(TypeRef::concrete(primitive_type_id(Primitive::Bytes)))
            }
            Def::List(list) => {
                let element = self.extract(list.t())?;
                self.emit_anonymous(SchemaKind::List { element })
            }
            Def::Slice(slice) => {
                let element = self.extract(slice.t())?;
                self.emit_anonymous(SchemaKind::List { element })
            }
            Def::Set(set) => {
                let element = self.extract(set.t())?;
                self.emit_anonymous(SchemaKind::Set { element })
            }
            Def::Map(map) => {
                let key = self.extract(map.k())?;
                let value = self.extract(map.v())?;
                self.emit_anonymous(SchemaKind::Map { key, value })
            }
            Def::Array(array) => {
                let element = self.extract(array.t())?;
                self.emit_anonymous(SchemaKind::Array {
                    element,
                    dimensions: vec![array.n as u64],
                })
            }
            Def::Option(option) => {
                let element = self.extract(option.t())?;
                self.emit_anonymous(SchemaKind::Option { element })
            }
            Def::DynamicValue(_) => self.emit_anonymous(SchemaKind::Dynamic),
            _ => self.extract_user(shape),
        }
    }

    fn extract_user(&mut self, shape: &'static Shape) -> Result<TypeRef, SchemaError> {
        let Type::User(user_type) = shape.ty else {
            return Err(SchemaError::UnsupportedShape {
                type_name: shape.type_identifier,
                reason: "shape is neither a supported container nor a user type",
            });
        };

        let type_id = if let Some(type_id) = self.emitted_by_user_decl.get(&shape.decl_id) {
            *type_id
        } else {
            if !self.active_user_decls.insert(shape.decl_id) {
                return Err(SchemaError::RecursiveSchemaUnsupported {
                    type_name: shape.type_identifier,
                });
            }

            let type_params = type_param_names(shape);
            let param_map = type_param_map(shape);
            let kind = match user_type {
                UserType::Struct(struct_type) => {
                    self.struct_kind(shape, struct_type, &param_map)?
                }
                UserType::Enum(enum_type) => self.enum_kind(shape, enum_type, &param_map)?,
                UserType::Union(_) => {
                    return Err(SchemaError::UnsupportedShape {
                        type_name: shape.type_identifier,
                        reason: "unions are not compact-capable binette schemas",
                    });
                }
                UserType::Opaque => {
                    return Err(SchemaError::UnsupportedShape {
                        type_name: shape.type_identifier,
                        reason: "opaque user types are not compact-capable binette schemas",
                    });
                }
            };

            let type_id = type_id_for_kind(&kind, &type_params)?;
            self.schemas.push(Schema {
                id: type_id,
                type_params,
                kind,
            });
            self.emitted_by_user_decl.insert(shape.decl_id, type_id);
            self.active_user_decls.remove(&shape.decl_id);
            type_id
        };

        let args = self.extract_type_args(shape)?;
        Ok(if args.is_empty() {
            TypeRef::concrete(type_id)
        } else {
            TypeRef::generic(type_id, args)
        })
    }

    fn struct_kind(
        &mut self,
        shape: &'static Shape,
        struct_type: facet_core::StructType,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<SchemaKind, SchemaError> {
        match struct_type.kind {
            StructKind::Tuple => {
                let elements = struct_type
                    .fields
                    .iter()
                    .map(|field| self.type_ref_for_shape(field.shape(), param_map))
                    .collect::<Result<Vec<_>, _>>()?;
                if elements.is_empty() {
                    return Ok(SchemaKind::Primitive(Primitive::Unit));
                }
                Ok(SchemaKind::Tuple { elements })
            }
            StructKind::Unit | StructKind::Struct | StructKind::TupleStruct => {
                Ok(SchemaKind::Struct {
                    name: schema_name(shape),
                    fields: self.fields(struct_type.fields, param_map)?,
                })
            }
        }
    }

    fn enum_kind(
        &mut self,
        shape: &'static Shape,
        enum_type: facet_core::EnumType,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<SchemaKind, SchemaError> {
        let variants = enum_type
            .variants
            .iter()
            .enumerate()
            .map(|(index, variant)| {
                Ok(Variant {
                    name: variant.effective_name().to_owned(),
                    index: index as u32,
                    payload: self.variant_payload(variant.data, param_map)?,
                })
            })
            .collect::<Result<Vec<_>, SchemaError>>()?;

        Ok(SchemaKind::Enum {
            name: schema_name(shape),
            variants,
        })
    }

    fn variant_payload(
        &mut self,
        data: facet_core::StructType,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<VariantPayload, SchemaError> {
        match data.kind {
            StructKind::Unit => Ok(VariantPayload::Unit),
            StructKind::Tuple | StructKind::TupleStruct if data.fields.len() == 1 => {
                Ok(VariantPayload::Newtype {
                    type_ref: self.type_ref_for_shape(data.fields[0].shape(), param_map)?,
                })
            }
            StructKind::Tuple | StructKind::TupleStruct => Ok(VariantPayload::Tuple {
                elements: data
                    .fields
                    .iter()
                    .map(|field| self.type_ref_for_shape(field.shape(), param_map))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            StructKind::Struct => Ok(VariantPayload::Struct {
                fields: self.fields(data.fields, param_map)?,
            }),
        }
    }

    fn fields(
        &mut self,
        fields: &'static [facet_core::Field],
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<Vec<Field>, SchemaError> {
        fields
            .iter()
            .filter(|field| !field.should_skip_serializing_unconditional())
            .map(|field| {
                Ok(Field {
                    name: field.effective_name().to_owned(),
                    type_ref: self.type_ref_for_shape(field.shape(), param_map)?,
                })
            })
            .collect()
    }

    fn type_ref_for_shape(
        &mut self,
        shape: &'static Shape,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<TypeRef, SchemaError> {
        if let Some((_, name)) = param_map.iter().find(|(id, _)| *id == shape.id) {
            Ok(TypeRef::Var { name: name.clone() })
        } else {
            self.extract(shape)
        }
    }

    fn extract_type_args(&mut self, shape: &'static Shape) -> Result<Vec<TypeRef>, SchemaError> {
        shape
            .type_params
            .iter()
            .map(|param| self.extract(param.shape()))
            .collect()
    }

    fn emit_anonymous(&mut self, kind: SchemaKind) -> Result<TypeRef, SchemaError> {
        let id = type_id_for_kind(&kind, &[])?;
        if !self.schemas.iter().any(|schema| schema.id == id) {
            self.schemas.push(Schema {
                id,
                type_params: Vec::new(),
                kind,
            });
        }
        Ok(TypeRef::concrete(id))
    }
}

// r[impl binette.schema.name]
fn schema_name(shape: &'static Shape) -> String {
    shape.type_identifier.to_owned()
}

fn type_param_names(shape: &'static Shape) -> Vec<String> {
    shape
        .type_params
        .iter()
        .map(|param| param.name.to_owned())
        .collect()
}

fn type_param_map(shape: &'static Shape) -> Vec<(facet_core::ConstTypeId, String)> {
    shape
        .type_params
        .iter()
        .map(|param| (param.shape().id, param.name.to_owned()))
        .collect()
}

fn primitive_for_scalar(shape: &'static Shape) -> Result<Option<Primitive>, SchemaError> {
    match shape.scalar_type() {
        Some(scalar) => scalar_to_primitive(scalar, shape.type_identifier).map(Some),
        None => Ok(None),
    }
}

fn scalar_primitive(shape: &'static Shape) -> Option<Primitive> {
    shape
        .scalar_type()
        .and_then(|scalar| scalar_to_primitive(scalar, shape.type_identifier).ok())
}

fn scalar_to_primitive(
    scalar: ScalarType,
    type_name: &'static str,
) -> Result<Primitive, SchemaError> {
    match scalar {
        ScalarType::Unit => Ok(Primitive::Unit),
        ScalarType::Bool => Ok(Primitive::Bool),
        ScalarType::Char => Ok(Primitive::Char),
        ScalarType::Str | ScalarType::String | ScalarType::CowStr => Ok(Primitive::String),
        ScalarType::F32 => Ok(Primitive::F32),
        ScalarType::F64 => Ok(Primitive::F64),
        ScalarType::U8 => Ok(Primitive::U8),
        ScalarType::U16 => Ok(Primitive::U16),
        ScalarType::U32 => Ok(Primitive::U32),
        ScalarType::U64 => Ok(Primitive::U64),
        ScalarType::U128 => Ok(Primitive::U128),
        ScalarType::I8 => Ok(Primitive::I8),
        ScalarType::I16 => Ok(Primitive::I16),
        ScalarType::I32 => Ok(Primitive::I32),
        ScalarType::I64 => Ok(Primitive::I64),
        ScalarType::I128 => Ok(Primitive::I128),
        other => Err(SchemaError::UnsupportedScalar {
            scalar: other,
            type_name,
        }),
    }
}

fn type_id_for_kind(kind: &SchemaKind, type_params: &[String]) -> Result<TypeId, SchemaError> {
    let mut hasher = SchemaHasher::default();
    hasher.feed_kind(kind, type_params)?;
    Ok(hasher.finish())
}

#[derive(Default)]
struct SchemaHasher {
    inner: blake3::Hasher,
}

impl SchemaHasher {
    fn feed_kind(&mut self, kind: &SchemaKind, type_params: &[String]) -> Result<(), SchemaError> {
        match kind {
            SchemaKind::Primitive(primitive) => {
                self.feed_string(primitive.hash_tag());
            }
            SchemaKind::Struct { name, fields } => {
                self.feed_string("struct");
                self.feed_string(name);
                self.feed_type_params(type_params);
                self.feed_len(fields.len());
                for field in fields {
                    self.feed_string(&field.name);
                    self.feed_type_ref(&field.type_ref);
                }
            }
            SchemaKind::Enum { name, variants } => {
                self.feed_string("enum");
                self.feed_string(name);
                self.feed_type_params(type_params);
                self.feed_len(variants.len());
                for variant in variants {
                    self.feed_string(&variant.name);
                    self.feed_u32(variant.index);
                    match &variant.payload {
                        VariantPayload::Unit => {
                            self.feed_string("unit");
                        }
                        VariantPayload::Newtype { type_ref } => {
                            self.feed_string("newtype");
                            self.feed_type_ref(type_ref);
                        }
                        VariantPayload::Tuple { elements } => {
                            self.feed_string("tuple");
                            self.feed_len(elements.len());
                            for element in elements {
                                self.feed_type_ref(element);
                            }
                        }
                        VariantPayload::Struct { fields } => {
                            self.feed_string("struct");
                            self.feed_len(fields.len());
                            for field in fields {
                                self.feed_string(&field.name);
                                self.feed_type_ref(&field.type_ref);
                            }
                        }
                    }
                }
            }
            SchemaKind::Tuple { elements } => {
                self.feed_string("tuple");
                self.feed_len(elements.len());
                for element in elements {
                    self.feed_type_ref(element);
                }
            }
            SchemaKind::List { element } => {
                self.feed_string("list");
                self.feed_type_ref(element);
            }
            SchemaKind::Set { element } => {
                self.feed_string("set");
                self.feed_type_ref(element);
            }
            SchemaKind::Map { key, value } => {
                self.feed_string("map");
                self.feed_type_ref(key);
                self.feed_type_ref(value);
            }
            SchemaKind::Array {
                element,
                dimensions,
            } => {
                self.feed_string("array");
                self.feed_type_ref(element);
                self.feed_len(dimensions.len());
                for dimension in dimensions {
                    self.feed_u64(*dimension);
                }
            }
            SchemaKind::Option { element } => {
                self.feed_string("option");
                self.feed_type_ref(element);
            }
            SchemaKind::Dynamic => {
                self.feed_string("dynamic");
            }
            SchemaKind::External { .. } => {
                return Err(SchemaError::ExternalMetadataHashUnsupported);
            }
        }
        Ok(())
    }

    fn feed_type_params(&mut self, type_params: &[String]) {
        self.feed_len(type_params.len());
        for type_param in type_params {
            self.feed_string(type_param);
        }
    }

    fn feed_type_ref(&mut self, type_ref: &TypeRef) {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                self.feed_string("concrete");
                self.feed_u64(type_id.0);
                if !args.is_empty() {
                    self.feed_string("args");
                    self.feed_len(args.len());
                    for arg in args {
                        self.feed_type_ref(arg);
                    }
                }
            }
            TypeRef::Var { name } => {
                self.feed_string("var");
                self.feed_string(name);
            }
        }
    }

    fn feed_string(&mut self, value: &str) {
        self.feed_len(value.len());
        self.inner.update(value.as_bytes());
    }

    fn feed_len(&mut self, len: usize) {
        self.feed_u32(len as u32);
    }

    fn feed_u32(&mut self, value: u32) {
        self.inner.update(&value.to_le_bytes());
    }

    fn feed_u64(&mut self, value: u64) {
        self.inner.update(&value.to_le_bytes());
    }

    fn finish(self) -> TypeId {
        let hash = self.inner.finalize();
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&hash.as_bytes()[..8]);
        TypeId(u64::from_le_bytes(bytes))
    }
}

impl Primitive {
    fn hash_tag(self) -> &'static str {
        match self {
            Primitive::Bool => "bool",
            Primitive::U8 => "u8",
            Primitive::U16 => "u16",
            Primitive::U32 => "u32",
            Primitive::U64 => "u64",
            Primitive::U128 => "u128",
            Primitive::I8 => "i8",
            Primitive::I16 => "i16",
            Primitive::I32 => "i32",
            Primitive::I64 => "i64",
            Primitive::I128 => "i128",
            Primitive::F32 => "f32",
            Primitive::F64 => "f64",
            Primitive::Char => "char",
            Primitive::String => "string",
            Primitive::Unit => "unit",
            Primitive::Never => "never",
            Primitive::Bytes => "bytes",
            Primitive::Payload => "payload",
        }
    }
}
