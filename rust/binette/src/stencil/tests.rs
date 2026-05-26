use facet::Facet;

use super::*;
use crate::encode::{encode_to_vec_with_plan, writer_plan_for};

#[derive(Facet)]
struct Fixed {
    id: u64,
    active: bool,
    code: u16,
    marker: char,
}

#[test]
fn fixed_encode_stencil_uses_direct_entry() {
    let value = Fixed {
        id: 0x0102_0304_0506_0708,
        active: true,
        code: 0x1122,
        marker: 'b',
    };
    let plan = writer_plan_for::<Fixed>().unwrap();
    let encoder = stencil_encoder_from_plan::<Fixed>(&plan).unwrap();

    assert!(matches!(encoder.entry, EncodeStencilEntry::Direct { .. }));
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[derive(Facet)]
struct FixedInner {
    count: u32,
    enabled: bool,
}

#[derive(Facet)]
struct FixedOuter {
    id: u64,
    inner: FixedInner,
    code: u16,
}

#[test]
fn hybrid_encode_uses_direct_entry_for_nested_fixed_shapes() {
    let value = FixedOuter {
        id: 0x0102_0304_0506_0708,
        inner: FixedInner {
            count: 42,
            enabled: true,
        },
        code: 0x1122,
    };
    let plan = writer_plan_for::<FixedOuter>().unwrap();
    let encoder = hybrid_stencil_encoder_from_plan::<FixedOuter>(&plan).unwrap();

    assert!(matches!(encoder.entry, EncodeStencilEntry::Direct { .. }));
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[derive(Facet)]
struct MixedNested {
    count: u32,
    label: String,
    enabled: bool,
}

#[derive(Facet)]
struct Mixed {
    id: u64,
    title: String,
    active: bool,
    nested: MixedNested,
    code: u16,
}

#[test]
fn mixed_encode_stencil_compiles_nested_strings_without_helpers() {
    let value = Mixed {
        id: 0x0102_0304_0506_0708,
        title: "binette".to_owned(),
        active: true,
        nested: MixedNested {
            count: 42,
            label: "nested".to_owned(),
            enabled: false,
        },
        code: 0x1122,
    };
    let plan = writer_plan_for::<Mixed>().unwrap();

    let mut compiler = StencilEncodeCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler.compile_root::<Mixed>(plan.root_node()).unwrap();

    let direct_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Direct { .. }))
        .count();
    let bytes_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Bytes { .. }))
        .count();
    let helper_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Helper { .. }))
        .count();

    assert!(direct_segments >= 3);
    assert_eq!(bytes_segments, 2);
    assert_eq!(helper_segments, 0);

    let encoder = stencil_encoder_from_plan::<Mixed>(&plan).unwrap();
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[derive(Facet)]
#[allow(dead_code)]
#[repr(u8)]
enum MixedEvent {
    Started,
    Moved(u32, u16),
    Failed { code: u16, flag: bool },
    Message { code: u16, text: String },
}

#[test]
fn enum_encode_stencil_compiles_payloads_without_helpers() {
    let value = MixedEvent::Message {
        code: 0x1122,
        text: "payload".to_owned(),
    };
    let plan = writer_plan_for::<MixedEvent>().unwrap();

    let mut compiler = StencilEncodeCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler
        .compile_root::<MixedEvent>(plan.root_node())
        .unwrap();

    let enum_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Enum { .. }))
        .count();
    let helper_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Helper { .. }))
        .count();

    assert_eq!(enum_segments, 1);
    assert_eq!(helper_segments, 0);
    assert_eq!(compiler.helpers.len(), 0);

    let encoder = stencil_encoder_from_plan::<MixedEvent>(&plan).unwrap();
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[test]
fn strict_encode_accepts_helperless_enum_stencils() {
    let value = MixedEvent::Message {
        code: 0x1122,
        text: "payload".to_owned(),
    };
    let plan = writer_plan_for::<MixedEvent>().unwrap();
    let encoder = strict_stencil_encoder_from_plan::<MixedEvent>(&plan).unwrap();

    match &encoder.entry {
        EncodeStencilEntry::Direct { .. } => {}
        EncodeStencilEntry::Helper { runtime, .. } => assert!(runtime.helpers.is_empty()),
    }
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[test]
fn option_encode_stencil_compiles_helperless_some_payload_without_helpers() {
    type Value = Option<(u16, String)>;

    let value = Some((0x1122, "payload".to_owned()));
    let plan = writer_plan_for::<Value>().unwrap();

    let mut compiler = StencilEncodeCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler.compile_root::<Value>(plan.root_node()).unwrap();

    let option_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Option { .. }))
        .count();
    let helper_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Helper { .. }))
        .count();

    assert_eq!(option_segments, 1);
    assert_eq!(helper_segments, 0);
    assert_eq!(compiler.helpers.len(), 0);

    let encoder = strict_stencil_encoder_from_plan::<Value>(&plan).unwrap();
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[test]
fn strict_encode_rejects_option_payload_that_needs_helper() {
    type Value = Option<std::collections::HashSet<u16>>;

    let plan = writer_plan_for::<Value>().unwrap();
    assert!(matches!(
        strict_stencil_encoder_from_plan::<Value>(&plan),
        Err(StencilError::Unsupported { .. })
    ));
}

#[test]
fn list_encode_stencil_compiles_helperless_elements_without_helpers() {
    type Value = Vec<(u16, String)>;

    let value = vec![(1, "one".to_owned()), (2, "two".to_owned())];
    let plan = writer_plan_for::<Value>().unwrap();

    let mut compiler = StencilEncodeCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler.compile_root::<Value>(plan.root_node()).unwrap();

    let list_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::List { .. }))
        .count();
    let helper_segments = compiler
        .ops
        .iter()
        .filter(|op| matches!(op, EncodeStencilOp::Helper { .. }))
        .count();

    assert_eq!(list_segments, 1);
    assert_eq!(helper_segments, 0);
    assert_eq!(compiler.helpers.len(), 0);

    let encoder = strict_stencil_encoder_from_plan::<Value>(&plan).unwrap();
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[test]
fn strict_encode_accepts_nested_list_stencils() {
    type Value = Vec<Vec<u16>>;

    let value = vec![vec![1, 2, 3], vec![5, 8]];
    let plan = writer_plan_for::<Value>().unwrap();
    let encoder = strict_stencil_encoder_from_plan::<Value>(&plan).unwrap();

    match &encoder.entry {
        EncodeStencilEntry::Direct { .. } => {}
        EncodeStencilEntry::Helper { runtime, .. } => assert!(runtime.helpers.is_empty()),
    }
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[derive(Facet)]
struct MixedAggregateNested {
    count: u32,
    label: String,
    enabled: bool,
}

#[derive(Facet)]
struct MixedAggregate {
    id: u64,
    title: String,
    counts: Vec<u32>,
    maybe: Option<String>,
    nested: MixedAggregateNested,
    pair: (u16, String),
}

#[test]
fn strict_encode_accepts_mixed_struct_with_list_option_and_strings() {
    let value = MixedAggregate {
        id: 0x0102_0304_0506_0708,
        title: "binette baseline".to_owned(),
        counts: vec![1, 2, 3, 5, 8],
        maybe: Some("present".to_owned()),
        nested: MixedAggregateNested {
            count: 42,
            label: "nested".to_owned(),
            enabled: true,
        },
        pair: (7, "seven".to_owned()),
    };
    let plan = writer_plan_for::<MixedAggregate>().unwrap();
    let encoder = strict_stencil_encoder_from_plan::<MixedAggregate>(&plan).unwrap();

    match &encoder.entry {
        EncodeStencilEntry::Direct { .. } => {}
        EncodeStencilEntry::Helper { runtime, .. } => assert!(runtime.helpers.is_empty()),
    }
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
}

#[test]
fn hybrid_decode_compiles_supported_siblings_around_subtree_helpers() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Message {
            pub head: u32,
            pub title: String,
            pub middle: u16,
            pub pair: (u8, String, u32),
            pub tail: u64,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Message {
            pub head: u32,
            pub title: String,
            pub middle: u16,
            pub pair: (u8, String, u32),
            pub tail: u64,
        }
    }

    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let mut writer_registry = SchemaRegistry::new();
    writer_registry
        .install_bundle(writer_plan.schema_bundle())
        .unwrap();
    let reader_plan =
        reader_plan_for::<reader::Message>(writer_plan.root(), &writer_registry).unwrap();

    let mut compiler = CursorStencilCompiler {
        writer_registry: &writer_registry,
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
        allow_helpers: true,
    };
    compiler
        .compile_root::<reader::Message>(&reader_plan.root)
        .unwrap();

    let op_kinds: Vec<&'static str> = compiler
        .ops
        .iter()
        .map(|op| match op {
            HybridStencilOp::Copy { .. } => "copy",
            HybridStencilOp::Helper { .. } => "helper",
            HybridStencilOp::List { .. } => "list",
        })
        .collect();

    assert_eq!(
        op_kinds,
        vec!["copy", "helper", "copy", "copy", "helper", "copy", "copy"]
    );
    assert_eq!(compiler.helpers.len(), 2);
}

#[test]
fn hybrid_decode_compiles_list_element_siblings_around_subtree_helpers() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Message {
            pub prefix: u16,
            pub items: Vec<(u16, String, u32)>,
            pub tail: u64,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Message {
            pub prefix: u16,
            pub items: Vec<(u16, String, u32)>,
            pub tail: u64,
        }
    }

    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let mut writer_registry = SchemaRegistry::new();
    writer_registry
        .install_bundle(writer_plan.schema_bundle())
        .unwrap();
    let reader_plan =
        reader_plan_for::<reader::Message>(writer_plan.root(), &writer_registry).unwrap();

    let mut compiler = CursorStencilCompiler {
        writer_registry: &writer_registry,
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
        allow_helpers: true,
    };
    compiler
        .compile_root::<reader::Message>(&reader_plan.root)
        .unwrap();

    assert_eq!(compiler.ops.len(), 3);
    assert!(matches!(compiler.ops[0], HybridStencilOp::Copy { .. }));
    let HybridStencilOp::List { element_ops, .. } = &compiler.ops[1] else {
        panic!("expected a native list op");
    };
    assert!(matches!(compiler.ops[2], HybridStencilOp::Copy { .. }));

    let element_kinds: Vec<&'static str> = element_ops
        .iter()
        .map(|op| match op {
            HybridStencilOp::Copy { .. } => "copy",
            HybridStencilOp::Helper { .. } => "helper",
            HybridStencilOp::List { .. } => "list",
        })
        .collect();

    assert_eq!(element_kinds, vec!["copy", "helper", "copy"]);
    assert_eq!(compiler.helpers.len(), 1);
}
