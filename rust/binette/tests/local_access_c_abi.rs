use facet::Facet;

use binette::local_access::{
    BINETTE_LOCAL_ACCESS_DIRECT, BINETTE_LOCAL_ACCESS_THUNK, BINETTE_LOCAL_BACKEND_SWIFT,
    BINETTE_LOCAL_EXTERNAL_METADATA_UNIT, BINETTE_LOCAL_KIND_ENUM, BINETTE_LOCAL_KIND_SCALAR,
    BINETTE_LOCAL_OPTION_DIRECT_TAG, BINETTE_LOCAL_SCALAR_PLAIN, BINETTE_LOCAL_SCALAR_STRING,
    BINETTE_LOCAL_SCHEMA_REF_TYPE, BINETTE_LOCAL_SEQUENCE_INLINE_FIXED,
    BINETTE_LOCAL_SEQUENCE_THUNK, BinetteLocalDescriptorAbi, BinetteLocalEnumAbi,
    BinetteLocalEnumTagAccessAbi, BinetteLocalEnumTagThunkAbi, BinetteLocalExternalAbi,
    BinetteLocalExternalMetadataAbi, BinetteLocalKindAbi, BinetteLocalLayoutAbi,
    BinetteLocalOptionAbi, BinetteLocalOptionRepresentationAbi, BinetteLocalOptionThunksAbi,
    BinetteLocalScalarAbi, BinetteLocalSchemaRefAbi, BinetteLocalSequenceAbi,
    BinetteLocalSequenceStorageAbi, BinetteLocalSequenceThunksAbi, BinetteLocalStrAbi,
    BinetteLocalStructAbi, BinetteLocalVariantAbi, BinetteLocalVariantConstructAbi,
    BinetteLocalVariantDropAbi, BinetteLocalVariantProjectAccessAbi,
    BinetteLocalVariantProjectIntoAbi, BinetteLocalVariantProjectThunkAbi, LocalTypeDescriptor,
    LocalValueLayout,
};
use binette::{
    Primitive, SchemaRegistry, StencilError, StencilMode, TypeRef, encode_to_vec_with_plan,
    hybrid_local_stencil_decoder_from_plan, hybrid_local_stencil_encoder_from_plan,
    primitive_type_id, reader_plan_for_bundle, strict_local_stencil_decoder_from_plan,
    strict_local_stencil_encoder_from_plan, writer_plan_for,
};

// r[verify binette.local-access.descriptor+2]
// r[verify binette.local-access.strict-hybrid]
// r[verify binette.compat.enum]
// r[verify binette.compat.enum.payload]
#[test]
fn c_abi_descriptor_drives_hybrid_local_enum_message_stencil() {
    #[derive(Debug, PartialEq, Facet)]
    #[repr(C)]
    enum Message {
        Hi(String),
        Bye(u32),
    }

    unsafe extern "C" fn message_tag(value: *const u8, _context: *mut std::ffi::c_void) -> u32 {
        match unsafe { &*value.cast::<Message>() } {
            Message::Hi(_) => 0,
            Message::Bye(_) => 1,
        }
    }

    unsafe extern "C" fn project_hi(
        value: *const u8,
        _context: *mut std::ffi::c_void,
    ) -> *const u8 {
        match unsafe { &*value.cast::<Message>() } {
            Message::Hi(text) => (text as *const String).cast(),
            Message::Bye(_) => std::ptr::null(),
        }
    }

    unsafe extern "C" fn project_hi_into(
        value: *const u8,
        out: *mut u8,
        out_len: usize,
        _context: *mut std::ffi::c_void,
    ) -> bool {
        if out_len != std::mem::size_of::<String>() {
            return false;
        }
        let Message::Hi(text) = (unsafe { &*value.cast::<Message>() }) else {
            return false;
        };
        unsafe { out.cast::<String>().write(text.clone()) };
        true
    }

    unsafe extern "C" fn drop_projected_string(value: *mut u8, _context: *mut std::ffi::c_void) {
        unsafe { std::ptr::drop_in_place(value.cast::<String>()) };
    }

    unsafe extern "C" fn project_bye(
        value: *const u8,
        _context: *mut std::ffi::c_void,
    ) -> *const u8 {
        match unsafe { &*value.cast::<Message>() } {
            Message::Hi(_) => std::ptr::null(),
            Message::Bye(code) => (code as *const u32).cast(),
        }
    }

    unsafe extern "C" fn project_bye_into(
        value: *const u8,
        out: *mut u8,
        out_len: usize,
        _context: *mut std::ffi::c_void,
    ) -> bool {
        if out_len != std::mem::size_of::<u32>() {
            return false;
        }
        let Message::Bye(code) = (unsafe { &*value.cast::<Message>() }) else {
            return false;
        };
        unsafe { out.cast::<u32>().write(*code) };
        true
    }

    unsafe extern "C" fn construct_hi(
        value: *mut u8,
        payload: *const u8,
        payload_len: usize,
        _context: *mut std::ffi::c_void,
    ) -> bool {
        let bytes = unsafe { std::slice::from_raw_parts(payload, payload_len) };
        let Ok(text) = String::from_utf8(bytes.to_vec()) else {
            return false;
        };
        unsafe { value.cast::<Message>().write(Message::Hi(text)) };
        true
    }

    unsafe extern "C" fn construct_bye(
        value: *mut u8,
        payload: *const u8,
        payload_len: usize,
        _context: *mut std::ffi::c_void,
    ) -> bool {
        if payload_len != std::mem::size_of::<u32>() {
            return false;
        }
        let mut code = std::mem::MaybeUninit::<u32>::uninit();
        unsafe {
            std::ptr::copy_nonoverlapping(
                payload,
                code.as_mut_ptr().cast::<u8>(),
                std::mem::size_of::<u32>(),
            )
        };
        let code = unsafe { code.assume_init() };
        unsafe { value.cast::<Message>().write(Message::Bye(code)) };
        true
    }

    let values = [
        Message::Hi("hello from C ABI structs".to_owned()),
        Message::Bye(0xCAFE_BABE),
    ];
    let writer_plan = writer_plan_for::<Message>().unwrap();
    let root_type_id = concrete_type_id(writer_plan.root());
    let string_descriptor = c_abi_string_descriptor();
    let u32_descriptor = c_abi_plain_descriptor(
        primitive_type_id(Primitive::U32).0,
        LocalValueLayout::of::<u32>(),
    );
    let variants = [
        BinetteLocalVariantAbi {
            name: c_abi_str("Hi"),
            index: 0,
            project: BinetteLocalVariantProjectAccessAbi {
                tag: BINETTE_LOCAL_ACCESS_THUNK,
                direct_offset: 0,
                thunk: BinetteLocalVariantProjectThunkAbi {
                    call: Some(project_hi),
                    context: std::ptr::null_mut(),
                },
            },
            project_into: BinetteLocalVariantProjectIntoAbi {
                call: Some(project_hi_into),
                context: std::ptr::null_mut(),
            },
            drop_projected: BinetteLocalVariantDropAbi {
                call: Some(drop_projected_string),
                context: std::ptr::null_mut(),
            },
            construct: BinetteLocalVariantConstructAbi {
                call: Some(construct_hi),
                context: std::ptr::null_mut(),
            },
            payload: &string_descriptor,
        },
        BinetteLocalVariantAbi {
            name: c_abi_str("Bye"),
            index: 1,
            project: BinetteLocalVariantProjectAccessAbi {
                tag: BINETTE_LOCAL_ACCESS_THUNK,
                direct_offset: 0,
                thunk: BinetteLocalVariantProjectThunkAbi {
                    call: Some(project_bye),
                    context: std::ptr::null_mut(),
                },
            },
            project_into: BinetteLocalVariantProjectIntoAbi {
                call: Some(project_bye_into),
                context: std::ptr::null_mut(),
            },
            drop_projected: BinetteLocalVariantDropAbi {
                call: None,
                context: std::ptr::null_mut(),
            },
            construct: BinetteLocalVariantConstructAbi {
                call: Some(construct_bye),
                context: std::ptr::null_mut(),
            },
            payload: &u32_descriptor,
        },
    ];
    let descriptor_abi = BinetteLocalDescriptorAbi {
        schema: c_abi_type_schema(root_type_id),
        backend: BINETTE_LOCAL_BACKEND_SWIFT,
        layout: c_abi_layout(LocalValueLayout::of::<Message>()),
        kind: BinetteLocalKindAbi {
            tag: BINETTE_LOCAL_KIND_ENUM,
            enumeration: BinetteLocalEnumAbi {
                tag: BinetteLocalEnumTagAccessAbi {
                    tag: BINETTE_LOCAL_ACCESS_THUNK,
                    direct_offset: 0,
                    thunk: BinetteLocalEnumTagThunkAbi {
                        call: Some(message_tag),
                        context: std::ptr::null_mut(),
                    },
                },
                variants: variants.as_ptr(),
                variant_count: variants.len(),
            },
            ..empty_c_abi_kind()
        },
    };
    let imported = unsafe { LocalTypeDescriptor::from_abi(&descriptor_abi) }.unwrap();

    let strict_encode_error =
        match strict_local_stencil_encoder_from_plan(&writer_plan, &imported.descriptor) {
            Ok(_) => panic!("strict local encode must reject thunk-backed C ABI enum"),
            Err(err) => err,
        };
    assert!(matches!(
        strict_encode_error,
        StencilError::Unsupported { .. }
    ));

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
    let strict_decode_error = match strict_local_stencil_decoder_from_plan(
        &reader_plan,
        &writer_registry,
        &imported.descriptor,
    ) {
        Ok(_) => panic!("strict local decode must reject thunk-backed C ABI enum"),
        Err(err) => err,
    };
    assert!(matches!(
        strict_decode_error,
        StencilError::Unsupported { .. }
    ));

    let encoder = hybrid_local_stencil_encoder_from_plan(
        &writer_plan,
        &imported.descriptor,
        &imported.thunks,
    )
    .unwrap();
    let decoder = hybrid_local_stencil_decoder_from_plan(
        &reader_plan,
        &writer_registry,
        &imported.descriptor,
        &imported.thunks,
    )
    .unwrap();

    assert_eq!(encoder.report().mode, StencilMode::Hybrid);
    assert_eq!(encoder.report().helper_paths, vec!["$".to_owned()]);
    assert_eq!(decoder.report().mode, StencilMode::Hybrid);
    assert_eq!(decoder.report().helper_paths, vec!["$".to_owned()]);

    for value in values {
        let encoded =
            unsafe { encoder.encode_raw_to_vec((&value as *const Message).cast()) }.unwrap();
        assert_eq!(
            encoded,
            encode_to_vec_with_plan(&value, &writer_plan).unwrap()
        );

        let mut decoded = std::mem::MaybeUninit::<Message>::uninit();
        unsafe { decoder.decode_raw_into(&encoded, decoded.as_mut_ptr().cast()) }.unwrap();
        let decoded = unsafe { decoded.assume_init() };
        assert_eq!(decoded, value);
    }
}

fn concrete_type_id(type_ref: &TypeRef) -> u64 {
    let TypeRef::Concrete { type_id, args } = type_ref else {
        panic!("expected concrete root type ref");
    };
    assert!(
        args.is_empty(),
        "C ABI test helper only handles plain type refs"
    );
    type_id.0
}

fn c_abi_str(value: &'static str) -> BinetteLocalStrAbi {
    BinetteLocalStrAbi {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

fn c_abi_type_schema(type_id: u64) -> BinetteLocalSchemaRefAbi {
    BinetteLocalSchemaRefAbi {
        tag: BINETTE_LOCAL_SCHEMA_REF_TYPE,
        type_id,
        owner_type_id: 0,
        path: BinetteLocalStrAbi::empty(),
    }
}

fn c_abi_layout(layout: LocalValueLayout) -> BinetteLocalLayoutAbi {
    BinetteLocalLayoutAbi {
        size: layout.size,
        align: layout.align,
        stride: layout.stride,
    }
}

fn c_abi_sequence_storage_empty() -> BinetteLocalSequenceStorageAbi {
    BinetteLocalSequenceStorageAbi {
        tag: BINETTE_LOCAL_SEQUENCE_INLINE_FIXED,
        offset: 0,
        element_count: 0,
        pointer_offset: 0,
        length_offset: 0,
        has_capacity: 0,
        capacity_offset: 0,
        element_stride: 0,
        thunks: BinetteLocalSequenceThunksAbi {
            len: None,
            element_u8: None,
            element_ptr: None,
            write_bytes: None,
            write_fixed_elements: None,
            context: std::ptr::null_mut(),
        },
    }
}

fn c_abi_option_empty() -> BinetteLocalOptionAbi {
    BinetteLocalOptionAbi {
        some: std::ptr::null(),
        representation: BinetteLocalOptionRepresentationAbi {
            tag: BINETTE_LOCAL_OPTION_DIRECT_TAG,
            tag_offset: 0,
            tag_width: 1,
            none_value: 0,
            some_value: 1,
            some_offset: 0,
            none_bytes: std::ptr::null(),
            none_bytes_len: 0,
            thunks: BinetteLocalOptionThunksAbi {
                is_some: None,
                some: None,
                write_none: None,
                write_some_bytes: None,
                context: std::ptr::null_mut(),
            },
        },
    }
}

fn empty_c_abi_kind() -> BinetteLocalKindAbi {
    BinetteLocalKindAbi {
        tag: BINETTE_LOCAL_KIND_SCALAR,
        scalar: BinetteLocalScalarAbi {
            tag: BINETTE_LOCAL_SCALAR_PLAIN,
            storage: c_abi_sequence_storage_empty(),
        },
        structure: BinetteLocalStructAbi {
            fields: std::ptr::null(),
            field_count: 0,
        },
        tuple: BinetteLocalStructAbi {
            fields: std::ptr::null(),
            field_count: 0,
        },
        enumeration: BinetteLocalEnumAbi {
            tag: BinetteLocalEnumTagAccessAbi {
                tag: BINETTE_LOCAL_ACCESS_DIRECT,
                direct_offset: 0,
                thunk: BinetteLocalEnumTagThunkAbi {
                    call: None,
                    context: std::ptr::null_mut(),
                },
            },
            variants: std::ptr::null(),
            variant_count: 0,
        },
        sequence: BinetteLocalSequenceAbi {
            element: std::ptr::null(),
            storage: c_abi_sequence_storage_empty(),
        },
        option: c_abi_option_empty(),
        external: BinetteLocalExternalAbi {
            kind: BinetteLocalStrAbi::empty(),
            metadata: BinetteLocalExternalMetadataAbi {
                tag: BINETTE_LOCAL_EXTERNAL_METADATA_UNIT,
                fields: std::ptr::null(),
                field_count: 0,
            },
        },
        text: BinetteLocalStrAbi::empty(),
    }
}

fn c_abi_plain_descriptor(type_id: u64, layout: LocalValueLayout) -> BinetteLocalDescriptorAbi {
    BinetteLocalDescriptorAbi {
        schema: c_abi_type_schema(type_id),
        backend: BINETTE_LOCAL_BACKEND_SWIFT,
        layout: c_abi_layout(layout),
        kind: BinetteLocalKindAbi {
            tag: BINETTE_LOCAL_KIND_SCALAR,
            scalar: BinetteLocalScalarAbi {
                tag: BINETTE_LOCAL_SCALAR_PLAIN,
                storage: c_abi_sequence_storage_empty(),
            },
            ..empty_c_abi_kind()
        },
    }
}

fn c_abi_string_descriptor() -> BinetteLocalDescriptorAbi {
    BinetteLocalDescriptorAbi {
        schema: c_abi_type_schema(primitive_type_id(Primitive::String).0),
        backend: BINETTE_LOCAL_BACKEND_SWIFT,
        layout: c_abi_layout(LocalValueLayout::of::<String>()),
        kind: BinetteLocalKindAbi {
            tag: BINETTE_LOCAL_KIND_SCALAR,
            scalar: BinetteLocalScalarAbi {
                tag: BINETTE_LOCAL_SCALAR_STRING,
                storage: BinetteLocalSequenceStorageAbi {
                    tag: BINETTE_LOCAL_SEQUENCE_THUNK,
                    offset: 0,
                    element_count: 0,
                    pointer_offset: 0,
                    length_offset: 0,
                    has_capacity: 0,
                    capacity_offset: 0,
                    element_stride: 1,
                    thunks: BinetteLocalSequenceThunksAbi {
                        len: Some(string_len),
                        element_u8: Some(string_element),
                        element_ptr: None,
                        write_bytes: Some(string_write),
                        write_fixed_elements: None,
                        context: std::ptr::null_mut(),
                    },
                },
            },
            ..empty_c_abi_kind()
        },
    }
}

unsafe extern "C" fn string_len(value: *const u8, _context: *mut std::ffi::c_void) -> usize {
    unsafe { &*value.cast::<String>() }.len()
}

unsafe extern "C" fn string_element(
    value: *const u8,
    index: usize,
    _context: *mut std::ffi::c_void,
) -> u8 {
    unsafe { &*value.cast::<String>() }.as_bytes()[index]
}

unsafe extern "C" fn string_write(
    value: *mut u8,
    ptr: *const u8,
    len: usize,
    _context: *mut std::ffi::c_void,
) -> bool {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(text) = String::from_utf8(bytes.to_vec()) else {
        return false;
    };
    unsafe { value.cast::<String>().write(text) };
    true
}
