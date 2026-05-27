use facet::Facet;

use crate::local_access::{
    BinetteLocalDescriptorAbi, LocalDescriptorAbiImport, LocalScalarAccess, LocalSchemaRef,
    LocalTypeDescriptor, LocalTypeKind,
};
use crate::{
    Field, Primitive, Schema, SchemaBundle, SchemaKind, SchemaRegistry, TypeId, TypeRef, Value,
    Variant, VariantPayload, decode_schema_bundle_from_slice, encode_schema_bundle_to_vec,
    encode_to_vec, hybrid_local_stencil_decoder_from_plan, hybrid_local_stencil_encoder_from_plan,
    primitive_for_type_id, primitive_type_id, reader_plan_for_bundle, reader_plan_for_bundles,
    schema_type_id, writer_plan_for, writer_plan_for_bundle,
};

pub const BINETTE_STATUS_OK: i32 = 0;
pub const BINETTE_STATUS_NULL_POINTER: i32 = 1;
pub const BINETTE_STATUS_DESCRIPTOR: i32 = 2;
pub const BINETTE_STATUS_PLAN: i32 = 3;
pub const BINETTE_STATUS_STENCIL: i32 = 4;
pub const BINETTE_STATUS_SCHEMA: i32 = 5;

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
pub extern "C" fn binette_primitive_bool_type_id() -> u64 {
    primitive_type_id(Primitive::Bool).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_u8_type_id() -> u64 {
    primitive_type_id(Primitive::U8).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_u16_type_id() -> u64 {
    primitive_type_id(Primitive::U16).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_u32_type_id() -> u64 {
    primitive_type_id(Primitive::U32).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_i32_type_id() -> u64 {
    primitive_type_id(Primitive::I32).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_i64_type_id() -> u64 {
    primitive_type_id(Primitive::I64).0
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_primitive_bytes_type_id() -> u64 {
    primitive_type_id(Primitive::Bytes).0
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
#[unsafe(no_mangle)]
pub extern "C" fn binette_canary_message_schema_bundle(out: *mut BinetteByteBuffer) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    *out = BinetteByteBuffer::empty();
    let plan = match writer_plan_for::<CanaryMessage>() {
        Ok(plan) => plan,
        Err(_) => return BINETTE_STATUS_PLAN,
    };
    match encode_schema_bundle_to_vec(plan.schema_bundle()) {
        Ok(bytes) => {
            *out = BinetteByteBuffer::from_vec(bytes);
            BINETTE_STATUS_OK
        }
        Err(_) => BINETTE_STATUS_SCHEMA,
    }
}

// r[impl binette.local-access.boundary]
#[unsafe(no_mangle)]
pub extern "C" fn binette_local_descriptor_synthetic_schema_bundle(
    handle: *mut BinetteLocalDescriptorHandle,
    out: *mut BinetteByteBuffer,
) -> i32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    let Some(out) = (unsafe { out.as_mut() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    *out = BinetteByteBuffer::empty();
    let bundle = match synthetic_schema_bundle_for_descriptor(&mut handle.import.descriptor) {
        Ok(bundle) => bundle,
        Err(()) => return BINETTE_STATUS_SCHEMA,
    };
    match encode_schema_bundle_to_vec(&bundle) {
        Ok(bytes) => {
            *out = BinetteByteBuffer::from_vec(bytes);
            BINETTE_STATUS_OK
        }
        Err(_) => BINETTE_STATUS_SCHEMA,
    }
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.strict-hybrid]
#[unsafe(no_mangle)]
pub extern "C" fn binette_local_encode_with_schema_bundle(
    handle: *const BinetteLocalDescriptorHandle,
    schema_bundle: *const u8,
    schema_bundle_len: usize,
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

    let schema_bundle = match ffi_slice(schema_bundle, schema_bundle_len) {
        Some(bytes) => match decode_schema_bundle_from_slice(bytes) {
            Ok(bundle) => bundle,
            Err(_) => return BINETTE_STATUS_SCHEMA,
        },
        None => return BINETTE_STATUS_NULL_POINTER,
    };
    let plan = match writer_plan_for_bundle(&schema_bundle) {
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
pub extern "C" fn binette_local_decode_with_schema_bundles(
    handle: *const BinetteLocalDescriptorHandle,
    writer_schema_bundle: *const u8,
    writer_schema_bundle_len: usize,
    reader_schema_bundle: *const u8,
    reader_schema_bundle_len: usize,
    bytes: *const u8,
    len: usize,
    out_value: *mut u8,
) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    if out_value.is_null() {
        return BINETTE_STATUS_NULL_POINTER;
    }
    let Some(input) = ffi_slice(bytes, len) else {
        return BINETTE_STATUS_NULL_POINTER;
    };
    let writer_bundle = match ffi_slice(writer_schema_bundle, writer_schema_bundle_len) {
        Some(bytes) => match decode_schema_bundle_from_slice(bytes) {
            Ok(bundle) => bundle,
            Err(_) => return BINETTE_STATUS_SCHEMA,
        },
        None => return BINETTE_STATUS_NULL_POINTER,
    };
    let reader_bundle = match ffi_slice(reader_schema_bundle, reader_schema_bundle_len) {
        Some(bytes) => match decode_schema_bundle_from_slice(bytes) {
            Ok(bundle) => bundle,
            Err(_) => return BINETTE_STATUS_SCHEMA,
        },
        None => return BINETTE_STATUS_NULL_POINTER,
    };
    let plan = match reader_plan_for_bundles(&writer_bundle, &reader_bundle) {
        Ok(plan) => plan,
        Err(_) => return BINETTE_STATUS_PLAN,
    };
    let mut writer_registry = SchemaRegistry::new();
    if writer_registry.install_bundle(&writer_bundle).is_err() {
        return BINETTE_STATUS_SCHEMA;
    }
    let stencil = match hybrid_local_stencil_decoder_from_plan(
        &plan,
        &writer_registry,
        &handle.import.descriptor,
        &handle.import.thunks,
    ) {
        Ok(stencil) => stencil,
        Err(_) => return BINETTE_STATUS_STENCIL,
    };
    match unsafe { stencil.decode_raw_into(input, out_value) } {
        Ok(()) => BINETTE_STATUS_OK,
        Err(_) => BINETTE_STATUS_STENCIL,
    }
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

fn ffi_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn synthetic_schema_bundle_for_descriptor(
    descriptor: &mut LocalTypeDescriptor,
) -> Result<SchemaBundle, ()> {
    let mut schemas = Vec::new();
    let root = canonicalize_descriptor_schema(descriptor, &mut schemas)?;
    Ok(SchemaBundle {
        schemas,
        root,
        attachments: Vec::new(),
    })
}

fn canonicalize_descriptor_schema(
    descriptor: &mut LocalTypeDescriptor,
    schemas: &mut Vec<Schema>,
) -> Result<TypeRef, ()> {
    let type_ref = match &mut descriptor.kind {
        LocalTypeKind::Scalar(LocalScalarAccess::Plain) => {
            let type_ref = descriptor_type_ref(descriptor)?;
            let TypeRef::Concrete { type_id, args } = &type_ref else {
                return Err(());
            };
            if !args.is_empty() || primitive_for_type_id(*type_id).is_none() {
                return Err(());
            }
            type_ref
        }
        LocalTypeKind::Scalar(LocalScalarAccess::String(_)) => {
            TypeRef::concrete(primitive_type_id(Primitive::String))
        }
        LocalTypeKind::Scalar(LocalScalarAccess::Bytes(_)) => {
            TypeRef::concrete(primitive_type_id(Primitive::Bytes))
        }
        LocalTypeKind::Struct { fields } => {
            let fields = fields
                .iter_mut()
                .map(|field| {
                    Ok(Field {
                        name: field.name.clone(),
                        type_ref: canonicalize_descriptor_schema(&mut field.descriptor, schemas)?,
                        required: true,
                    })
                })
                .collect::<Result<Vec<_>, ()>>()?;
            push_synthetic_schema(
                schemas,
                SchemaKind::Struct {
                    name: "local.struct".to_owned(),
                    fields,
                },
            )?
        }
        LocalTypeKind::Enum { variants, .. } => {
            let variants = variants
                .iter_mut()
                .map(|variant| {
                    let payload = match &mut variant.payload {
                        Some(payload) => VariantPayload::Newtype {
                            type_ref: canonicalize_descriptor_schema(payload, schemas)?,
                        },
                        None => VariantPayload::Unit,
                    };
                    Ok(Variant {
                        name: variant.name.clone(),
                        index: variant.index,
                        payload,
                    })
                })
                .collect::<Result<Vec<_>, ()>>()?;
            push_synthetic_schema(
                schemas,
                SchemaKind::Enum {
                    name: "local.enum".to_owned(),
                    variants,
                },
            )?
        }
        LocalTypeKind::Sequence { element, .. } => {
            let element = canonicalize_descriptor_schema(element, schemas)?;
            push_synthetic_schema(schemas, SchemaKind::List { element })?
        }
        LocalTypeKind::Option { some, .. } => {
            let element = canonicalize_descriptor_schema(some, schemas)?;
            push_synthetic_schema(schemas, SchemaKind::Option { element })?
        }
        LocalTypeKind::ExternalAttachment { kind } => push_synthetic_schema(
            schemas,
            SchemaKind::External {
                kind: kind.clone(),
                metadata: Value::Unit,
            },
        )?,
        LocalTypeKind::Opaque { .. } => return Err(()),
    };
    descriptor.schema = LocalSchemaRef::Type(type_ref.clone());
    Ok(type_ref)
}

fn descriptor_type_ref(descriptor: &LocalTypeDescriptor) -> Result<TypeRef, ()> {
    match &descriptor.schema {
        LocalSchemaRef::Type(type_ref) => Ok(type_ref.clone()),
        LocalSchemaRef::Position { .. } => Err(()),
    }
}

fn push_synthetic_schema(schemas: &mut Vec<Schema>, kind: SchemaKind) -> Result<TypeRef, ()> {
    let schema = schema_with_canonical_id(kind)?;
    let type_ref = TypeRef::concrete(schema.id);
    if !schemas.iter().any(|existing| existing.id == schema.id) {
        schemas.push(schema);
    }
    Ok(type_ref)
}

fn schema_with_canonical_id(kind: SchemaKind) -> Result<Schema, ()> {
    let mut schema = Schema {
        id: TypeId(0),
        type_params: Vec::new(),
        kind,
    };
    schema.id = schema_type_id(&schema).map_err(|_| ())?;
    Ok(schema)
}
