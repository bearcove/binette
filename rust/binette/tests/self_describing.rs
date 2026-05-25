use binette::{
    ArrayValue, CompactError, EnumValue, ExtensionValue, FieldValue, SelfDescribingError, Value,
    decode_dynamic_value_from_slice, decode_self_described_from_slice, encode_dynamic_value_to_vec,
    encode_self_described_to_vec,
};

// r[verify binette.forms]
// r[verify binette.mode.self-describing]
// r[verify binette.tags]
// r[verify binette.tags.scalar-payload]
// r[verify binette.endianness]
// r[verify binette.length.canonical-width]
// r[verify binette.scalar.bytes]
// r[verify binette.scalar.char]
// r[verify binette.scalar.float]
// r[verify binette.scalar.signed]
// r[verify binette.scalar.unit]
// r[verify binette.scalar.unsigned]
// r[verify binette.value-kind.preserved]
// r[verify binette.value-model]
#[test]
fn self_describing_scalars_use_fixed_tags_and_payloads() {
    let value = Value::String("binette".to_owned());
    let bytes = encode_self_described_to_vec(&value).unwrap();

    assert_eq!(
        bytes,
        [0x0F, 7, 0, 0, 0, b'b', b'i', b'n', b'e', b't', b't', b'e']
    );
    assert_eq!(decode_self_described_from_slice(&bytes).unwrap(), value);

    let err = decode_self_described_from_slice(&[0x01, 0x02]).unwrap_err();
    assert!(matches!(
        err,
        SelfDescribingError::Compact(CompactError::InvalidBool {
            position: 1,
            value: 0x02,
        })
    ));

    for (value, bytes) in [
        (Value::Unit, vec![0x00]),
        (Value::U16(0x1234), vec![0x03, 0x34, 0x12]),
        (Value::I16(-2), vec![0x08, 0xFE, 0xFF]),
        (Value::F32(1.0), vec![0x0C, 0x00, 0x00, 0x80, 0x3F]),
        (Value::Char('\u{00E9}'), vec![0x0E, 0xE9, 0x00, 0x00, 0x00]),
        (Value::Bytes(vec![1, 2, 3]), vec![0x10, 3, 0, 0, 0, 1, 2, 3]),
    ] {
        assert_eq!(encode_self_described_to_vec(&value).unwrap(), bytes);
        assert_eq!(decode_self_described_from_slice(&bytes).unwrap(), value);
    }
}

// r[verify binette.aggregate.struct.self-describing]
// r[verify binette.aggregate.enum.self-describing]
// r[verify binette.tags.aggregate-payload]
#[test]
fn self_describing_structs_and_enums_round_trip_names_and_payloads() {
    let value = Value::Struct(vec![
        FieldValue {
            name: "id".to_owned(),
            value: Value::U64(42),
        },
        FieldValue {
            name: "event".to_owned(),
            value: Value::Enum(EnumValue {
                variant: "renamed".to_owned(),
                payload: Box::new(Value::String("binette".to_owned())),
            }),
        },
    ]);

    let bytes = encode_self_described_to_vec(&value).unwrap();
    assert_eq!(bytes[0], 0x17);
    assert_eq!(decode_self_described_from_slice(&bytes).unwrap(), value);
}

// r[verify binette.aggregate.set]
// r[verify binette.aggregate.map]
// r[verify binette.aggregate.set-map.canonical]
#[test]
fn self_describing_sets_and_maps_sort_by_complete_encoded_key_bytes() {
    let set = Value::Set(vec![Value::U16(1), Value::U16(256)]);
    let set_bytes = encode_self_described_to_vec(&set).unwrap();
    assert_eq!(set_bytes, [0x13, 2, 0, 0, 0, 0x03, 0, 1, 0x03, 1, 0]);
    assert_eq!(
        decode_self_described_from_slice(&set_bytes).unwrap(),
        Value::Set(vec![Value::U16(256), Value::U16(1)])
    );

    let map = Value::Map(vec![
        (Value::U16(1), Value::String("one".to_owned())),
        (Value::U16(256), Value::String("two-five-six".to_owned())),
    ]);
    let map_bytes = encode_self_described_to_vec(&map).unwrap();
    assert_eq!(map_bytes[0], 0x14);
    assert_eq!(
        decode_self_described_from_slice(&map_bytes).unwrap(),
        Value::Map(vec![
            (Value::U16(256), Value::String("two-five-six".to_owned())),
            (Value::U16(1), Value::String("one".to_owned())),
        ])
    );
}

// r[verify binette.aggregate.set-map.decode-policy]
#[test]
fn self_describing_decode_rejects_noncanonical_set_order() {
    let bytes = [0x13, 2, 0, 0, 0, 0x03, 1, 0, 0x03, 0, 1];
    let err = decode_self_described_from_slice(&bytes).unwrap_err();

    assert!(matches!(
        err,
        SelfDescribingError::Compact(CompactError::NonCanonicalOrder {
            position: 8,
            aggregate: "set",
        })
    ));
}

// r[verify binette.aggregate.set-map.float-keys]
#[test]
fn self_describing_set_and_map_keys_reject_nan_payloads() {
    let value = Value::Set(vec![Value::F32(f32::NAN)]);
    assert!(matches!(
        encode_self_described_to_vec(&value).unwrap_err(),
        SelfDescribingError::NanCanonicalKey { aggregate: "set" }
    ));

    let bytes = [0x13, 1, 0, 0, 0, 0x0C, 0, 0, 0xc0, 0x7f];
    let err = decode_self_described_from_slice(&bytes).unwrap_err();
    assert!(matches!(
        err,
        SelfDescribingError::NanCanonicalKey { aggregate: "set" }
    ));
}

// r[verify binette.aggregate.array]
// r[verify binette.aggregate.tuple]
#[test]
fn self_describing_arrays_and_tuples_carry_shape_and_arity() {
    let array = Value::Array(ArrayValue {
        dimensions: vec![2, 1],
        elements: vec![Value::U8(10), Value::U8(20)],
    });
    let tuple = Value::Tuple(vec![array, Value::Bool(true)]);

    let bytes = encode_self_described_to_vec(&tuple).unwrap();
    assert_eq!(bytes[0], 0x16);
    assert_eq!(decode_self_described_from_slice(&bytes).unwrap(), tuple);
}

// r[verify binette.aggregate.dynamic-value]
#[test]
fn dynamic_value_helpers_encode_exactly_one_nested_self_described_value() {
    let inner = Value::String("dynamic".to_owned());
    let compact_dynamic = encode_dynamic_value_to_vec(&inner).unwrap();
    let self_describing_dynamic =
        encode_self_described_to_vec(&Value::Dynamic(Box::new(inner.clone()))).unwrap();

    assert_eq!(compact_dynamic[0], 0x0F);
    assert_eq!(self_describing_dynamic[0], 0x1B);
    assert_eq!(&self_describing_dynamic[1..], compact_dynamic.as_slice());
    assert_eq!(
        decode_dynamic_value_from_slice(&compact_dynamic).unwrap(),
        inner
    );
}

// r[verify binette.tags.extension]
// r[verify binette.tags.forward-contract]
// r[verify binette.value-model.extension-form]
#[test]
fn self_describing_extension_tags_preserve_opaque_payloads() {
    let value = Value::Extension(ExtensionValue {
        tag: 0x80,
        id: 7,
        payload: vec![1, 2, 3],
    });
    let bytes = encode_self_described_to_vec(&value).unwrap();

    assert_eq!(bytes, [0x80, 7, 0, 0, 0, 3, 0, 0, 0, 1, 2, 3]);
    assert_eq!(decode_self_described_from_slice(&bytes).unwrap(), value);
}
