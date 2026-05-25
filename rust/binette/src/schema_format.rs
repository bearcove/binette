use thiserror::Error;

use crate::error::SchemaError;
use crate::hash::schema_type_id;
use crate::schema::{
    AttachmentDeclaration, Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef,
    Variant, VariantPayload,
};
use crate::value::{
    EnumValue, FieldValue, SelfDescribingError, Value, decode_self_described_from_slice,
    encode_self_described_to_vec,
};

#[derive(Debug, Error)]
pub enum SchemaFormatError {
    #[error(transparent)]
    SelfDescribing(#[from] SelfDescribingError),

    #[error(transparent)]
    Schema(#[from] SchemaError),

    #[error("expected {expected} for {context}, got {found}")]
    Expected {
        context: &'static str,
        expected: &'static str,
        found: &'static str,
    },

    #[error("missing field {field} in {context}")]
    MissingField {
        context: &'static str,
        field: &'static str,
    },

    #[error("duplicate field {field} in {context}")]
    DuplicateField {
        context: &'static str,
        field: String,
    },

    #[error("unexpected field {field} in {context}")]
    UnexpectedField {
        context: &'static str,
        field: String,
    },

    #[error("unknown {context} variant {variant}")]
    UnknownVariant {
        context: &'static str,
        variant: String,
    },

    #[error("unknown primitive tag {tag}")]
    UnknownPrimitive { tag: String },

    #[error("{context} list must not be empty")]
    EmptyList { context: &'static str },
}

// r[impl binette.schema.encoding.self-describing]
// r[impl binette.schema.format+2]
pub fn schema_to_value(schema: &Schema) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field("id", Value::U64(schema.id.0)),
        field(
            "type_params",
            Value::List(
                schema
                    .type_params
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        ),
        field("kind", schema_kind_to_value(&schema.kind)?),
    ]))
}

// r[impl binette.schema.encoding.self-describing]
// r[impl binette.schema.format+2]
pub fn schema_from_value(value: &Value) -> Result<Schema, SchemaFormatError> {
    let fields = expect_struct(value, "schema", &["id", "type_params", "kind"])?;
    let schema = Schema {
        id: TypeId(expect_u64(fields[0], "schema.id")?),
        type_params: expect_string_list(fields[1], "schema.type_params")?,
        kind: schema_kind_from_value(fields[2])?,
    };

    let computed = schema_type_id(&schema)?;
    if computed != schema.id {
        return Err(SchemaError::SchemaIdMismatch {
            declared: schema.id,
            computed,
        }
        .into());
    }

    Ok(schema)
}

pub fn encode_schema_to_vec(schema: &Schema) -> Result<Vec<u8>, SchemaFormatError> {
    Ok(encode_self_described_to_vec(&schema_to_value(schema)?)?)
}

pub fn decode_schema_from_slice(input: &[u8]) -> Result<Schema, SchemaFormatError> {
    schema_from_value(&decode_self_described_from_slice(input)?)
}

// r[impl binette.bundle.format]
pub fn schema_bundle_to_value(bundle: &SchemaBundle) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field(
            "schemas",
            Value::List(
                bundle
                    .schemas
                    .iter()
                    .map(schema_to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        ),
        field("root", type_ref_to_value(&bundle.root)?),
        field(
            "attachments",
            Value::List(
                bundle
                    .attachments
                    .iter()
                    .map(attachment_to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        ),
    ]))
}

// r[impl binette.bundle.format]
pub fn schema_bundle_from_value(value: &Value) -> Result<SchemaBundle, SchemaFormatError> {
    let fields = expect_struct(value, "schema bundle", &["schemas", "root", "attachments"])?;
    Ok(SchemaBundle {
        schemas: expect_list(fields[0], "schema bundle.schemas")?
            .iter()
            .map(schema_from_value)
            .collect::<Result<Vec<_>, _>>()?,
        root: type_ref_from_value(fields[1])?,
        attachments: expect_list(fields[2], "schema bundle.attachments")?
            .iter()
            .map(attachment_from_value)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub fn encode_schema_bundle_to_vec(bundle: &SchemaBundle) -> Result<Vec<u8>, SchemaFormatError> {
    Ok(encode_self_described_to_vec(&schema_bundle_to_value(
        bundle,
    )?)?)
}

pub fn decode_schema_bundle_from_slice(input: &[u8]) -> Result<SchemaBundle, SchemaFormatError> {
    schema_bundle_from_value(&decode_self_described_from_slice(input)?)
}

// r[impl binette.schema.format.type-ref+2]
pub fn type_ref_to_value(type_ref: &TypeRef) -> Result<Value, SchemaFormatError> {
    match type_ref {
        TypeRef::Concrete { type_id, args } => Ok(Value::Enum(EnumValue {
            variant: "concrete".to_owned(),
            payload: Box::new(Value::Struct(vec![
                field("type_id", Value::U64(type_id.0)),
                field(
                    "args",
                    Value::List(
                        args.iter()
                            .map(type_ref_to_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                ),
            ])),
        })),
        TypeRef::Var { name } => Ok(Value::Enum(EnumValue {
            variant: "var".to_owned(),
            payload: Box::new(Value::String(name.clone())),
        })),
    }
}

// r[impl binette.schema.format.type-ref+2]
pub fn type_ref_from_value(value: &Value) -> Result<TypeRef, SchemaFormatError> {
    let enum_value = expect_enum(value, "type reference")?;
    match enum_value.variant.as_str() {
        "concrete" => {
            let fields = expect_struct(
                &enum_value.payload,
                "type reference concrete",
                &["type_id", "args"],
            )?;
            Ok(TypeRef::generic(
                TypeId(expect_u64(fields[0], "type reference concrete.type_id")?),
                expect_list(fields[1], "type reference concrete.args")?
                    .iter()
                    .map(type_ref_from_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        "var" => Ok(TypeRef::Var {
            name: expect_string(&enum_value.payload, "type reference var")?.to_owned(),
        }),
        other => Err(SchemaFormatError::UnknownVariant {
            context: "type reference",
            variant: other.to_owned(),
        }),
    }
}

// r[impl binette.schema.format.kind+2]
fn schema_kind_to_value(kind: &SchemaKind) -> Result<Value, SchemaFormatError> {
    match kind {
        SchemaKind::Primitive(primitive) => enum_value(
            "primitive",
            Value::String(primitive_tag(*primitive).to_owned()),
        ),
        SchemaKind::Struct { name, fields } => enum_value(
            "struct",
            Value::Struct(vec![
                field("name", Value::String(name.clone())),
                field("fields", fields_to_value(fields)?),
            ]),
        ),
        SchemaKind::Enum { name, variants } => enum_value(
            "enum",
            Value::Struct(vec![
                field("name", Value::String(name.clone())),
                field(
                    "variants",
                    Value::List(
                        variants
                            .iter()
                            .map(variant_to_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                ),
            ]),
        ),
        SchemaKind::Tuple { elements } => {
            if elements.is_empty() {
                return Err(SchemaFormatError::EmptyList {
                    context: "schema kind tuple",
                });
            }
            enum_value(
                "tuple",
                Value::List(
                    elements
                        .iter()
                        .map(type_ref_to_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            )
        }
        SchemaKind::List { element } => element_schema_kind("list", element),
        SchemaKind::Set { element } => element_schema_kind("set", element),
        SchemaKind::Map { key, value } => enum_value(
            "map",
            Value::Struct(vec![
                field("key", type_ref_to_value(key)?),
                field("value", type_ref_to_value(value)?),
            ]),
        ),
        SchemaKind::Array {
            element,
            dimensions,
        } => {
            if dimensions.is_empty() {
                return Err(SchemaFormatError::EmptyList {
                    context: "schema kind array.dimensions",
                });
            }
            enum_value(
                "array",
                Value::Struct(vec![
                    field("element", type_ref_to_value(element)?),
                    field(
                        "dimensions",
                        Value::List(dimensions.iter().copied().map(Value::U64).collect()),
                    ),
                ]),
            )
        }
        SchemaKind::Option { element } => element_schema_kind("option", element),
        SchemaKind::Dynamic => enum_value("dynamic", Value::Unit),
        SchemaKind::External { kind, metadata } => enum_value(
            "external",
            Value::Struct(vec![
                field("kind", Value::String(kind.clone())),
                field("metadata", Value::Dynamic(Box::new(metadata.clone()))),
            ]),
        ),
    }
}

// r[impl binette.schema.format.kind+2]
fn schema_kind_from_value(value: &Value) -> Result<SchemaKind, SchemaFormatError> {
    let enum_value = expect_enum(value, "schema kind")?;
    match enum_value.variant.as_str() {
        "primitive" => Ok(SchemaKind::Primitive(primitive_from_tag(expect_string(
            &enum_value.payload,
            "schema kind primitive",
        )?)?)),
        "struct" => {
            let fields = expect_struct(
                &enum_value.payload,
                "schema kind struct",
                &["name", "fields"],
            )?;
            Ok(SchemaKind::Struct {
                name: expect_string(fields[0], "schema kind struct.name")?.to_owned(),
                fields: fields_from_value(fields[1])?,
            })
        }
        "enum" => {
            let fields = expect_struct(
                &enum_value.payload,
                "schema kind enum",
                &["name", "variants"],
            )?;
            Ok(SchemaKind::Enum {
                name: expect_string(fields[0], "schema kind enum.name")?.to_owned(),
                variants: expect_list(fields[1], "schema kind enum.variants")?
                    .iter()
                    .map(variant_from_value)
                    .collect::<Result<Vec<_>, _>>()?,
            })
        }
        "tuple" => {
            let elements = expect_list(&enum_value.payload, "schema kind tuple")?
                .iter()
                .map(type_ref_from_value)
                .collect::<Result<Vec<_>, _>>()?;
            if elements.is_empty() {
                return Err(SchemaFormatError::EmptyList {
                    context: "schema kind tuple",
                });
            }
            Ok(SchemaKind::Tuple { elements })
        }
        "list" => element_schema_kind_from_value(&enum_value.payload, "schema kind list")
            .map(|element| SchemaKind::List { element }),
        "set" => element_schema_kind_from_value(&enum_value.payload, "schema kind set")
            .map(|element| SchemaKind::Set { element }),
        "map" => {
            let fields = expect_struct(&enum_value.payload, "schema kind map", &["key", "value"])?;
            Ok(SchemaKind::Map {
                key: type_ref_from_value(fields[0])?,
                value: type_ref_from_value(fields[1])?,
            })
        }
        "array" => {
            let fields = expect_struct(
                &enum_value.payload,
                "schema kind array",
                &["element", "dimensions"],
            )?;
            let dimensions = expect_list(fields[1], "schema kind array.dimensions")?
                .iter()
                .map(|value| expect_u64(value, "schema kind array.dimension"))
                .collect::<Result<Vec<_>, _>>()?;
            if dimensions.is_empty() {
                return Err(SchemaFormatError::EmptyList {
                    context: "schema kind array.dimensions",
                });
            }
            Ok(SchemaKind::Array {
                element: type_ref_from_value(fields[0])?,
                dimensions,
            })
        }
        "option" => element_schema_kind_from_value(&enum_value.payload, "schema kind option")
            .map(|element| SchemaKind::Option { element }),
        "dynamic" => {
            expect_unit(&enum_value.payload, "schema kind dynamic")?;
            Ok(SchemaKind::Dynamic)
        }
        "external" => {
            let fields = expect_struct(
                &enum_value.payload,
                "schema kind external",
                &["kind", "metadata"],
            )?;
            Ok(SchemaKind::External {
                kind: expect_string(fields[0], "schema kind external.kind")?.to_owned(),
                metadata: expect_dynamic(fields[1], "schema kind external.metadata")?.clone(),
            })
        }
        other => Err(SchemaFormatError::UnknownVariant {
            context: "schema kind",
            variant: other.to_owned(),
        }),
    }
}

// r[impl binette.schema.format.fields+2]
fn fields_to_value(fields: &[Field]) -> Result<Value, SchemaFormatError> {
    Ok(Value::List(
        fields
            .iter()
            .map(field_to_value)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

// r[impl binette.schema.format.fields+2]
fn fields_from_value(value: &Value) -> Result<Vec<Field>, SchemaFormatError> {
    expect_list(value, "field descriptors")?
        .iter()
        .map(field_from_value)
        .collect()
}

fn field_to_value(field_: &Field) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field("name", Value::String(field_.name.clone())),
        field("type_ref", type_ref_to_value(&field_.type_ref)?),
    ]))
}

fn field_from_value(value: &Value) -> Result<Field, SchemaFormatError> {
    let fields = expect_struct(value, "field descriptor", &["name", "type_ref"])?;
    Ok(Field {
        name: expect_string(fields[0], "field descriptor.name")?.to_owned(),
        type_ref: type_ref_from_value(fields[1])?,
    })
}

// r[impl binette.schema.format.variants+2]
fn variant_to_value(variant: &Variant) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field("name", Value::String(variant.name.clone())),
        field("index", Value::U32(variant.index)),
        field("payload", variant_payload_to_value(&variant.payload)?),
    ]))
}

// r[impl binette.schema.format.variants+2]
fn variant_from_value(value: &Value) -> Result<Variant, SchemaFormatError> {
    let fields = expect_struct(value, "variant descriptor", &["name", "index", "payload"])?;
    Ok(Variant {
        name: expect_string(fields[0], "variant descriptor.name")?.to_owned(),
        index: expect_u32(fields[1], "variant descriptor.index")?,
        payload: variant_payload_from_value(fields[2])?,
    })
}

fn variant_payload_to_value(payload: &VariantPayload) -> Result<Value, SchemaFormatError> {
    match payload {
        VariantPayload::Unit => enum_value("unit", Value::Unit),
        VariantPayload::Newtype { type_ref } => enum_value("newtype", type_ref_to_value(type_ref)?),
        VariantPayload::Tuple { elements } => enum_value(
            "tuple",
            Value::List(
                elements
                    .iter()
                    .map(type_ref_to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        ),
        VariantPayload::Struct { fields } => enum_value("struct", fields_to_value(fields)?),
    }
}

fn variant_payload_from_value(value: &Value) -> Result<VariantPayload, SchemaFormatError> {
    let enum_value = expect_enum(value, "variant payload")?;
    match enum_value.variant.as_str() {
        "unit" => {
            expect_unit(&enum_value.payload, "variant payload unit")?;
            Ok(VariantPayload::Unit)
        }
        "newtype" => Ok(VariantPayload::Newtype {
            type_ref: type_ref_from_value(&enum_value.payload)?,
        }),
        "tuple" => Ok(VariantPayload::Tuple {
            elements: expect_list(&enum_value.payload, "variant payload tuple")?
                .iter()
                .map(type_ref_from_value)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        "struct" => Ok(VariantPayload::Struct {
            fields: fields_from_value(&enum_value.payload)?,
        }),
        other => Err(SchemaFormatError::UnknownVariant {
            context: "variant payload",
            variant: other.to_owned(),
        }),
    }
}

// r[impl binette.bundle.attachments]
fn attachment_to_value(attachment: &AttachmentDeclaration) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field("kind", Value::String(attachment.kind.clone())),
        field(
            "metadata_schema",
            Value::Option(
                attachment
                    .metadata_schema
                    .as_ref()
                    .map(type_ref_to_value)
                    .transpose()?
                    .map(Box::new),
            ),
        ),
    ]))
}

// r[impl binette.bundle.attachments]
fn attachment_from_value(value: &Value) -> Result<AttachmentDeclaration, SchemaFormatError> {
    let fields = expect_struct(
        value,
        "attachment declaration",
        &["kind", "metadata_schema"],
    )?;
    Ok(AttachmentDeclaration {
        kind: expect_string(fields[0], "attachment declaration.kind")?.to_owned(),
        metadata_schema: match fields[1] {
            Value::Option(None) => None,
            Value::Option(Some(type_ref)) => Some(type_ref_from_value(type_ref)?),
            other => {
                return Err(SchemaFormatError::Expected {
                    context: "attachment declaration.metadata_schema",
                    expected: "option",
                    found: value_kind(other),
                });
            }
        },
    })
}

fn element_schema_kind(
    variant: &'static str,
    element: &TypeRef,
) -> Result<Value, SchemaFormatError> {
    enum_value(
        variant,
        Value::Struct(vec![field("element", type_ref_to_value(element)?)]),
    )
}

fn element_schema_kind_from_value(
    value: &Value,
    context: &'static str,
) -> Result<TypeRef, SchemaFormatError> {
    let fields = expect_struct(value, context, &["element"])?;
    type_ref_from_value(fields[0])
}

fn enum_value(variant: &'static str, payload: Value) -> Result<Value, SchemaFormatError> {
    Ok(Value::Enum(EnumValue {
        variant: variant.to_owned(),
        payload: Box::new(payload),
    }))
}

fn field(name: &'static str, value: Value) -> FieldValue {
    FieldValue {
        name: name.to_owned(),
        value,
    }
}

fn expect_struct<'a>(
    value: &'a Value,
    context: &'static str,
    expected_fields: &[&'static str],
) -> Result<Vec<&'a Value>, SchemaFormatError> {
    let Value::Struct(fields) = value else {
        return Err(SchemaFormatError::Expected {
            context,
            expected: "struct",
            found: value_kind(value),
        });
    };

    let mut result = vec![None; expected_fields.len()];
    for field in fields {
        let Some(index) = expected_fields
            .iter()
            .position(|expected| *expected == field.name)
        else {
            return Err(SchemaFormatError::UnexpectedField {
                context,
                field: field.name.clone(),
            });
        };
        if result[index].replace(&field.value).is_some() {
            return Err(SchemaFormatError::DuplicateField {
                context,
                field: field.name.clone(),
            });
        }
    }

    result
        .into_iter()
        .zip(expected_fields)
        .map(|(value, field)| value.ok_or(SchemaFormatError::MissingField { context, field }))
        .collect()
}

fn expect_enum<'a>(
    value: &'a Value,
    context: &'static str,
) -> Result<&'a EnumValue, SchemaFormatError> {
    let Value::Enum(value) = value else {
        return Err(SchemaFormatError::Expected {
            context,
            expected: "enum",
            found: value_kind(value),
        });
    };
    Ok(value)
}

fn expect_list<'a>(
    value: &'a Value,
    context: &'static str,
) -> Result<&'a [Value], SchemaFormatError> {
    let Value::List(values) = value else {
        return Err(SchemaFormatError::Expected {
            context,
            expected: "list",
            found: value_kind(value),
        });
    };
    Ok(values)
}

fn expect_string<'a>(
    value: &'a Value,
    context: &'static str,
) -> Result<&'a str, SchemaFormatError> {
    let Value::String(value) = value else {
        return Err(SchemaFormatError::Expected {
            context,
            expected: "string",
            found: value_kind(value),
        });
    };
    Ok(value)
}

fn expect_string_list(
    value: &Value,
    context: &'static str,
) -> Result<Vec<String>, SchemaFormatError> {
    expect_list(value, context)?
        .iter()
        .map(|value| expect_string(value, context).map(str::to_owned))
        .collect()
}

fn expect_u64(value: &Value, context: &'static str) -> Result<u64, SchemaFormatError> {
    let Value::U64(value) = value else {
        return Err(SchemaFormatError::Expected {
            context,
            expected: "u64",
            found: value_kind(value),
        });
    };
    Ok(*value)
}

fn expect_u32(value: &Value, context: &'static str) -> Result<u32, SchemaFormatError> {
    let Value::U32(value) = value else {
        return Err(SchemaFormatError::Expected {
            context,
            expected: "u32",
            found: value_kind(value),
        });
    };
    Ok(*value)
}

fn expect_unit(value: &Value, context: &'static str) -> Result<(), SchemaFormatError> {
    if matches!(value, Value::Unit) {
        Ok(())
    } else {
        Err(SchemaFormatError::Expected {
            context,
            expected: "unit",
            found: value_kind(value),
        })
    }
}

fn expect_dynamic<'a>(
    value: &'a Value,
    context: &'static str,
) -> Result<&'a Value, SchemaFormatError> {
    let Value::Dynamic(value) = value else {
        return Err(SchemaFormatError::Expected {
            context,
            expected: "dynamic value",
            found: value_kind(value),
        });
    };
    Ok(value)
}

fn primitive_tag(primitive: Primitive) -> &'static str {
    match primitive {
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

fn primitive_from_tag(tag: &str) -> Result<Primitive, SchemaFormatError> {
    match tag {
        "bool" => Ok(Primitive::Bool),
        "u8" => Ok(Primitive::U8),
        "u16" => Ok(Primitive::U16),
        "u32" => Ok(Primitive::U32),
        "u64" => Ok(Primitive::U64),
        "u128" => Ok(Primitive::U128),
        "i8" => Ok(Primitive::I8),
        "i16" => Ok(Primitive::I16),
        "i32" => Ok(Primitive::I32),
        "i64" => Ok(Primitive::I64),
        "i128" => Ok(Primitive::I128),
        "f32" => Ok(Primitive::F32),
        "f64" => Ok(Primitive::F64),
        "char" => Ok(Primitive::Char),
        "string" => Ok(Primitive::String),
        "unit" => Ok(Primitive::Unit),
        "never" => Ok(Primitive::Never),
        "bytes" => Ok(Primitive::Bytes),
        "payload" => Ok(Primitive::Payload),
        other => Err(SchemaFormatError::UnknownPrimitive {
            tag: other.to_owned(),
        }),
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Unit => "unit",
        Value::Bool(_) => "bool",
        Value::U8(_) => "u8",
        Value::U16(_) => "u16",
        Value::U32(_) => "u32",
        Value::U64(_) => "u64",
        Value::U128(_) => "u128",
        Value::I8(_) => "i8",
        Value::I16(_) => "i16",
        Value::I32(_) => "i32",
        Value::I64(_) => "i64",
        Value::I128(_) => "i128",
        Value::F32(_) => "f32",
        Value::F64(_) => "f64",
        Value::Char(_) => "char",
        Value::String(_) => "string",
        Value::Bytes(_) => "bytes",
        Value::Payload(_) => "payload",
        Value::List(_) => "list",
        Value::Set(_) => "set",
        Value::Map(_) => "map",
        Value::Array(_) => "array",
        Value::Tuple(_) => "tuple",
        Value::Struct(_) => "struct",
        Value::Enum(_) => "enum",
        Value::Option(_) => "option",
        Value::Dynamic(_) => "dynamic value",
        Value::ExternalAttachment => "external attachment",
        Value::Extension(_) => "extension",
    }
}
