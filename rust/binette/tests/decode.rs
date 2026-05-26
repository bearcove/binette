use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use binette::{
    CompactError, DecodeError, EncodeError, Primitive, SchemaBundle, SchemaRegistry, StencilError,
    TypeRef, decode_from_slice, encode_to_vec, encode_to_vec_with_plan, encode_to_vec_with_stencil,
    primitive_type_id, stencil_decoder_for, stencil_encoder_from_plan, strict_stencil_decoder_for,
    strict_stencil_encoder_from_plan, writer_plan_for,
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
// r[verify binette.scalar.bool]
// r[verify binette.compat.field-matching]
// r[verify binette.compat.skip-unknown]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_decodes_fixed_scalar_struct_with_validation_reorder_and_skip() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Message {
            pub id: u64,
            pub enabled: bool,
            pub code: u16,
            pub writer_only: u32,
            pub writer_only_flag: bool,
            pub seq: u8,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        pub struct Message {
            pub seq: u8,
            pub enabled: bool,
            pub id: u64,
            pub code: u16,
        }
    }

    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(
        &writer::Message {
            id: 0x0102_0304_0506_0708,
            enabled: true,
            code: 0x1122,
            writer_only: 0xaabb_ccdd,
            writer_only_flag: false,
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
            enabled: true,
            id: 0x0102_0304_0506_0708,
            code: 0x1122,
        }
    );

    assert!(matches!(
        stencil.decode(&bytes[..bytes.len() - 1]),
        Err(StencilError::InputLength { .. })
    ));

    let mut invalid_read_bool = bytes.clone();
    invalid_read_bool[8] = 2;
    assert!(matches!(
        stencil.decode(&invalid_read_bool),
        Err(StencilError::InvalidBool {
            position: 8,
            value: 2,
            ..
        })
    ));

    let mut invalid_skipped_bool = bytes;
    invalid_skipped_bool[15] = 2;
    assert!(matches!(
        stencil.decode(&invalid_skipped_bool),
        Err(StencilError::InvalidBool {
            position: 15,
            value: 2,
            ..
        })
    ));
}

// r[verify binette.aggregate.struct.compact]
// r[verify binette.aggregate.tuple]
// r[verify binette.compat.field-matching]
// r[verify binette.compat.skip-unknown]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_decodes_nested_structs_and_tuples() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Header {
            pub trace: u64,
            pub flags: bool,
        }

        #[derive(Facet)]
        pub struct Extra {
            pub code: u16,
            pub enabled: bool,
        }

        #[derive(Facet)]
        pub struct Message {
            pub id: u32,
            pub header: Header,
            pub pair: (u16, bool),
            pub writer_only: Extra,
            pub tail: u8,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        pub struct Header {
            pub flags: bool,
            pub trace: u64,
        }

        #[derive(Debug, Facet, PartialEq)]
        pub struct Message {
            pub tail: u8,
            pub pair: (u16, bool),
            pub header: Header,
            pub id: u32,
        }
    }

    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(
        &writer::Message {
            id: 0x1122_3344,
            header: writer::Header {
                trace: 0x0102_0304_0506_0708,
                flags: true,
            },
            pair: (0x5566, false),
            writer_only: writer::Extra {
                code: 0x7788,
                enabled: true,
            },
            tail: 9,
        },
        &writer_plan,
    )
    .unwrap();

    let stencil =
        stencil_decoder_for::<reader::Message>(writer_plan.root(), &writer_registry).unwrap();
    let decoded = stencil.decode(&bytes).unwrap();
    let interpreted =
        decode_from_slice::<reader::Message>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(decoded, interpreted);
    assert_eq!(
        decoded,
        reader::Message {
            tail: 9,
            pair: (0x5566, false),
            header: reader::Header {
                flags: true,
                trace: 0x0102_0304_0506_0708,
            },
            id: 0x1122_3344,
        }
    );

    let mut invalid_nested_bool = bytes.clone();
    invalid_nested_bool[12] = 2;
    assert!(matches!(
        stencil.decode(&invalid_nested_bool),
        Err(StencilError::InvalidBool {
            position: 12,
            value: 2,
            ..
        })
    ));

    let mut invalid_skipped_nested_bool = bytes;
    invalid_skipped_nested_bool[18] = 2;
    assert!(matches!(
        stencil.decode(&invalid_skipped_nested_bool),
        Err(StencilError::InvalidBool {
            position: 18,
            value: 2,
            ..
        })
    ));
}

// r[verify binette.aggregate.enum.compact]
// r[verify binette.compat.enum]
// r[verify binette.compat.enum.payload]
// r[verify binette.compat.enum.unknown-variant]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_decodes_enum_variants_by_name_and_payload_plan() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Started,
            Moved(u32, u16),
            Failed { code: u16, flag: bool },
            WriterOnly,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Failed { flag: bool, code: u16 },
            Started,
            Moved(u32, u16),
        }
    }

    let writer_plan = writer_plan_for::<writer::Event>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let failed_bytes = encode_to_vec_with_plan(
        &writer::Event::Failed {
            code: 0x1122,
            flag: true,
        },
        &writer_plan,
    )
    .unwrap();

    let stencil =
        stencil_decoder_for::<reader::Event>(writer_plan.root(), &writer_registry).unwrap();
    assert_eq!(stencil.fixed_expected_len(), None);

    let decoded = stencil.decode(&failed_bytes).unwrap();
    let interpreted =
        decode_from_slice::<reader::Event>(&failed_bytes, writer_plan.root(), &writer_registry)
            .unwrap();

    assert_eq!(decoded, interpreted);
    assert_eq!(
        decoded,
        reader::Event::Failed {
            flag: true,
            code: 0x1122,
        }
    );

    let moved_bytes = encode_to_vec_with_plan(&writer::Event::Moved(10, 20), &writer_plan).unwrap();
    assert_eq!(
        stencil.decode(&moved_bytes).unwrap(),
        reader::Event::Moved(10, 20)
    );

    let mut unknown_variant = failed_bytes.clone();
    unknown_variant[0..4].copy_from_slice(&99u32.to_le_bytes());
    assert!(matches!(
        stencil.decode(&unknown_variant),
        Err(StencilError::UnknownVariantIndex {
            position: 0,
            variant_index: 99,
        })
    ));

    let writer_only_bytes =
        encode_to_vec_with_plan(&writer::Event::WriterOnly, &writer_plan).unwrap();
    assert!(matches!(
        stencil.decode(&writer_only_bytes),
        Err(StencilError::UnreadableWriterVariant {
            position: 0,
            variant_index: 3,
            ..
        })
    ));
}

// r[verify binette.aggregate.enum.compact]
// r[verify binette.compat.plan]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_decodes_same_schema_enum_through_translation_plan() {
    #[derive(Debug, Facet, PartialEq)]
    #[allow(dead_code)]
    #[repr(u8)]
    enum Event {
        Started,
        Moved(u32, u16),
        Failed { code: u16, flag: bool },
    }

    let writer_plan = writer_plan_for::<Event>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&Event::Moved(10, 20), &writer_plan).unwrap();

    let stencil = stencil_decoder_for::<Event>(writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(stencil.decode(&bytes).unwrap(), Event::Moved(10, 20));
}

// r[verify binette.aggregate.struct.compact]
// r[verify binette.aggregate.list]
// r[verify binette.aggregate.option]
// r[verify binette.compat.field-matching]
// r[verify binette.compat.skip-unknown]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_decodes_mixed_struct_through_schema_plan() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Nested {
            pub count: u32,
            pub label: String,
            pub enabled: bool,
        }

        #[derive(Facet)]
        pub struct Message {
            pub id: u64,
            pub title: String,
            pub active: bool,
            pub counts: Vec<u32>,
            pub maybe: Option<String>,
            pub nested: Nested,
            pub pair: (u16, String),
            pub writer_only: String,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Debug, Facet, PartialEq)]
        pub struct Nested {
            pub label: String,
            pub enabled: bool,
            pub count: u32,
        }

        #[derive(Debug, Facet, PartialEq)]
        pub struct Message {
            pub pair: (u16, String),
            pub nested: Nested,
            pub maybe: Option<String>,
            pub counts: Vec<u32>,
            pub active: bool,
            pub title: String,
            pub id: u64,
        }
    }

    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(
        &writer::Message {
            id: 0x0102_0304_0506_0708,
            title: "binette baseline".to_owned(),
            active: true,
            counts: vec![1, 2, 3, 5, 8, 13, 21, 34],
            maybe: Some("present".to_owned()),
            nested: writer::Nested {
                count: 42,
                label: "nested".to_owned(),
                enabled: true,
            },
            pair: (7, "seven".to_owned()),
            writer_only: "skipped by reader".to_owned(),
        },
        &writer_plan,
    )
    .unwrap();

    let stencil =
        stencil_decoder_for::<reader::Message>(writer_plan.root(), &writer_registry).unwrap();
    assert_eq!(stencil.fixed_expected_len(), None);

    let decoded = stencil.decode(&bytes).unwrap();
    let interpreted =
        decode_from_slice::<reader::Message>(&bytes, writer_plan.root(), &writer_registry).unwrap();

    assert_eq!(decoded, interpreted);
    assert_eq!(
        decoded,
        reader::Message {
            pair: (7, "seven".to_owned()),
            nested: reader::Nested {
                label: "nested".to_owned(),
                enabled: true,
                count: 42,
            },
            maybe: Some("present".to_owned()),
            counts: vec![1, 2, 3, 5, 8, 13, 21, 34],
            active: true,
            title: "binette baseline".to_owned(),
            id: 0x0102_0304_0506_0708,
        }
    );
}

// r[verify binette.mode.compact]
// r[verify binette.aggregate.struct.compact]
// r[verify binette.aggregate.list]
// r[verify binette.aggregate.option]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_encodes_mixed_struct_through_writer_plan() {
    #[derive(Facet)]
    struct Nested {
        count: u32,
        label: String,
        enabled: bool,
    }

    #[derive(Facet)]
    struct Message {
        id: u64,
        title: String,
        active: bool,
        counts: Vec<u32>,
        maybe: Option<String>,
        nested: Nested,
        pair: (u16, String),
    }

    let value = Message {
        id: 0x0102_0304_0506_0708,
        title: "binette baseline".to_owned(),
        active: true,
        counts: vec![1, 2, 3, 5, 8, 13, 21, 34],
        maybe: Some("present".to_owned()),
        nested: Nested {
            count: 42,
            label: "nested".to_owned(),
            enabled: true,
        },
        pair: (7, "seven".to_owned()),
    };

    let writer_plan = writer_plan_for::<Message>().unwrap();
    let stencil = stencil_encoder_from_plan::<Message>(&writer_plan).unwrap();

    let stencil_bytes = encode_to_vec_with_stencil(&value, &stencil).unwrap();
    let interpreted_bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

    assert_eq!(stencil_bytes, interpreted_bytes);
}

// r[verify binette.mode.compact]
// r[verify binette.aggregate.struct.compact]
// r[verify binette.scalar.bool]
// r[verify binette.scalar.char]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_encodes_fixed_struct_through_direct_writer_plan() {
    #[derive(Facet)]
    struct Message {
        id: u64,
        active: bool,
        code: u16,
        marker: char,
    }

    let value = Message {
        id: 0x0102_0304_0506_0708,
        active: true,
        code: 0x1122,
        marker: 'b',
    };

    let writer_plan = writer_plan_for::<Message>().unwrap();
    let stencil = stencil_encoder_from_plan::<Message>(&writer_plan).unwrap();

    let stencil_bytes = encode_to_vec_with_stencil(&value, &stencil).unwrap();
    let interpreted_bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

    assert_eq!(stencil_bytes, interpreted_bytes);
}

// r[verify binette.aggregate.enum.compact]
// r[verify binette.compat.enum.payload]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_encodes_enum_through_writer_plan() {
    #[derive(Facet)]
    #[allow(dead_code)]
    #[repr(u8)]
    enum Event {
        Started,
        Moved(u32, u16),
        Failed { code: u16, flag: bool },
    }

    let value = Event::Failed {
        code: 0x1122,
        flag: true,
    };
    let writer_plan = writer_plan_for::<Event>().unwrap();
    let stencil = stencil_encoder_from_plan::<Event>(&writer_plan).unwrap();

    let stencil_bytes = encode_to_vec_with_stencil(&value, &stencil).unwrap();
    let interpreted_bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

    assert_eq!(stencil_bytes, interpreted_bytes);
}

// r[verify binette.aggregate.tuple]
// r[verify binette.aggregate.list]
// r[verify binette.aggregate.set]
// r[verify binette.aggregate.map]
// r[verify binette.aggregate.option]
// r[verify binette.aggregate.array]
// r[verify binette.aggregate.dynamic-value]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn stencil_handles_aggregate_roots_through_schema_plan() {
    fn assert_stencil_matches_interpreter<T>(value: T)
    where
        T: Facet<'static> + std::fmt::Debug + PartialEq,
    {
        let writer_plan = writer_plan_for::<T>().unwrap();
        let writer_registry = registry_for(writer_plan.schema_bundle());
        let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

        let decoder = stencil_decoder_for::<T>(writer_plan.root(), &writer_registry).unwrap();
        let decoded = decoder.decode(&bytes).unwrap();
        assert_eq!(decoded, value);

        let encoder = stencil_encoder_from_plan::<T>(&writer_plan).unwrap();
        let stencil_bytes = encode_to_vec_with_stencil(&value, &encoder).unwrap();
        assert_eq!(stencil_bytes, bytes);
    }

    assert_stencil_matches_interpreter((7u16, "seven".to_owned(), vec![1u32, 2, 3], Some(true)));
    assert_stencil_matches_interpreter(vec![(1u16, "one".to_owned()), (2, "two".to_owned())]);
    assert_stencil_matches_interpreter(HashSet::from([3u16, 1, 2]));
    assert_stencil_matches_interpreter(HashMap::from([(2u16, 20u8), (1, 10), (3, 30)]));
    assert_stencil_matches_interpreter(Some((9u16, "nine".to_owned())));
    assert_stencil_matches_interpreter([5u16, 8, 13, 21]);

    let mut object = VObject::new();
    object.insert("name", FacetValue::from("binette"));
    object.insert("count", FacetValue::from(3u64));
    let mut items = VArray::new();
    items.push(FacetValue::from(true));
    items.push(FacetValue::NULL);
    object.insert("items", FacetValue::from(items));
    assert_stencil_matches_interpreter(FacetValue::from(object));
}

// r[verify binette.mode.compact]
// r[verify binette.aggregate.array]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn strict_stencil_handles_fixed_array_roots() {
    let value = [5u16, 8, 13, 21];
    let writer_plan = writer_plan_for::<[u16; 4]>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

    let decoder =
        strict_stencil_decoder_for::<[u16; 4]>(writer_plan.root(), &writer_registry).unwrap();
    assert_eq!(decoder.fixed_expected_len(), Some(bytes.len()));
    assert_eq!(decoder.decode(&bytes).unwrap(), value);

    let encoder = strict_stencil_encoder_from_plan::<[u16; 4]>(&writer_plan).unwrap();
    assert_eq!(encode_to_vec_with_stencil(&value, &encoder).unwrap(), bytes);
}

// r[verify binette.mode.compact]
// r[verify binette.aggregate.list]
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[test]
fn strict_stencil_decodes_fixed_element_list_roots() {
    let value = vec![(1u16, 10u32), (2, 20), (3, 30), (5, 50)];
    let writer_plan = writer_plan_for::<Vec<(u16, u32)>>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

    let decoder =
        strict_stencil_decoder_for::<Vec<(u16, u32)>>(writer_plan.root(), &writer_registry)
            .unwrap();
    assert_eq!(decoder.fixed_expected_len(), None);
    assert_eq!(decoder.decode(&bytes).unwrap(), value);

    assert!(matches!(
        decoder.decode(&bytes[..bytes.len() - 1]),
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
