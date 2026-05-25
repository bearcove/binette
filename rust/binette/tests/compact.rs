use binette::{
    CompactError, CompactReader, Primitive, SchemaBundle, SchemaRegistry, TypeRef,
    primitive_type_id, schema_bundle_for,
};
use facet::Facet;

fn registry_for(bundle: &SchemaBundle) -> SchemaRegistry {
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(bundle).unwrap();
    registry
}

fn primitive_ref(primitive: Primitive) -> TypeRef {
    TypeRef::concrete(primitive_type_id(primitive))
}

fn bytes_with_u32_len(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

// r[verify binette.scalar.bool]
#[test]
fn compact_skip_validates_bool_bytes() {
    let mut reader = CompactReader::new(&[0x01]);
    reader
        .skip_value(&primitive_ref(Primitive::Bool), &SchemaRegistry::new())
        .unwrap();
    assert!(reader.is_empty());

    let mut reader = CompactReader::new(&[0x02]);
    let err = reader
        .skip_value(&primitive_ref(Primitive::Bool), &SchemaRegistry::new())
        .unwrap_err();
    assert!(matches!(
        err,
        CompactError::InvalidBool {
            position: 0,
            value: 0x02
        }
    ));
}

// r[verify binette.scalar.string]
// r[verify binette.length.u32]
#[test]
fn compact_skip_uses_u32_lengths_for_strings() {
    let bytes = bytes_with_u32_len(b"amos");
    let mut reader = CompactReader::new(&bytes);
    reader
        .skip_value(&primitive_ref(Primitive::String), &SchemaRegistry::new())
        .unwrap();
    assert_eq!(reader.position(), bytes.len());

    let bytes = bytes_with_u32_len(&[0xFF]);
    let mut reader = CompactReader::new(&bytes);
    let err = reader
        .skip_value(&primitive_ref(Primitive::String), &SchemaRegistry::new())
        .unwrap_err();
    assert!(matches!(
        err,
        CompactError::InvalidString { position: 4, .. }
    ));
}

// r[verify binette.aggregate.struct.compact]
// r[verify binette.aggregate.schema-driven-skip]
#[test]
fn compact_skip_walks_struct_fields_in_writer_order() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        name: String,
        active: bool,
    }

    let bundle = schema_bundle_for::<Account>().unwrap();
    let registry = registry_for(&bundle);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&42u64.to_le_bytes());
    bytes.extend_from_slice(&bytes_with_u32_len(b"binette"));
    bytes.push(0x01);

    let mut reader = CompactReader::new(&bytes);
    reader.skip_value(&bundle.root, &registry).unwrap();
    assert!(reader.is_empty());
}

// r[verify binette.aggregate.schema-driven-skip]
#[test]
fn compact_skip_walks_nested_aggregates() {
    #[derive(Facet)]
    struct Nested {
        numbers: Vec<u32>,
        label: Option<String>,
        fixed: [u16; 2],
    }

    let bundle = schema_bundle_for::<Nested>().unwrap();
    let registry = registry_for(&bundle);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&10u32.to_le_bytes());
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.push(0x01);
    bytes.extend_from_slice(&bytes_with_u32_len(b"yes"));
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());

    let mut reader = CompactReader::new(&bytes);
    reader.skip_value(&bundle.root, &registry).unwrap();
    assert!(reader.is_empty());
}

// r[verify binette.aggregate.schema-driven-skip]
#[test]
fn compact_skip_uses_enum_variant_index_to_skip_payload() {
    #[derive(Facet)]
    #[repr(u8)]
    #[allow(dead_code)]
    enum Event {
        Started,
        Moved(u32, u32),
        Failed { code: u16 },
    }

    let bundle = schema_bundle_for::<Event>().unwrap();
    let registry = registry_for(&bundle);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&200u32.to_le_bytes());

    let mut reader = CompactReader::new(&bytes);
    reader.skip_value(&bundle.root, &registry).unwrap();
    assert!(reader.is_empty());
}

// r[verify binette.schema.type-ref]
// r[verify binette.aggregate.schema-driven-skip]
#[test]
fn compact_skip_resolves_generic_type_arguments() {
    #[derive(Facet)]
    struct Wrapper<T> {
        value: T,
    }

    let bundle = schema_bundle_for::<Wrapper<String>>().unwrap();
    let registry = registry_for(&bundle);
    let bytes = bytes_with_u32_len(b"generic");

    let mut reader = CompactReader::new(&bytes);
    reader.skip_value(&bundle.root, &registry).unwrap();
    assert!(reader.is_empty());
}

// r[verify binette.aggregate.schema-driven-skip]
#[test]
fn compact_skip_rejects_invalid_option_tags() {
    #[derive(Facet)]
    struct Maybe {
        value: Option<u32>,
    }

    let bundle = schema_bundle_for::<Maybe>().unwrap();
    let registry = registry_for(&bundle);
    let mut reader = CompactReader::new(&[0x02]);
    let err = reader.skip_value(&bundle.root, &registry).unwrap_err();
    assert!(matches!(
        err,
        CompactError::InvalidOptionTag {
            position: 0,
            value: 0x02
        }
    ));
}
