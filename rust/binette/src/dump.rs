use crate::schema::{SchemaBundle, TypeId};
use crate::schema_format::{SchemaFormatError, schema_bundle_from_value, schema_bundle_to_value};
use crate::value::{
    EnumValue, FieldValue, Value, decode_self_described_from_slice, encode_self_described_to_vec,
};

// r[impl binette.bundle.dump]
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaDump {
    pub bundle: SchemaBundle,
    pub metadata: ProducerMetadata,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ProducerMetadata {
    pub declarations: Vec<DeclarationMetadata>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeclarationMetadata {
    pub type_id: TypeId,
    pub source_name: Option<String>,
    pub documentation: Option<String>,
    pub source_location: Option<String>,
    pub fields: Vec<FieldMetadata>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldMetadata {
    pub name: String,
    pub defaultability: Defaultability,
    pub documentation: Option<String>,
    pub source_location: Option<String>,
}

// r[impl binette.compat.defaultability-metadata]
#[derive(Debug, Clone, PartialEq)]
pub enum Defaultability {
    NoDefault,
    OpaqueDefault,
    LiteralDefault(Value),
}

// r[impl binette.bundle.snapshot]
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaSnapshot {
    pub dumps: Vec<SchemaDump>,
}

// r[impl binette.bundle.dump]
pub fn schema_dump_to_value(dump: &SchemaDump) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field("bundle", schema_bundle_to_value(&dump.bundle)?),
        field("metadata", producer_metadata_to_value(&dump.metadata)?),
    ]))
}

// r[impl binette.bundle.dump]
pub fn schema_dump_from_value(value: &Value) -> Result<SchemaDump, SchemaFormatError> {
    let fields = expect_struct(value, "schema dump", &["bundle", "metadata"])?;
    Ok(SchemaDump {
        bundle: schema_bundle_from_value(fields[0])?,
        metadata: producer_metadata_from_value(fields[1])?,
    })
}

pub fn encode_schema_dump_to_vec(dump: &SchemaDump) -> Result<Vec<u8>, SchemaFormatError> {
    Ok(encode_self_described_to_vec(&schema_dump_to_value(dump)?)?)
}

pub fn decode_schema_dump_from_slice(input: &[u8]) -> Result<SchemaDump, SchemaFormatError> {
    schema_dump_from_value(&decode_self_described_from_slice(input)?)
}

// r[impl binette.bundle.snapshot]
pub fn schema_snapshot_to_value(snapshot: &SchemaSnapshot) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![field(
        "dumps",
        Value::List(
            snapshot
                .dumps
                .iter()
                .map(schema_dump_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    )]))
}

// r[impl binette.bundle.snapshot]
pub fn schema_snapshot_from_value(value: &Value) -> Result<SchemaSnapshot, SchemaFormatError> {
    let fields = expect_struct(value, "schema snapshot", &["dumps"])?;
    Ok(SchemaSnapshot {
        dumps: expect_list(fields[0], "schema snapshot.dumps")?
            .iter()
            .map(schema_dump_from_value)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub fn encode_schema_snapshot_to_vec(
    snapshot: &SchemaSnapshot,
) -> Result<Vec<u8>, SchemaFormatError> {
    Ok(encode_self_described_to_vec(&schema_snapshot_to_value(
        snapshot,
    )?)?)
}

pub fn decode_schema_snapshot_from_slice(
    input: &[u8],
) -> Result<SchemaSnapshot, SchemaFormatError> {
    schema_snapshot_from_value(&decode_self_described_from_slice(input)?)
}

fn producer_metadata_to_value(metadata: &ProducerMetadata) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![field(
        "declarations",
        Value::List(
            metadata
                .declarations
                .iter()
                .map(declaration_metadata_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    )]))
}

fn producer_metadata_from_value(value: &Value) -> Result<ProducerMetadata, SchemaFormatError> {
    let fields = expect_struct(value, "producer metadata", &["declarations"])?;
    Ok(ProducerMetadata {
        declarations: expect_list(fields[0], "producer metadata.declarations")?
            .iter()
            .map(declaration_metadata_from_value)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn declaration_metadata_to_value(
    metadata: &DeclarationMetadata,
) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field("type_id", Value::U64(metadata.type_id.0)),
        field(
            "source_name",
            optional_string_to_value(&metadata.source_name),
        ),
        field(
            "documentation",
            optional_string_to_value(&metadata.documentation),
        ),
        field(
            "source_location",
            optional_string_to_value(&metadata.source_location),
        ),
        field(
            "fields",
            Value::List(
                metadata
                    .fields
                    .iter()
                    .map(field_metadata_to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        ),
    ]))
}

fn declaration_metadata_from_value(
    value: &Value,
) -> Result<DeclarationMetadata, SchemaFormatError> {
    let fields = expect_struct(
        value,
        "declaration metadata",
        &[
            "type_id",
            "source_name",
            "documentation",
            "source_location",
            "fields",
        ],
    )?;
    Ok(DeclarationMetadata {
        type_id: TypeId(expect_u64(fields[0], "declaration metadata.type_id")?),
        source_name: expect_optional_string(fields[1], "declaration metadata.source_name")?,
        documentation: expect_optional_string(fields[2], "declaration metadata.documentation")?,
        source_location: expect_optional_string(fields[3], "declaration metadata.source_location")?,
        fields: expect_list(fields[4], "declaration metadata.fields")?
            .iter()
            .map(field_metadata_from_value)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn field_metadata_to_value(metadata: &FieldMetadata) -> Result<Value, SchemaFormatError> {
    Ok(Value::Struct(vec![
        field("name", Value::String(metadata.name.clone())),
        field(
            "defaultability",
            defaultability_to_value(&metadata.defaultability)?,
        ),
        field(
            "documentation",
            optional_string_to_value(&metadata.documentation),
        ),
        field(
            "source_location",
            optional_string_to_value(&metadata.source_location),
        ),
    ]))
}

fn field_metadata_from_value(value: &Value) -> Result<FieldMetadata, SchemaFormatError> {
    let fields = expect_struct(
        value,
        "field metadata",
        &["name", "defaultability", "documentation", "source_location"],
    )?;
    Ok(FieldMetadata {
        name: expect_string(fields[0], "field metadata.name")?.to_owned(),
        defaultability: defaultability_from_value(fields[1])?,
        documentation: expect_optional_string(fields[2], "field metadata.documentation")?,
        source_location: expect_optional_string(fields[3], "field metadata.source_location")?,
    })
}

fn defaultability_to_value(defaultability: &Defaultability) -> Result<Value, SchemaFormatError> {
    match defaultability {
        Defaultability::NoDefault => enum_value("none", Value::Unit),
        Defaultability::OpaqueDefault => enum_value("opaque", Value::Unit),
        Defaultability::LiteralDefault(value) => {
            enum_value("literal", Value::Dynamic(Box::new(value.clone())))
        }
    }
}

fn defaultability_from_value(value: &Value) -> Result<Defaultability, SchemaFormatError> {
    let enum_value = expect_enum(value, "defaultability")?;
    match enum_value.variant.as_str() {
        "none" => {
            expect_unit(&enum_value.payload, "defaultability none")?;
            Ok(Defaultability::NoDefault)
        }
        "opaque" => {
            expect_unit(&enum_value.payload, "defaultability opaque")?;
            Ok(Defaultability::OpaqueDefault)
        }
        "literal" => {
            let Value::Dynamic(value) = enum_value.payload.as_ref() else {
                return Err(SchemaFormatError::Expected {
                    context: "defaultability literal",
                    expected: "dynamic value",
                    found: value_kind(&enum_value.payload),
                });
            };
            Ok(Defaultability::LiteralDefault((**value).clone()))
        }
        other => Err(SchemaFormatError::UnknownVariant {
            context: "defaultability",
            variant: other.to_owned(),
        }),
    }
}

fn optional_string_to_value(value: &Option<String>) -> Value {
    Value::Option(
        value
            .as_ref()
            .map(|value| Box::new(Value::String(value.clone()))),
    )
}

fn expect_optional_string(
    value: &Value,
    context: &'static str,
) -> Result<Option<String>, SchemaFormatError> {
    match value {
        Value::Option(None) => Ok(None),
        Value::Option(Some(value)) => Ok(Some(expect_string(value, context)?.to_owned())),
        other => Err(SchemaFormatError::Expected {
            context,
            expected: "option",
            found: value_kind(other),
        }),
    }
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
