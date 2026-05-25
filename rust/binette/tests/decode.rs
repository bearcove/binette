use binette::{
    DecodeError, SchemaBundle, SchemaRegistry, decode_from_slice, encode_to_vec, schema_bundle_for,
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

    let writer_bundle = schema_bundle_for::<Account>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let bytes = encode_to_vec(&Account {
        id: 42,
        name: "binette".to_owned(),
        active: true,
    })
    .unwrap();

    let decoded =
        decode_from_slice::<Account>(&bytes, &writer_bundle.root, &writer_registry).unwrap();
    assert_eq!(
        decoded,
        Account {
            id: 42,
            name: "binette".to_owned(),
            active: true,
        }
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

    let writer_bundle = schema_bundle_for::<writer::Account>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let bytes = encode_to_vec(&writer::Account {
        id: 7,
        name: "Amos".to_owned(),
        nickname: "not for this reader".to_owned(),
    })
    .unwrap();

    let decoded =
        decode_from_slice::<reader::Account>(&bytes, &writer_bundle.root, &writer_registry)
            .unwrap();

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

    let writer_bundle = schema_bundle_for::<Nested>().unwrap();
    let writer_registry = registry_for(&writer_bundle);
    let expected = Nested {
        numbers: vec![10, 20],
        label: Some("yes".to_owned()),
        fixed: [3, 4],
    };

    let bytes = encode_to_vec(&expected).unwrap();
    let decoded =
        decode_from_slice::<Nested>(&bytes, &writer_bundle.root, &writer_registry).unwrap();

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

    let writer_bundle = schema_bundle_for::<Event>().unwrap();
    let writer_registry = registry_for(&writer_bundle);
    let expected = Event::Moved(100, 200);

    let bytes = encode_to_vec(&expected).unwrap();
    let decoded =
        decode_from_slice::<Event>(&bytes, &writer_bundle.root, &writer_registry).unwrap();

    assert_eq!(decoded, expected);
}

#[test]
fn decode_rejects_trailing_bytes() {
    #[derive(Debug, Facet)]
    struct One {
        id: u8,
    }

    let writer_bundle = schema_bundle_for::<One>().unwrap();
    let writer_registry = registry_for(&writer_bundle);
    let err = decode_from_slice::<One>(&[1, 2], &writer_bundle.root, &writer_registry).unwrap_err();

    assert!(matches!(
        err,
        DecodeError::TrailingBytes {
            position: 1,
            remaining: 1
        }
    ));
}
