use crate::error::SchemaError;
use crate::schema::{Primitive, Schema, SchemaKind, TypeId, TypeRef, VariantPayload};

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

pub fn primitive_for_type_id(type_id: TypeId) -> Option<Primitive> {
    Primitive::ALL
        .into_iter()
        .find(|primitive| primitive_type_id(*primitive) == type_id)
}

pub(crate) fn type_id_for_kind(
    kind: &SchemaKind,
    type_params: &[String],
) -> Result<TypeId, SchemaError> {
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
    const ALL: [Self; 19] = [
        Self::Bool,
        Self::U8,
        Self::U16,
        Self::U32,
        Self::U64,
        Self::U128,
        Self::I8,
        Self::I16,
        Self::I32,
        Self::I64,
        Self::I128,
        Self::F32,
        Self::F64,
        Self::Char,
        Self::String,
        Self::Unit,
        Self::Never,
        Self::Bytes,
        Self::Payload,
    ];

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
