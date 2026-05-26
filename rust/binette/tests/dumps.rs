use binette::{
    DeclarationMetadata, Defaultability, EnumValue, FieldMetadata, FieldValue, ProducerMetadata,
    SchemaDump, SchemaFormatError, SchemaSnapshot, TypeId, TypeRef, Value,
    decode_schema_dump_from_slice, decode_schema_snapshot_from_slice, encode_schema_dump_to_vec,
    encode_schema_snapshot_to_vec, schema_bundle_for, schema_dump_from_value, schema_dump_to_value,
    schema_snapshot_from_value, schema_snapshot_to_value, schema_type_id,
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

// r[verify binette.bundle.dump]
// r[verify binette.compat.defaultability-metadata]
#[test]
fn schema_dump_round_trips_metadata_without_affecting_type_ids() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        name: String,
        nickname: Option<String>,
    }

    let bundle = schema_bundle_for::<Account>().unwrap();
    let root = concrete_id(&bundle.root);
    let root_schema = bundle
        .schemas
        .iter()
        .find(|schema| schema.id == root)
        .unwrap();
    let root_type_id = schema_type_id(root_schema).unwrap();

    let dump = SchemaDump {
        bundle,
        metadata: ProducerMetadata {
            declarations: vec![DeclarationMetadata {
                type_id: root,
                source_name: Some("Account".to_owned()),
                documentation: Some("account record".to_owned()),
                source_location: Some("tests/dumps.rs".to_owned()),
                fields: vec![
                    FieldMetadata {
                        name: "id".to_owned(),
                        defaultability: Defaultability::NoDefault,
                        documentation: None,
                        source_location: None,
                    },
                    FieldMetadata {
                        name: "name".to_owned(),
                        defaultability: Defaultability::OpaqueDefault,
                        documentation: Some("display name".to_owned()),
                        source_location: None,
                    },
                    FieldMetadata {
                        name: "nickname".to_owned(),
                        defaultability: Defaultability::LiteralDefault(Value::Option(None)),
                        documentation: None,
                        source_location: Some("Account::nickname".to_owned()),
                    },
                ],
            }],
        },
    };

    let bytes = encode_schema_dump_to_vec(&dump).unwrap();
    let decoded = decode_schema_dump_from_slice(&bytes).unwrap();
    assert_eq!(decoded, dump);

    let value = schema_dump_to_value(&decoded).unwrap();
    assert_eq!(schema_dump_from_value(&value).unwrap(), decoded);

    let decoded_root = decoded
        .bundle
        .schemas
        .iter()
        .find(|schema| schema.id == root)
        .unwrap();
    assert_eq!(schema_type_id(decoded_root).unwrap(), root_type_id);
}

// r[verify binette.bundle.snapshot]
#[test]
fn schema_snapshot_round_trips_bundle_roots() {
    #[derive(Facet)]
    struct Account {
        id: u64,
    }

    #[derive(Facet)]
    struct Message {
        body: String,
    }

    let account = SchemaDump {
        bundle: schema_bundle_for::<Account>().unwrap(),
        metadata: ProducerMetadata::default(),
    };
    let message = SchemaDump {
        bundle: schema_bundle_for::<Message>().unwrap(),
        metadata: ProducerMetadata::default(),
    };
    let roots = [
        concrete_id(&account.bundle.root),
        concrete_id(&message.bundle.root),
    ];
    let snapshot = SchemaSnapshot {
        dumps: vec![account, message],
    };

    let bytes = encode_schema_snapshot_to_vec(&snapshot).unwrap();
    let decoded = decode_schema_snapshot_from_slice(&bytes).unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(
        decoded
            .dumps
            .iter()
            .map(|dump| concrete_id(&dump.bundle.root))
            .collect::<Vec<_>>(),
        roots
    );

    let value = schema_snapshot_to_value(&decoded).unwrap();
    assert_eq!(schema_snapshot_from_value(&value).unwrap(), decoded);
}

// r[verify binette.schema.extension]
#[test]
fn extension_is_not_a_schema_kind() {
    let schema_value = Value::Struct(vec![
        FieldValue {
            name: "id".to_owned(),
            value: Value::U64(0),
        },
        FieldValue {
            name: "type_params".to_owned(),
            value: Value::List(Vec::new()),
        },
        FieldValue {
            name: "kind".to_owned(),
            value: Value::Enum(EnumValue {
                variant: "extension".to_owned(),
                payload: Box::new(Value::Unit),
            }),
        },
    ]);

    let err = binette::schema_from_value(&schema_value).unwrap_err();
    assert!(matches!(
        err,
        SchemaFormatError::UnknownVariant {
            context: "schema kind",
            variant,
        } if variant == "extension"
    ));
}
