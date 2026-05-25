use binette::{
    DecodeError, Primitive, SchemaBundle, SchemaRegistry, TypeRef, decode_from_slice,
    encode_to_vec, encode_to_vec_with_plan, primitive_type_id, writer_plan_for,
};
use facet::Facet;

fn registry_for(bundle: &SchemaBundle) -> SchemaRegistry {
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(bundle).unwrap();
    registry
}

// r[verify binette.compat.plan]
// r[verify binette.mode.compact]
// r[verify binette.aggregate.struct.compact]
#[test]
fn decodes_same_struct_into_facet_partial() {
    #[derive(Debug, Facet, PartialEq)]
    struct Account {
        id: u64,
        name: String,
        active: bool,
    }

    let writer_plan = writer_plan_for::<Account>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());

    let bytes = encode_to_vec_with_plan(
        &Account {
            id: 42,
            name: "binette".to_owned(),
            active: true,
        },
        &writer_plan,
    )
    .unwrap();

    let decoded =
        decode_from_slice::<Account>(&bytes, writer_plan.root(), &writer_registry).unwrap();
    assert_eq!(
        decoded,
        Account {
            id: 42,
            name: "binette".to_owned(),
            active: true,
        }
    );

    let bytes_from_convenience = encode_to_vec(&Account {
        id: 42,
        name: "binette".to_owned(),
        active: true,
    })
    .unwrap();
    assert_eq!(
        bytes, bytes_from_convenience,
        "the convenience wrapper must use the same schema-derived writer plan"
    );
}

// r[verify binette.compat.field-matching]
// r[verify binette.compat.skip-unknown]
// r[verify binette.aggregate.schema-driven-skip]
#[test]
fn decodes_reader_fields_by_name_and_skips_writer_only_fields() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
            pub name: String,
            pub nickname: String,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        pub struct Account {
            pub name: String,
            pub id: u64,
        }
    }

    let writer_plan = writer_plan_for::<writer::Account>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());

    let bytes = encode_to_vec_with_plan(
        &writer::Account {
            id: 7,
            name: "Amos".to_owned(),
            nickname: "not for this reader".to_owned(),
        },
        &writer_plan,
    )
    .unwrap();

    let decoded =
        decode_from_slice::<reader::Account>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(
        decoded,
        reader::Account {
            name: "Amos".to_owned(),
            id: 7,
        }
    );
}

// r[verify binette.aggregate.list]
// r[verify binette.aggregate.option]
// r[verify binette.aggregate.array]
#[test]
fn encode_then_decode_nested_compact_shapes() {
    #[derive(Debug, Facet, PartialEq)]
    struct Nested {
        numbers: Vec<u32>,
        label: Option<String>,
        fixed: [u16; 2],
    }

    let writer_plan = writer_plan_for::<Nested>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let expected = Nested {
        numbers: vec![10, 20],
        label: Some("yes".to_owned()),
        fixed: [3, 4],
    };

    let bytes = encode_to_vec_with_plan(&expected, &writer_plan).unwrap();
    let decoded =
        decode_from_slice::<Nested>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(decoded, expected);
}

// r[verify binette.schema.type-ref]
// r[verify binette.mode.compact]
#[test]
fn encode_then_decode_nested_generic_shapes() {
    #[derive(Debug, Facet, PartialEq)]
    struct Wrapper<T> {
        value: T,
    }

    #[derive(Debug, Facet, PartialEq)]
    struct Holder<T> {
        wrapped: Wrapper<T>,
    }

    let writer_plan = writer_plan_for::<Holder<String>>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let expected = Holder {
        wrapped: Wrapper {
            value: "generic".to_owned(),
        },
    };

    let bytes = encode_to_vec_with_plan(&expected, &writer_plan).unwrap();
    let decoded =
        decode_from_slice::<Holder<String>>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(decoded, expected);
}

// r[verify binette.aggregate.enum.compact]
#[test]
fn encode_then_decode_enum_payloads() {
    #[derive(Debug, Facet, PartialEq)]
    #[repr(u8)]
    enum Event {
        Started,
        Moved(u32, u32),
        Failed { code: u16 },
    }

    let writer_plan = writer_plan_for::<Event>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let expected = Event::Moved(100, 200);

    let bytes = encode_to_vec_with_plan(&expected, &writer_plan).unwrap();
    let decoded = decode_from_slice::<Event>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(decoded, expected);
}

// r[verify binette.compat.enum]
// r[verify binette.compat.enum.payload]
#[test]
fn decodes_enum_variants_by_name_and_payload_plan() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Started,
            Failed { code: u16, message: String },
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Failed { message: String, code: u16 },
            Started,
        }
    }

    let writer_plan = writer_plan_for::<writer::Event>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(
        &writer::Event::Failed {
            code: 404,
            message: "gone".to_owned(),
        },
        &writer_plan,
    )
    .unwrap();

    let decoded =
        decode_from_slice::<reader::Event>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(
        decoded,
        reader::Event::Failed {
            message: "gone".to_owned(),
            code: 404,
        }
    );
}

// r[verify binette.compat.enum.unknown-variant]
#[test]
fn decode_rejects_writer_only_enum_variant_at_runtime() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Started,
            Failed { code: u16 },
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Started,
        }
    }

    let writer_plan = writer_plan_for::<writer::Event>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes =
        encode_to_vec_with_plan(&writer::Event::Failed { code: 500 }, &writer_plan).unwrap();

    let err = decode_from_slice::<reader::Event>(&bytes, writer_plan.root(), &writer_registry)
        .unwrap_err();

    assert!(matches!(
        err,
        DecodeError::UnreadableWriterVariant {
            variant_index: 1,
            variant,
            ..
        } if variant == "Failed"
    ));
}

#[test]
fn decode_rejects_trailing_bytes() {
    #[derive(Debug, Facet)]
    struct One {
        id: u8,
    }

    let writer_plan = writer_plan_for::<One>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let err = decode_from_slice::<One>(&[1, 2], writer_plan.root(), &writer_registry).unwrap_err();

    assert!(matches!(
        err,
        DecodeError::TrailingBytes {
            position: 1,
            remaining: 1
        }
    ));
}

// r[verify binette.type-id.context-free]
// r[verify binette.mode.compact]
#[test]
fn writer_plan_uses_schema_root_for_transparent_wrappers() {
    #[derive(Facet)]
    #[repr(transparent)]
    struct UserId(String);

    let plan = writer_plan_for::<UserId>().unwrap();
    assert_eq!(
        plan.root(),
        &TypeRef::concrete(primitive_type_id(Primitive::String))
    );
    assert!(plan.schema_bundle().schemas.is_empty());

    let bytes = encode_to_vec_with_plan(&UserId("amos".to_owned()), &plan).unwrap();
    assert_eq!(bytes, [4, 0, 0, 0, b'a', b'm', b'o', b's']);
}
