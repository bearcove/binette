use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use binette::{
    CompactError, DecodeError, EncodeError, Primitive, SchemaBundle, SchemaRegistry, StencilError,
    TypeRef, decode_from_slice, encode_to_vec, encode_to_vec_with_plan, primitive_type_id,
    stencil_decoder_for, writer_plan_for,
};
use facet::Facet;
use facet_value::{VArray, VObject, Value as FacetValue};

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

// r[verify binette.mode.compact]
// r[verify binette.compat.field-matching]
// r[verify binette.compat.skip-unknown]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_decodes_fixed_scalar_struct_with_reorder_and_skip() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Message {
            pub id: u64,
            pub code: u16,
            pub writer_only: u32,
            pub seq: u8,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        pub struct Message {
            pub seq: u8,
            pub id: u64,
            pub code: u16,
        }
    }

    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(
        &writer::Message {
            id: 0x0102_0304_0506_0708,
            code: 0x1122,
            writer_only: 0xaabb_ccdd,
            seq: 7,
        },
        &writer_plan,
    )
    .unwrap();

    let stencil =
        stencil_decoder_for::<reader::Message>(writer_plan.root(), &writer_registry).unwrap();
    assert_eq!(stencil.expected_len(), bytes.len());

    let decoded = stencil.decode(&bytes).unwrap();
    let interpreted =
        decode_from_slice::<reader::Message>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(decoded, interpreted);
    assert_eq!(
        decoded,
        reader::Message {
            seq: 7,
            id: 0x0102_0304_0506_0708,
            code: 0x1122,
        }
    );

    assert!(matches!(
        stencil.decode(&bytes[..bytes.len() - 1]),
        Err(StencilError::InputLength { .. })
    ));
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

// r[verify binette.aggregate.dynamic-value]
#[test]
fn encode_then_decode_facet_value_as_compact_dynamic_value() {
    #[derive(Debug, Facet, PartialEq)]
    struct Holder {
        value: FacetValue,
    }

    let mut object = VObject::new();
    object.insert("name", FacetValue::from("binette"));
    object.insert("count", FacetValue::from(3u64));

    let mut items = VArray::new();
    items.push(FacetValue::from(true));
    items.push(FacetValue::NULL);
    object.insert("items", FacetValue::from(items));

    let expected = Holder {
        value: FacetValue::from(object),
    };
    let writer_plan = writer_plan_for::<Holder>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());

    let bytes = encode_to_vec_with_plan(&expected, &writer_plan).unwrap();
    assert_eq!(bytes[0], 0x17);

    let decoded =
        decode_from_slice::<Holder>(&bytes, writer_plan.root(), &writer_registry).unwrap();
    assert_eq!(decoded, expected);
}

// r[verify binette.aggregate.tuple]
#[test]
fn encode_then_decode_tuple_values() {
    let expected = (7u16, "seven".to_owned());
    let writer_plan = writer_plan_for::<(u16, String)>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());

    let bytes = encode_to_vec_with_plan(&expected, &writer_plan).unwrap();
    assert_eq!(bytes, [7, 0, 5, 0, 0, 0, b's', b'e', b'v', b'e', b'n']);

    let decoded =
        decode_from_slice::<(u16, String)>(&bytes, writer_plan.root(), &writer_registry).unwrap();
    assert_eq!(decoded, expected);
}

// r[verify binette.aggregate.set]
// r[verify binette.aggregate.map]
// r[verify binette.aggregate.set-map.canonical]
#[test]
fn encode_then_decode_sets_and_maps_in_canonical_byte_order() {
    let set = HashSet::from([1u16, 256u16]);
    let set_plan = writer_plan_for::<HashSet<u16>>().unwrap();
    let set_registry = registry_for(set_plan.schema_bundle());
    let set_bytes = encode_to_vec_with_plan(&set, &set_plan).unwrap();

    assert_eq!(set_bytes, [2, 0, 0, 0, 0, 1, 1, 0]);
    assert_eq!(
        decode_from_slice::<HashSet<u16>>(&set_bytes, set_plan.root(), &set_registry).unwrap(),
        set
    );

    let map = HashMap::from([(1u16, 10u8), (256u16, 20u8)]);
    let map_plan = writer_plan_for::<HashMap<u16, u8>>().unwrap();
    let map_registry = registry_for(map_plan.schema_bundle());
    let map_bytes = encode_to_vec_with_plan(&map, &map_plan).unwrap();

    assert_eq!(map_bytes, [2, 0, 0, 0, 0, 1, 20, 1, 0, 10]);
    assert_eq!(
        decode_from_slice::<HashMap<u16, u8>>(&map_bytes, map_plan.root(), &map_registry).unwrap(),
        map
    );
}

// r[verify binette.aggregate.set]
// r[verify binette.aggregate.set-map.decode-policy]
#[test]
fn decode_rejects_noncanonical_set_order() {
    let writer_plan = writer_plan_for::<HashSet<u16>>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = [2, 0, 0, 0, 1, 0, 0, 1];

    let err = decode_from_slice::<HashSet<u16>>(&bytes, writer_plan.root(), &writer_registry)
        .unwrap_err();

    assert!(matches!(
        err,
        DecodeError::Compact(CompactError::NonCanonicalOrder {
            position: 6,
            aggregate: "set"
        })
    ));
}

// r[verify binette.aggregate.map]
// r[verify binette.aggregate.set-map.decode-policy]
#[test]
fn decode_rejects_noncanonical_map_order() {
    let writer_plan = writer_plan_for::<HashMap<u16, u8>>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = [2, 0, 0, 0, 1, 0, 10, 0, 1, 20];

    let err = decode_from_slice::<HashMap<u16, u8>>(&bytes, writer_plan.root(), &writer_registry)
        .unwrap_err();

    assert!(matches!(
        err,
        DecodeError::Compact(CompactError::NonCanonicalOrder {
            position: 7,
            aggregate: "map"
        })
    ));
}

// r[verify binette.aggregate.set-map.float-keys]
#[test]
fn encode_and_decode_reject_nan_inside_set_or_map_keys() {
    #[derive(Debug, Copy, Clone, Facet)]
    struct FloatKey(f32);

    impl PartialEq for FloatKey {
        fn eq(&self, other: &Self) -> bool {
            self.0.to_bits() == other.0.to_bits()
        }
    }

    impl Eq for FloatKey {}

    impl Hash for FloatKey {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.0.to_bits().hash(state);
        }
    }

    let set = HashSet::from([FloatKey(f32::NAN)]);
    let err = encode_to_vec(&set).unwrap_err();
    assert!(matches!(err, EncodeError::NanCanonicalKey { .. }));

    let writer_plan = writer_plan_for::<HashSet<FloatKey>>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = [1, 0, 0, 0, 0, 0, 0xc0, 0x7f];
    let err = decode_from_slice::<HashSet<FloatKey>>(&bytes, writer_plan.root(), &writer_registry)
        .unwrap_err();

    assert!(matches!(
        err,
        DecodeError::Compact(CompactError::NanCanonicalKey {
            position: 4,
            aggregate: "set"
        })
    ));
}

// r[verify binette.aggregate.set]
// r[verify binette.compat.plan]
#[test]
fn decodes_set_elements_with_translation_plan() {
    mod writer {
        use std::hash::{Hash, Hasher};

        use facet::Facet;

        #[derive(Debug, Facet)]
        #[allow(dead_code)]
        pub struct Item {
            pub id: u16,
            pub label: String,
            pub writer_only: u8,
        }

        impl PartialEq for Item {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id && self.label == other.label
            }
        }

        impl Eq for Item {}

        impl Hash for Item {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.id.hash(state);
                self.label.hash(state);
            }
        }
    }

    mod reader {
        use std::hash::{Hash, Hasher};

        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        pub struct Item {
            pub label: String,
            pub id: u16,
        }

        impl Eq for Item {}

        impl Hash for Item {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.label.hash(state);
                self.id.hash(state);
            }
        }
    }

    let writer_plan = writer_plan_for::<HashSet<writer::Item>>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let value = HashSet::from([writer::Item {
        id: 42,
        label: "answer".to_owned(),
        writer_only: 9,
    }]);

    let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();
    let decoded =
        decode_from_slice::<HashSet<reader::Item>>(&bytes, writer_plan.root(), &writer_registry)
            .unwrap();

    assert_eq!(
        decoded,
        HashSet::from([reader::Item {
            label: "answer".to_owned(),
            id: 42,
        }])
    );
}

// r[verify binette.aggregate.map]
// r[verify binette.compat.plan]
#[test]
fn decodes_map_values_with_translation_plan() {
    mod writer {
        use facet::Facet;

        #[derive(Debug, Facet)]
        #[allow(dead_code)]
        pub struct Item {
            pub id: u16,
            pub label: String,
            pub writer_only: u8,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        pub struct Item {
            pub label: String,
            pub id: u16,
        }
    }

    let writer_plan = writer_plan_for::<HashMap<u16, writer::Item>>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let value = HashMap::from([(
        7u16,
        writer::Item {
            id: 42,
            label: "answer".to_owned(),
            writer_only: 9,
        },
    )]);

    let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();
    let decoded = decode_from_slice::<HashMap<u16, reader::Item>>(
        &bytes,
        writer_plan.root(),
        &writer_registry,
    )
    .unwrap();

    assert_eq!(
        decoded,
        HashMap::from([(
            7u16,
            reader::Item {
                label: "answer".to_owned(),
                id: 42,
            },
        )])
    );
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
