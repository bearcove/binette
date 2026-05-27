use facet::Facet;

use super::*;
use crate::encode::{encode_to_vec_with_plan, writer_plan_for};
use crate::hash::primitive_type_id;
use crate::local_access::{
    LocalAccess, LocalBackend, LocalDescriptorImport, LocalDescriptorImportKind, LocalFieldImport,
    LocalOptionEncodeThunks, LocalOptionRepresentation, LocalOptionSequenceDecodeThunks,
    LocalScalarAccess, LocalSequenceDecodeThunks, LocalSequenceEncodeThunks, LocalSequenceStorage,
    LocalThunk, LocalThunkBindings, LocalTypeDescriptor, LocalValueLayout,
};
use crate::reader_plan_for_bundle;

#[derive(Facet)]
struct Fixed {
    id: u64,
    active: bool,
    code: u16,
    marker: char,
}

#[derive(Debug, PartialEq, Facet)]
#[repr(C)]
struct SwiftFixed {
    id: u64,
    active: bool,
    code: u16,
}

#[derive(Facet)]
#[repr(C)]
struct SwiftText {
    id: u64,
    title: String,
    code: u16,
}

#[derive(Debug, PartialEq, Facet)]
#[repr(C)]
struct SwiftMaybeText {
    id: u64,
    maybe: Option<String>,
    code: u16,
}

// r[verify binette.local-access.descriptor]
// r[verify binette.local-access.strict-hybrid]
#[test]
fn strict_local_encode_stencil_accepts_swift_imported_fixed_descriptor() {
    let value = SwiftFixed {
        id: 0x0102_0304_0506_0708,
        active: true,
        code: 0x1122,
    };
    let plan = writer_plan_for::<SwiftFixed>().unwrap();
    let descriptor = swift_fixed_descriptor(plan.root());
    let encoder = strict_local_stencil_encoder_from_plan(&plan, &descriptor).unwrap();

    assert_eq!(encoder.report().mode, StencilMode::Strict);
    assert_eq!(encoder.report().helper_count, 0);
    assert!(encoder.report().helper_paths.is_empty());
    let actual =
        unsafe { encoder.encode_raw_to_vec((&value as *const SwiftFixed).cast()) }.unwrap();
    assert_eq!(actual, encode_to_vec_with_plan(&value, &plan).unwrap());
}

// r[verify binette.local-access.descriptor]
// r[verify binette.local-access.strict-hybrid]
#[test]
fn strict_local_decode_stencil_accepts_swift_imported_fixed_descriptor() {
    let value = SwiftFixed {
        id: 0x1112_1314_1516_1718,
        active: false,
        code: 0x3344,
    };
    let writer_plan = writer_plan_for::<SwiftFixed>().unwrap();
    let mut writer_registry = SchemaRegistry::new();
    writer_registry
        .install_bundle(writer_plan.schema_bundle())
        .unwrap();
    let reader_plan = reader_plan_for_bundle(
        writer_plan.root(),
        &writer_registry,
        writer_plan.root(),
        &writer_registry,
    )
    .unwrap();
    let descriptor = swift_fixed_descriptor(reader_plan.reader_root());
    let decoder =
        strict_local_stencil_decoder_from_plan(&reader_plan, &writer_registry, &descriptor)
            .unwrap();
    let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

    assert_eq!(decoder.report().mode, StencilMode::Strict);
    assert_eq!(decoder.report().helper_count, 0);
    assert!(decoder.report().helper_paths.is_empty());
    assert_eq!(decoder.expected_len(), bytes.len());

    let mut decoded = std::mem::MaybeUninit::<SwiftFixed>::uninit();
    unsafe { decoder.decode_raw_into(&bytes, decoded.as_mut_ptr().cast()) }.unwrap();
    let decoded = unsafe { decoded.assume_init() };
    assert_eq!(decoded, value);
}

// r[verify binette.local-access.descriptor]
// r[verify binette.local-access.strict-hybrid]
#[test]
fn hybrid_local_encode_stencil_uses_bound_backend_thunk_for_string_subtree() {
    let value = SwiftText {
        id: 0x0102_0304_0506_0708,
        title: "hello from a bound thunk".to_owned(),
        code: 0x1122,
    };
    let plan = writer_plan_for::<SwiftText>().unwrap();
    let descriptor = swift_text_descriptor(plan.root());

    let strict_error = match strict_local_stencil_encoder_from_plan(&plan, &descriptor) {
        Ok(_) => panic!("strict local encode must reject thunk-backed string fields"),
        Err(err) => err,
    };
    assert!(matches!(strict_error, StencilError::Unsupported { .. }));

    let thunks = swift_string_thunk_bindings();
    let encoder = hybrid_local_stencil_encoder_from_plan(&plan, &descriptor, &thunks).unwrap();

    assert_eq!(encoder.report().mode, StencilMode::Hybrid);
    assert_eq!(encoder.report().helper_count, 1);
    assert_eq!(encoder.report().helper_paths, vec!["$.title".to_owned()]);
    let actual = unsafe { encoder.encode_raw_to_vec((&value as *const SwiftText).cast()) }.unwrap();
    assert_eq!(actual, encode_to_vec_with_plan(&value, &plan).unwrap());
}

// r[verify binette.local-access.descriptor]
// r[verify binette.local-access.strict-hybrid]
#[test]
fn hybrid_local_decode_stencil_uses_bound_backend_thunk_for_string_subtree() {
    let value = SwiftText {
        id: 0x1112_1314_1516_1718,
        title: "decode through a bound thunk".to_owned(),
        code: 0x3344,
    };
    let writer_plan = writer_plan_for::<SwiftText>().unwrap();
    let mut writer_registry = SchemaRegistry::new();
    writer_registry
        .install_bundle(writer_plan.schema_bundle())
        .unwrap();
    let reader_plan = reader_plan_for_bundle(
        writer_plan.root(),
        &writer_registry,
        writer_plan.root(),
        &writer_registry,
    )
    .unwrap();
    let descriptor = swift_text_descriptor(reader_plan.reader_root());

    let strict_error =
        match strict_local_stencil_decoder_from_plan(&reader_plan, &writer_registry, &descriptor) {
            Ok(_) => panic!("strict local decode must reject thunk-backed string fields"),
            Err(err) => err,
        };
    assert!(matches!(strict_error, StencilError::Unsupported { .. }));

    let thunks = swift_string_thunk_bindings();
    let decoder = hybrid_local_stencil_decoder_from_plan(
        &reader_plan,
        &writer_registry,
        &descriptor,
        &thunks,
    )
    .unwrap();
    let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();

    assert_eq!(decoder.report().mode, StencilMode::Hybrid);
    assert_eq!(decoder.report().helper_count, 1);
    assert_eq!(decoder.report().helper_paths, vec!["$.title".to_owned()]);

    let mut decoded = std::mem::MaybeUninit::<SwiftText>::uninit();
    unsafe { decoder.decode_raw_into(&bytes, decoded.as_mut_ptr().cast()) }.unwrap();
    let decoded = unsafe { decoded.assume_init() };
    assert_eq!(decoded.id, value.id);
    assert_eq!(decoded.title, value.title);
    assert_eq!(decoded.code, value.code);
}

// r[verify binette.local-access.descriptor]
// r[verify binette.local-access.strict-hybrid]
#[test]
fn hybrid_local_encode_stencil_uses_bound_backend_thunk_for_optional_string_subtree() {
    let values = [
        SwiftMaybeText {
            id: 0x0102_0304_0506_0708,
            maybe: Some("optional payload".to_owned()),
            code: 0x1122,
        },
        SwiftMaybeText {
            id: 0x2122_2324_2526_2728,
            maybe: None,
            code: 0x3344,
        },
    ];
    let plan = writer_plan_for::<SwiftMaybeText>().unwrap();
    let descriptor = swift_maybe_text_descriptor(plan.root());

    let strict_error = match strict_local_stencil_encoder_from_plan(&plan, &descriptor) {
        Ok(_) => panic!("strict local encode must reject thunk-backed optional string fields"),
        Err(err) => err,
    };
    assert!(matches!(strict_error, StencilError::Unsupported { .. }));

    let thunks = swift_string_thunk_bindings();
    let encoder = hybrid_local_stencil_encoder_from_plan(&plan, &descriptor, &thunks).unwrap();

    assert_eq!(encoder.report().mode, StencilMode::Hybrid);
    assert_eq!(encoder.report().helper_count, 1);
    assert_eq!(encoder.report().helper_paths, vec!["$.maybe".to_owned()]);
    for value in values {
        let actual =
            unsafe { encoder.encode_raw_to_vec((&value as *const SwiftMaybeText).cast()) }.unwrap();
        assert_eq!(actual, encode_to_vec_with_plan(&value, &plan).unwrap());
    }
}

// r[verify binette.local-access.descriptor]
// r[verify binette.local-access.strict-hybrid]
#[test]
fn hybrid_local_decode_stencil_uses_bound_backend_thunk_for_optional_string_subtree() {
    let values = [
        SwiftMaybeText {
            id: 0x1112_1314_1516_1718,
            maybe: Some("decode optional".to_owned()),
            code: 0x3344,
        },
        SwiftMaybeText {
            id: 0x4142_4344_4546_4748,
            maybe: None,
            code: 0x5566,
        },
    ];
    let writer_plan = writer_plan_for::<SwiftMaybeText>().unwrap();
    let mut writer_registry = SchemaRegistry::new();
    writer_registry
        .install_bundle(writer_plan.schema_bundle())
        .unwrap();
    let reader_plan = reader_plan_for_bundle(
        writer_plan.root(),
        &writer_registry,
        writer_plan.root(),
        &writer_registry,
    )
    .unwrap();
    let descriptor = swift_maybe_text_descriptor(reader_plan.reader_root());

    let strict_error =
        match strict_local_stencil_decoder_from_plan(&reader_plan, &writer_registry, &descriptor) {
            Ok(_) => panic!("strict local decode must reject thunk-backed optional string fields"),
            Err(err) => err,
        };
    assert!(matches!(strict_error, StencilError::Unsupported { .. }));

    let thunks = swift_string_thunk_bindings();
    let decoder = hybrid_local_stencil_decoder_from_plan(
        &reader_plan,
        &writer_registry,
        &descriptor,
        &thunks,
    )
    .unwrap();

    assert_eq!(decoder.report().mode, StencilMode::Hybrid);
    assert_eq!(decoder.report().helper_count, 1);
    assert_eq!(decoder.report().helper_paths, vec!["$.maybe".to_owned()]);

    for value in values {
        let bytes = encode_to_vec_with_plan(&value, &writer_plan).unwrap();
        let mut decoded = std::mem::MaybeUninit::<SwiftMaybeText>::uninit();
        unsafe { decoder.decode_raw_into(&bytes, decoded.as_mut_ptr().cast()) }.unwrap();
        let decoded = unsafe { decoded.assume_init() };
        assert_eq!(decoded, value);
    }
}

fn swift_fixed_descriptor(root: &TypeRef) -> LocalTypeDescriptor {
    LocalTypeDescriptor::from_import(LocalDescriptorImport::swift_probe(
        root.clone(),
        LocalValueLayout::of::<SwiftFixed>(),
        LocalDescriptorImportKind::Struct {
            fields: vec![
                LocalFieldImport {
                    name: "id".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftFixed, id),
                    },
                    descriptor: primitive_import(Primitive::U64, LocalValueLayout::of::<u64>()),
                },
                LocalFieldImport {
                    name: "active".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftFixed, active),
                    },
                    descriptor: primitive_import(Primitive::Bool, LocalValueLayout::of::<bool>()),
                },
                LocalFieldImport {
                    name: "code".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftFixed, code),
                    },
                    descriptor: primitive_import(Primitive::U16, LocalValueLayout::of::<u16>()),
                },
            ],
        },
    ))
    .unwrap()
}

fn swift_text_descriptor(root: &TypeRef) -> LocalTypeDescriptor {
    LocalTypeDescriptor::from_import(LocalDescriptorImport::swift_probe(
        root.clone(),
        LocalValueLayout::of::<SwiftText>(),
        LocalDescriptorImportKind::Struct {
            fields: vec![
                LocalFieldImport {
                    name: "id".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftText, id),
                    },
                    descriptor: primitive_import(Primitive::U64, LocalValueLayout::of::<u64>()),
                },
                LocalFieldImport {
                    name: "title".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftText, title),
                    },
                    descriptor: swift_thunk_string_import(),
                },
                LocalFieldImport {
                    name: "code".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftText, code),
                    },
                    descriptor: primitive_import(Primitive::U16, LocalValueLayout::of::<u16>()),
                },
            ],
        },
    ))
    .unwrap()
}

fn swift_maybe_text_descriptor(root: &TypeRef) -> LocalTypeDescriptor {
    LocalTypeDescriptor::from_import(LocalDescriptorImport::swift_probe(
        root.clone(),
        LocalValueLayout::of::<SwiftMaybeText>(),
        LocalDescriptorImportKind::Struct {
            fields: vec![
                LocalFieldImport {
                    name: "id".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftMaybeText, id),
                    },
                    descriptor: primitive_import(Primitive::U64, LocalValueLayout::of::<u64>()),
                },
                LocalFieldImport {
                    name: "maybe".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftMaybeText, maybe),
                    },
                    descriptor: swift_option_string_import(root),
                },
                LocalFieldImport {
                    name: "code".to_owned(),
                    access: LocalAccess::Direct {
                        offset: std::mem::offset_of!(SwiftMaybeText, code),
                    },
                    descriptor: primitive_import(Primitive::U16, LocalValueLayout::of::<u16>()),
                },
            ],
        },
    ))
    .unwrap()
}

fn swift_thunk_string_import() -> LocalDescriptorImport {
    LocalDescriptorImport {
        schema: TypeRef::concrete(primitive_type_id(Primitive::String)).into(),
        backend: LocalBackend::SwiftProbe,
        layout: LocalValueLayout::of::<String>(),
        kind: LocalDescriptorImportKind::Scalar(LocalScalarAccess::String(
            LocalSequenceStorage::Thunk {
                len: swift_string_len_thunk(),
                element: swift_string_element_thunk(),
                write: Some(swift_string_write_thunk()),
            },
        )),
    }
}

fn swift_option_string_import(owner: &TypeRef) -> LocalDescriptorImport {
    LocalDescriptorImport {
        schema: crate::local_access::LocalSchemaRef::Position {
            owner: owner.clone(),
            path: "maybe".to_owned(),
        },
        backend: LocalBackend::SwiftProbe,
        layout: LocalValueLayout::of::<Option<String>>(),
        kind: LocalDescriptorImportKind::Option {
            some: Box::new(swift_thunk_string_import()),
            representation: LocalOptionRepresentation::Thunk {
                is_some: swift_option_is_some_thunk(),
                some: swift_option_some_thunk(),
                write_none: Some(swift_option_write_none_thunk()),
                write_some_bytes: Some(swift_option_write_some_bytes_thunk()),
            },
        },
    }
}

fn swift_string_thunk_bindings() -> LocalThunkBindings {
    LocalThunkBindings::new()
        .with_sequence_u8(
            swift_string_len_thunk(),
            swift_string_element_thunk(),
            LocalSequenceEncodeThunks {
                len: test_swift_string_len,
                element_u8: test_swift_string_element,
                context: 0,
            },
        )
        .with_sequence_decode(
            swift_string_write_thunk(),
            LocalSequenceDecodeThunks {
                write_bytes: test_swift_string_write,
                context: 0,
            },
        )
        .with_option(
            swift_option_is_some_thunk(),
            swift_option_some_thunk(),
            LocalOptionEncodeThunks {
                is_some: test_swift_option_is_some,
                some: test_swift_option_some,
                context: 0,
            },
        )
        .with_option_sequence_decode(
            swift_option_write_none_thunk(),
            swift_option_write_some_bytes_thunk(),
            LocalOptionSequenceDecodeThunks {
                write_none: test_swift_option_write_none,
                write_some_bytes: test_swift_option_write_some_bytes,
                context: 0,
            },
        )
}

fn swift_string_len_thunk() -> LocalThunk {
    LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.utf8.count")
}

fn swift_string_element_thunk() -> LocalThunk {
    LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.utf8.element")
}

fn swift_string_write_thunk() -> LocalThunk {
    LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.init.utf8")
}

fn swift_option_is_some_thunk() -> LocalThunk {
    LocalThunk::new(LocalBackend::SwiftProbe, "Swift.Optional.isSome")
}

fn swift_option_some_thunk() -> LocalThunk {
    LocalThunk::new(LocalBackend::SwiftProbe, "Swift.Optional.some")
}

fn swift_option_write_none_thunk() -> LocalThunk {
    LocalThunk::new(LocalBackend::SwiftProbe, "Swift.Optional.init.none")
}

fn swift_option_write_some_bytes_thunk() -> LocalThunk {
    LocalThunk::new(
        LocalBackend::SwiftProbe,
        "Swift.Optional<String>.init.some.utf8",
    )
}

unsafe extern "C" fn test_swift_string_len(
    value: *const u8,
    _context: *mut std::ffi::c_void,
) -> usize {
    unsafe { (&*value.cast::<String>()).len() }
}

unsafe extern "C" fn test_swift_string_element(
    value: *const u8,
    index: usize,
    _context: *mut std::ffi::c_void,
) -> u8 {
    unsafe { (&*value.cast::<String>()).as_bytes()[index] }
}

unsafe extern "C" fn test_swift_string_write(
    value: *mut u8,
    ptr: *const u8,
    len: usize,
    _context: *mut std::ffi::c_void,
) -> bool {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(string) = String::from_utf8(bytes.to_vec()) else {
        return false;
    };
    unsafe { value.cast::<String>().write(string) };
    true
}

unsafe extern "C" fn test_swift_option_is_some(
    value: *const u8,
    _context: *mut std::ffi::c_void,
) -> bool {
    unsafe { (&*value.cast::<Option<String>>()).is_some() }
}

unsafe extern "C" fn test_swift_option_some(
    value: *const u8,
    _context: *mut std::ffi::c_void,
) -> *const u8 {
    unsafe {
        (&*value.cast::<Option<String>>())
            .as_ref()
            .map_or(std::ptr::null(), |value| {
                value as *const String as *const u8
            })
    }
}

unsafe extern "C" fn test_swift_option_write_none(
    value: *mut u8,
    _context: *mut std::ffi::c_void,
) -> bool {
    unsafe { value.cast::<Option<String>>().write(None) };
    true
}

unsafe extern "C" fn test_swift_option_write_some_bytes(
    value: *mut u8,
    ptr: *const u8,
    len: usize,
    _context: *mut std::ffi::c_void,
) -> bool {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(string) = String::from_utf8(bytes.to_vec()) else {
        return false;
    };
    unsafe { value.cast::<Option<String>>().write(Some(string)) };
    true
}

fn primitive_import(primitive: Primitive, layout: LocalValueLayout) -> LocalDescriptorImport {
    LocalDescriptorImport {
        schema: TypeRef::concrete(primitive_type_id(primitive)).into(),
        backend: LocalBackend::SwiftProbe,
        layout,
        kind: LocalDescriptorImportKind::Scalar(LocalScalarAccess::Plain),
    }
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
    assert_eq!(encoder.report().mode, StencilMode::Strict);
    assert_eq!(encoder.report().helper_count, 0);
    assert!(encoder.report().helper_paths.is_empty());
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
fn option_string_encode_stencil_uses_niche_layout_without_facet_option_helper() {
    type Value = Option<String>;

    let value = Some("payload".to_owned());
    let plan = writer_plan_for::<Value>().unwrap();

    let mut compiler = StencilEncodeCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler.compile_root::<Value>(plan.root_node()).unwrap();

    let [EncodeStencilOp::Option { layout, .. }] = compiler.ops.as_slice() else {
        panic!("expected one option encode op, got {:#?}", compiler.ops);
    };
    assert_eq!(*layout, EncodeOptionLayout::NicheString);
    assert!(compiler.helpers.is_empty());

    let encoder = strict_stencil_encoder_from_plan::<Value>(&plan).unwrap();
    assert_eq!(
        encoder.encode_to_vec(&value).unwrap(),
        encode_to_vec_with_plan(&value, &plan).unwrap()
    );
    assert_eq!(
        encoder.encode_to_vec(&None).unwrap(),
        encode_to_vec_with_plan(&None::<String>, &plan).unwrap()
    );
}

#[test]
fn list_encode_stencil_uses_vec_layout_without_facet_list_helpers() {
    type Value = Vec<(u16, String)>;

    let value = vec![(1, "one".to_owned()), (2, "two".to_owned())];
    let plan = writer_plan_for::<Value>().unwrap();

    let mut compiler = StencilEncodeCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler.compile_root::<Value>(plan.root_node()).unwrap();

    let [EncodeStencilOp::List { layout, .. }] = compiler.ops.as_slice() else {
        panic!("expected one list encode op, got {:#?}", compiler.ops);
    };
    assert!(matches!(layout, EncodeListLayout::Vec { .. }));
    assert!(compiler.helpers.is_empty());

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

    let encoder = hybrid_stencil_encoder_from_plan::<Value>(&plan).unwrap();
    assert_eq!(encoder.report().mode, StencilMode::Hybrid);
    assert_eq!(encoder.report().helper_count, 1);
    assert_eq!(encoder.report().helper_paths, vec!["$"]);
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
        plan_nodes: reader_plan.nodes(),
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
        plan_nodes: reader_plan.nodes(),
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
