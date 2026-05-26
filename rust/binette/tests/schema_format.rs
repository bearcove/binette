use binette::{
    AttachmentDeclaration, EnumValue, FieldValue, Primitive, Schema, SchemaBundle,
    SchemaFormatError, SchemaKind, SchemaRegistry, TypeId, TypeRef, Value,
    decode_schema_bundle_from_slice, decode_schema_from_slice, encode_schema_bundle_to_vec,
    encode_schema_to_vec, primitive_type_id, schema_bundle_for, schema_bundle_from_value,
    schema_bundle_to_value, schema_from_value, schema_to_value, schema_type_id,
};
use facet::Facet;

fn concrete_id(type_ref: &TypeRef) -> TypeId {
    match type_ref {
        TypeRef::Concrete { type_id, args } => {
            assert!(args.is_empty(), "unexpected type args in {type_ref:#?}");
            *type_id
        }
        TypeRef::Var { .. } => panic!("expected concrete type ref, got {type_ref:#?}"),
    }
}

fn root_schema(bundle: &SchemaBundle) -> &Schema {
    let root = concrete_id(&bundle.root);
    bundle
        .schemas
        .iter()
        .find(|schema| schema.id == root)
        .unwrap_or_else(|| panic!("schema {root:?} not found in {bundle:#?}"))
}

fn valid_primitive_schema_value() -> Value {
    schema_to_value(&Schema {
        id: primitive_type_id(Primitive::U8),
        type_params: Vec::new(),
        kind: SchemaKind::Primitive(Primitive::U8),
    })
    .unwrap()
}

// r[verify binette.schema.encoding.self-describing]
// r[verify binette.schema.format+2]
// r[verify binette.schema.format.kind+2]
// r[verify binette.schema.format.fields+2]
// r[verify binette.schema.format.type-ref+2]
#[test]
fn schema_values_round_trip_through_self_describing_bytes() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        display_name: String,
        lucky: Option<u32>,
    }

    let bundle = schema_bundle_for::<Account>().unwrap();
    let account = root_schema(&bundle);
    let value = schema_to_value(account).unwrap();

    let Value::Struct(fields) = &value else {
        panic!("schema did not encode as struct: {value:#?}");
    };
    assert_eq!(
        fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "type_params", "kind"]
    );

    let bytes = encode_schema_to_vec(account).unwrap();
    assert_eq!(decode_schema_from_slice(&bytes).unwrap(), *account);
}

// r[verify binette.schema.format.variants+2]
// r[verify binette.schema.format.kind+2]
#[test]
fn schema_format_encodes_enum_variants_and_payloads() {
    #[derive(Facet)]
    #[repr(u8)]
    #[allow(dead_code)]
    enum Event {
        Started,
        Renamed(String),
        Moved(u32, u32),
        Failed { code: u16, message: String },
    }

    let bundle = schema_bundle_for::<Event>().unwrap();
    let event = root_schema(&bundle);
    let value = schema_to_value(event).unwrap();
    let decoded = decode_schema_from_slice(&encode_schema_to_vec(event).unwrap()).unwrap();

    assert_eq!(decoded, *event);
    let Value::Struct(schema_fields) = value else {
        panic!("schema did not encode as struct");
    };
    let kind = &schema_fields[2].value;
    let Value::Enum(EnumValue { variant, payload }) = kind else {
        panic!("schema kind did not encode as enum: {kind:#?}");
    };
    assert_eq!(variant, "enum");
    let Value::Struct(kind_fields) = payload.as_ref() else {
        panic!("enum schema payload did not encode as struct: {payload:#?}");
    };
    let Value::List(variants) = &kind_fields[1].value else {
        panic!("enum variants did not encode as list: {kind_fields:#?}");
    };
    assert_eq!(variants.len(), 4);
}

// r[verify binette.bundle.format]
// r[verify binette.bundle.model]
// r[verify binette.bundle.attachments]
// r[verify binette.bundle.registry]
#[test]
fn schema_bundle_values_round_trip_with_root_and_attachments() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        display_name: String,
    }

    let mut bundle = schema_bundle_for::<Account>().unwrap();
    bundle.attachments.push(AttachmentDeclaration {
        kind: "channel".to_owned(),
        metadata_schema: Some(TypeRef::concrete(primitive_type_id(Primitive::String))),
    });

    let bytes = encode_schema_bundle_to_vec(&bundle).unwrap();
    let decoded = decode_schema_bundle_from_slice(&bytes).unwrap();
    assert_eq!(decoded, bundle);

    let value = schema_bundle_to_value(&decoded).unwrap();
    assert_eq!(schema_bundle_from_value(&value).unwrap(), decoded);

    let mut registry = SchemaRegistry::new();
    registry.install_bundle(&decoded).unwrap();
    assert!(registry.contains(concrete_id(&decoded.root)));
}

// r[verify binette.schema.external]
// r[verify binette.schema.format.kind+2]
// r[verify binette.type-id.hash.external]
#[test]
fn external_schema_metadata_is_a_binette_value() {
    let mut schema = Schema {
        id: TypeId(0),
        type_params: Vec::new(),
        kind: SchemaKind::External {
            kind: "channel".to_owned(),
            metadata: Value::Struct(vec![
                FieldValue {
                    name: "transport".to_owned(),
                    value: Value::String("ordered".to_owned()),
                },
                FieldValue {
                    name: "version".to_owned(),
                    value: Value::U32(1),
                },
            ]),
        },
    };
    schema.id = schema_type_id(&schema).unwrap();

    let bytes = encode_schema_to_vec(&schema).unwrap();
    assert_eq!(decode_schema_from_slice(&bytes).unwrap(), schema);

    let mut registry = SchemaRegistry::new();
    registry
        .install_bundle(&SchemaBundle {
            schemas: vec![schema.clone()],
            root: TypeRef::concrete(schema.id),
            attachments: Vec::new(),
        })
        .unwrap();
    assert!(registry.contains(schema.id));
}

// r[verify binette.schema.format+2]
#[test]
fn schema_format_rejects_extra_duplicate_and_missing_fields() {
    let Value::Struct(mut fields) = valid_primitive_schema_value() else {
        panic!("valid schema value did not encode as struct");
    };
    fields.push(FieldValue {
        name: "extra".to_owned(),
        value: Value::Unit,
    });
    let err = schema_from_value(&Value::Struct(fields)).unwrap_err();
    assert!(matches!(err, SchemaFormatError::UnexpectedField { .. }));

    let Value::Struct(mut fields) = valid_primitive_schema_value() else {
        panic!("valid schema value did not encode as struct");
    };
    fields.push(fields[0].clone());
    let err = decode_schema_from_slice(
        &binette::encode_self_described_to_vec(&Value::Struct(fields)).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(err, SchemaFormatError::DuplicateField { .. }));

    let Value::Struct(mut fields) = valid_primitive_schema_value() else {
        panic!("valid schema value did not encode as struct");
    };
    fields.retain(|field| field.name != "kind");
    let err = decode_schema_from_slice(
        &binette::encode_self_described_to_vec(&Value::Struct(fields)).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(err, SchemaFormatError::MissingField { .. }));
}
