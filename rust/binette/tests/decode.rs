use binette::{DecodeError, SchemaBundle, SchemaRegistry, decode_from_slice, schema_bundle_for};
use facet::Facet;

fn registry_for(bundle: &SchemaBundle) -> SchemaRegistry {
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(bundle).unwrap();
    registry
}

fn push_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

// r[verify binette.compat.plan]
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

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&42u64.to_le_bytes());
    push_string(&mut bytes, "binette");
    bytes.push(0x01);

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

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&7u64.to_le_bytes());
    push_string(&mut bytes, "Amos");
    push_string(&mut bytes, "not for this reader");

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
