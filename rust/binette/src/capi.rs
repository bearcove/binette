use facet::Facet;

use crate::local_access::{
    BinetteLocalDescriptorAbi, LocalDescriptorAbiImport, LocalTypeDescriptor,
};
use crate::{
    Primitive, SchemaRegistry, TypeRef, encode_to_vec, hybrid_local_stencil_decoder_from_plan,
    hybrid_local_stencil_encoder_from_plan, primitive_type_id, reader_plan_for_bundle,
    writer_plan_for,
};

const BINETTE_STATUS_OK: i32 = 0;
const BINETTE_STATUS_NULL_POINTER: i32 = 1;
const BINETTE_STATUS_DESCRIPTOR: i32 = 2;
const BINETTE_STATUS_PLAN: i32 = 3;
const BINETTE_STATUS_STENCIL: i32 = 4;

#[derive(Debug, Facet)]
#[repr(C)]
enum CanaryMessage {
    Hi(String),
    Bye(u32),
}

pub struct BinetteLocalDescriptorHandle {
    import: LocalDescriptorAbiImport,
}

#[repr(C)]
pub struct BinetteByteBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl BinetteByteBuffer {
    fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    fn from_vec(mut bytes: Vec<u8>) -> Self {
        let buffer = Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        };
        std::mem::forget(bytes);
        buffer
    }
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.descriptor+2]
// r[impl binette.local-access.swift-probes+2]
#[unsafe(no_mangle)]
pub extern "C" fn binette_local_descriptor_import(
    descriptor: *const BinetteLocalDescriptorAbi,
    out: *mut *mut BinetteLocalDescriptorHandle,
) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    *out = std::ptr::null_mut();
    let imported = match unsafe { LocalTypeDescriptor::from_abi(descriptor) } {
        Ok(imported) => imported,
        Err(_) => return BINETTE_STATUS_DESCRIPTOR,
    };
    *out = Box::into_raw(Box::new(BinetteLocalDescriptorHandle { import: imported }));
    BINETTE_STATUS_OK
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_local_descriptor_free(handle: *mut BinetteLocalDescriptorHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_byte_buffer_free(buffer: BinetteByteBuffer) {
    if !buffer.ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.cap) });
    }
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_string_type_id() -> u64 {
    primitive_type_id(Primitive::String).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_u32_type_id() -> u64 {
    primitive_type_id(Primitive::U32).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_canary_message_type_id() -> u64 {
    let Ok(plan) = writer_plan_for::<CanaryMessage>() else {
        return 0;
    };
    concrete_type_id(plan.root()).map_or(0, |type_id| type_id.0)
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.strict-hybrid]
#[unsafe(no_mangle)]
pub extern "C" fn binette_canary_message_encode(
    handle: *const BinetteLocalDescriptorHandle,
    value: *const u8,
    out: *mut BinetteByteBuffer,
) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    if value.is_null() {
        return BINETTE_STATUS_NULL_POINTER;
    }
    let Some(out) = (unsafe { out.as_mut() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    *out = BinetteByteBuffer::empty();
    let plan = match writer_plan_for::<CanaryMessage>() {
        Ok(plan) => plan,
        Err(_) => return BINETTE_STATUS_PLAN,
    };
    let stencil = match hybrid_local_stencil_encoder_from_plan(
        &plan,
        &handle.import.descriptor,
        &handle.import.thunks,
    ) {
        Ok(stencil) => stencil,
        Err(_) => return BINETTE_STATUS_STENCIL,
    };
    let bytes = match unsafe { stencil.encode_raw_to_vec(value) } {
        Ok(bytes) => bytes,
        Err(_) => return BINETTE_STATUS_STENCIL,
    };
    *out = BinetteByteBuffer::from_vec(bytes);
    BINETTE_STATUS_OK
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.strict-hybrid]
#[unsafe(no_mangle)]
pub extern "C" fn binette_canary_message_decode(
    handle: *const BinetteLocalDescriptorHandle,
    bytes: *const u8,
    len: usize,
    out_value: *mut u8,
) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    if bytes.is_null() || out_value.is_null() {
        return BINETTE_STATUS_NULL_POINTER;
    }
    let writer_plan = match writer_plan_for::<CanaryMessage>() {
        Ok(plan) => plan,
        Err(_) => return BINETTE_STATUS_PLAN,
    };
    let mut registry = SchemaRegistry::new();
    if registry
        .install_bundle(writer_plan.schema_bundle())
        .is_err()
    {
        return BINETTE_STATUS_PLAN;
    }
    let reader_plan = match reader_plan_for_bundle(
        writer_plan.root(),
        &registry,
        writer_plan.root(),
        &registry,
    ) {
        Ok(plan) => plan,
        Err(_) => return BINETTE_STATUS_PLAN,
    };
    let stencil = match hybrid_local_stencil_decoder_from_plan(
        &reader_plan,
        &registry,
        &handle.import.descriptor,
        &handle.import.thunks,
    ) {
        Ok(stencil) => stencil,
        Err(_) => return BINETTE_STATUS_STENCIL,
    };
    let input = unsafe { std::slice::from_raw_parts(bytes, len) };
    match unsafe { stencil.decode_raw_into(input, out_value) } {
        Ok(()) => BINETTE_STATUS_OK,
        Err(_) => BINETTE_STATUS_STENCIL,
    }
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_canary_message_rust_encode_hi(out: *mut BinetteByteBuffer) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    *out = BinetteByteBuffer::empty();
    let value = CanaryMessage::Hi("hello from rust".to_owned());
    let _ = canary_message_payload_len(&value);
    match encode_to_vec(&value) {
        Ok(bytes) => {
            *out = BinetteByteBuffer::from_vec(bytes);
            BINETTE_STATUS_OK
        }
        Err(_) => BINETTE_STATUS_STENCIL,
    }
}

fn canary_message_payload_len(value: &CanaryMessage) -> usize {
    match value {
        CanaryMessage::Hi(text) => text.len(),
        CanaryMessage::Bye(code) => *code as usize,
    }
}

fn concrete_type_id(type_ref: &TypeRef) -> Option<crate::TypeId> {
    let TypeRef::Concrete { type_id, args } = type_ref else {
        return None;
    };
    args.is_empty().then_some(*type_id)
}
