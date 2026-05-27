use super::*;
use facet_core::{PtrMut, PtrUninit};

pub(super) const STENCIL_OK: u32 = 0;
pub(super) const HYBRID_ERROR_FLAG: usize = 1usize << (usize::BITS - 1);

pub(super) fn hybrid_error_status(value: usize) -> Option<u32> {
    if value & HYBRID_ERROR_FLAG == 0 {
        return None;
    }
    Some((value & !HYBRID_ERROR_FLAG) as u32)
}

fn hybrid_error_for_failure(index: usize) -> usize {
    match status_for_failure(index) {
        Ok(status) => HYBRID_ERROR_FLAG | status as usize,
        Err(_) => HYBRID_ERROR_FLAG,
    }
}

pub(super) unsafe extern "C" fn stencil_decode_helper(
    runtime: *const StencilRuntime,
    input: *const u8,
    len: usize,
    out: *mut u8,
    cursor: usize,
    helper_index: usize,
) -> usize {
    let Some(runtime) = (unsafe { runtime.as_ref() }) else {
        return HYBRID_ERROR_FLAG;
    };
    let Some(helper) = runtime.helpers.get(helper_index) else {
        return HYBRID_ERROR_FLAG;
    };
    if cursor > len {
        return hybrid_error_for_helper(helper);
    }

    let input = unsafe { slice::from_raw_parts(input, len) };
    let tail = &input[cursor..];
    let consumed = match helper {
        StencilHelper::Decode {
            plan,
            plan_nodes,
            reader_shape,
            output_offset,
            ..
        } => {
            let output = unsafe { out.add(*output_offset) };
            match unsafe {
                decode_plan_node_into_raw(
                    tail,
                    plan,
                    plan_nodes,
                    &runtime.writer_registry,
                    reader_shape,
                    output,
                )
            } {
                Ok(consumed) => consumed,
                Err(_) => return hybrid_error_for_helper(helper),
            }
        }
        StencilHelper::LocalSequenceBytes {
            output_offset,
            thunks,
            ..
        } => {
            if tail.len() < 4 {
                return hybrid_error_for_helper(helper);
            }
            let len = u32::from_le_bytes(tail[..4].try_into().unwrap()) as usize;
            let Some(end) = 4usize.checked_add(len) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(bytes) = tail.get(4..end) else {
                return hybrid_error_for_helper(helper);
            };
            let output = unsafe { out.add(*output_offset) };
            let context = thunks.context as *mut std::ffi::c_void;
            if !unsafe { (thunks.write_bytes)(output, bytes.as_ptr(), bytes.len(), context) } {
                return hybrid_error_for_helper(helper);
            }
            end
        }
        StencilHelper::LocalOptionSequenceBytes {
            output_offset,
            thunks,
            ..
        } => {
            let Some(tag) = tail.first().copied() else {
                return hybrid_error_for_helper(helper);
            };
            let output = unsafe { out.add(*output_offset) };
            let context = thunks.context as *mut std::ffi::c_void;
            match tag {
                STENCIL_OPTION_NONE_U8 => {
                    if !unsafe { (thunks.write_none)(output, context) } {
                        return hybrid_error_for_helper(helper);
                    }
                    1
                }
                STENCIL_OPTION_SOME_U8 => {
                    if tail.len() < 5 {
                        return hybrid_error_for_helper(helper);
                    }
                    let len = u32::from_le_bytes(tail[1..5].try_into().unwrap()) as usize;
                    let Some(end) = 5usize.checked_add(len) else {
                        return hybrid_error_for_helper(helper);
                    };
                    let Some(bytes) = tail.get(5..end) else {
                        return hybrid_error_for_helper(helper);
                    };
                    if !unsafe {
                        (thunks.write_some_bytes)(output, bytes.as_ptr(), bytes.len(), context)
                    } {
                        return hybrid_error_for_helper(helper);
                    }
                    end
                }
                _ => return hybrid_error_for_helper(helper),
            }
        }
        StencilHelper::Skip { writer_type, .. } => {
            let mut reader = CompactReader::new(tail);
            if reader
                .skip_value(writer_type, &runtime.writer_registry)
                .is_err()
            {
                return hybrid_error_for_helper(helper);
            }
            reader.position()
        }
    };

    cursor
        .checked_add(consumed)
        .unwrap_or_else(|| hybrid_error_for_helper(helper))
}

fn hybrid_error_for_helper(helper: &StencilHelper) -> usize {
    match helper {
        StencilHelper::Decode { failure_index, .. }
        | StencilHelper::LocalSequenceBytes { failure_index, .. }
        | StencilHelper::LocalOptionSequenceBytes { failure_index, .. }
        | StencilHelper::Skip { failure_index, .. } => hybrid_error_for_failure(*failure_index),
    }
}

pub(super) unsafe extern "C" fn stencil_encode_helper(
    runtime: *const StencilEncodeRuntime,
    value: *const u8,
    out: *mut Vec<u8>,
    helper_index: usize,
) -> u32 {
    let Some(runtime) = (unsafe { runtime.as_ref() }) else {
        return 1;
    };
    let Some(out) = (unsafe { out.as_mut() }) else {
        return 1;
    };
    let Some(helper) = runtime.helpers.get(helper_index) else {
        return 1;
    };

    let status = match helper {
        StencilEncodeHelper::Node { failure_index, .. }
        | StencilEncodeHelper::LocalSequenceBytes { failure_index, .. }
        | StencilEncodeHelper::LocalSequenceFixedElements { failure_index, .. }
        | StencilEncodeHelper::LocalOptionSequenceBytes { failure_index, .. } => {
            status_for_failure(*failure_index).unwrap_or(1)
        }
    };

    match helper {
        StencilEncodeHelper::Node {
            node,
            shape,
            input_offset,
            ..
        } => {
            if value.is_null() {
                return status;
            }
            let value = value.wrapping_add(*input_offset);
            let peek: Peek<'_, 'static> =
                unsafe { Peek::unchecked_new(PtrConst::new(value), shape) };
            if encode_node_with_writer_node(out, peek, node, &runtime.nodes).is_err() {
                return status;
            }
        }
        StencilEncodeHelper::LocalSequenceBytes {
            input_offset,
            thunks,
            ..
        } => {
            if value.is_null() {
                return status;
            }
            let value = value.wrapping_add(*input_offset);
            let context = thunks.context as *mut std::ffi::c_void;
            let len = unsafe { (thunks.len)(value, context) };
            let Ok(len_u32) = u32::try_from(len) else {
                return status;
            };
            out.extend_from_slice(&len_u32.to_le_bytes());
            out.reserve(len);
            for index in 0..len {
                out.push(unsafe { (thunks.element_u8)(value, index, context) });
            }
        }
        StencilEncodeHelper::LocalSequenceFixedElements {
            input_offset,
            thunks,
            element_ops,
            element_output_len,
            ..
        } => {
            if value.is_null() {
                return status;
            }
            let value = value.wrapping_add(*input_offset);
            let context = thunks.context as *mut std::ffi::c_void;
            let len = unsafe { (thunks.len)(value, context) };
            let Ok(len_u32) = u32::try_from(len) else {
                return status;
            };
            let Some(elements_len) = element_output_len.checked_mul(len) else {
                return status;
            };
            out.extend_from_slice(&len_u32.to_le_bytes());
            out.reserve(elements_len);
            for index in 0..len {
                let element = unsafe { (thunks.element_ptr)(value, index, context) };
                if element.is_null() {
                    return status;
                }
                let element_base = out.len();
                out.resize(element_base + element_output_len, 0);
                for op in element_ops {
                    let source = unsafe { element.add(op.input_offset) };
                    unsafe {
                        copy_nonoverlapping(
                            source,
                            out.as_mut_ptr().add(element_base + op.output_offset),
                            op.width.bytes(),
                        );
                    }
                }
            }
        }
        StencilEncodeHelper::LocalOptionSequenceBytes {
            input_offset,
            option_thunks,
            sequence_thunks,
            ..
        } => {
            if value.is_null() {
                return status;
            }
            let value = value.wrapping_add(*input_offset);
            let option_context = option_thunks.context as *mut std::ffi::c_void;
            if !unsafe { (option_thunks.is_some)(value, option_context) } {
                out.push(STENCIL_OPTION_NONE_U8);
                return STENCIL_OK;
            }
            let some = unsafe { (option_thunks.some)(value, option_context) };
            if some.is_null() {
                return status;
            }
            let sequence_context = sequence_thunks.context as *mut std::ffi::c_void;
            let len = unsafe { (sequence_thunks.len)(some, sequence_context) };
            let Ok(len_u32) = u32::try_from(len) else {
                return status;
            };
            out.push(STENCIL_OPTION_SOME_U8);
            out.extend_from_slice(&len_u32.to_le_bytes());
            out.reserve(len);
            for index in 0..len {
                out.push(unsafe { (sequence_thunks.element_u8)(some, index, sequence_context) });
            }
        }
    }

    STENCIL_OK
}

#[repr(C)]
pub(super) struct StencilByteParts {
    ptr: *const u8,
    len: usize,
}

pub(super) const STENCIL_ENCODE_BYTES_STRING: usize = 1;
pub(super) const STENCIL_ENCODE_BYTES_BYTES: usize = 2;

pub(super) unsafe extern "C" fn stencil_encode_byte_parts(
    value: *const u8,
    shape: *const Shape,
    kind: usize,
) -> StencilByteParts {
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return StencilByteParts {
            ptr: std::ptr::null(),
            len: 0,
        };
    };
    let peek: Peek<'_, 'static> = unsafe { Peek::unchecked_new(PtrConst::new(value), shape) };
    let bytes = match kind {
        STENCIL_ENCODE_BYTES_STRING => {
            let Some(value) = peek.as_str() else {
                return StencilByteParts {
                    ptr: std::ptr::null(),
                    len: 0,
                };
            };
            value.as_bytes()
        }
        STENCIL_ENCODE_BYTES_BYTES => {
            if let Some(value) = peek.as_bytes() {
                value
            } else {
                let Ok(list) = peek.into_list_like() else {
                    return StencilByteParts {
                        ptr: std::ptr::null(),
                        len: 0,
                    };
                };
                let Some(value) = list.as_bytes() else {
                    return StencilByteParts {
                        ptr: std::ptr::null(),
                        len: 0,
                    };
                };
                value
            }
        }
        _ => {
            return StencilByteParts {
                ptr: std::ptr::null(),
                len: 0,
            };
        }
    };

    if bytes.len() > u32::MAX as usize {
        return StencilByteParts {
            ptr: std::ptr::null(),
            len: 0,
        };
    }

    StencilByteParts {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

pub(super) unsafe extern "C" fn stencil_copy_bytes(dst: *mut u8, src: *const u8, len: usize) {
    unsafe {
        copy_nonoverlapping(src, dst, len);
    }
}

pub(super) const STENCIL_ENUM_VARIANT_ERROR: usize = usize::MAX;

pub(super) unsafe extern "C" fn stencil_enum_variant_index(
    value: *const u8,
    shape: *const Shape,
) -> usize {
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return STENCIL_ENUM_VARIANT_ERROR;
    };
    let peek: Peek<'_, 'static> = unsafe { Peek::unchecked_new(PtrConst::new(value), shape) };
    let Ok(enum_peek) = peek.into_enum() else {
        return STENCIL_ENUM_VARIANT_ERROR;
    };
    enum_peek
        .variant_index()
        .unwrap_or(STENCIL_ENUM_VARIANT_ERROR)
}

#[repr(C)]
pub(super) struct StencilOptionParts {
    tag: usize,
    ptr: *const u8,
}

pub(super) const STENCIL_OPTION_NONE: usize = 0;
pub(super) const STENCIL_OPTION_SOME: usize = 1;
pub(super) const STENCIL_OPTION_ERROR: usize = usize::MAX;
pub(super) const STENCIL_OPTION_NONE_U8: u8 = 0;
pub(super) const STENCIL_OPTION_SOME_U8: u8 = 1;

pub(super) unsafe extern "C" fn stencil_option_parts(
    value: *const u8,
    shape: *const Shape,
) -> StencilOptionParts {
    let error = StencilOptionParts {
        tag: STENCIL_OPTION_ERROR,
        ptr: std::ptr::null(),
    };
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return error;
    };
    let peek: Peek<'_, 'static> = unsafe { Peek::unchecked_new(PtrConst::new(value), shape) };
    let Ok(option) = peek.into_option() else {
        return error;
    };
    match option.value() {
        Some(inner) => StencilOptionParts {
            tag: STENCIL_OPTION_SOME,
            ptr: inner.data().raw_ptr(),
        },
        None => StencilOptionParts {
            tag: STENCIL_OPTION_NONE,
            ptr: std::ptr::null(),
        },
    }
}

pub(super) const STENCIL_LIST_ERROR: usize = usize::MAX;

pub(super) unsafe extern "C" fn stencil_list_len(value: *const u8, shape: *const Shape) -> usize {
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return STENCIL_LIST_ERROR;
    };
    let peek: Peek<'_, 'static> = unsafe { Peek::unchecked_new(PtrConst::new(value), shape) };
    let Ok(list) = peek.into_list_like() else {
        return STENCIL_LIST_ERROR;
    };
    list.len()
}

pub(super) unsafe extern "C" fn stencil_list_element(
    value: *const u8,
    shape: *const Shape,
    index: usize,
) -> *const u8 {
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return std::ptr::null();
    };
    let peek: Peek<'_, 'static> = unsafe { Peek::unchecked_new(PtrConst::new(value), shape) };
    let Ok(list) = peek.into_list_like() else {
        return std::ptr::null();
    };
    let Some(element) = list.get(index) else {
        return std::ptr::null();
    };
    element.data().raw_ptr()
}

pub(super) unsafe extern "C" fn stencil_encode_reserve(out: *mut Vec<u8>, len: usize) -> *mut u8 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return std::ptr::null_mut();
    };
    let start = out.len();
    let Some(end) = start.checked_add(len) else {
        return std::ptr::null_mut();
    };
    if out.try_reserve(len).is_err() {
        return std::ptr::null_mut();
    }
    let ptr = unsafe { out.as_mut_ptr().add(start) };
    unsafe {
        out.set_len(end);
    }
    ptr
}

pub(super) unsafe extern "C" fn stencil_decode_list_begin(
    value: *mut u8,
    shape: *const Shape,
    count: usize,
) -> *mut u8 {
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let Def::List(list) = shape.def else {
        return std::ptr::null_mut();
    };
    let Some(init) = list.init_in_place_with_capacity() else {
        return std::ptr::null_mut();
    };
    let Some(as_mut_ptr) = list.as_mut_ptr_typed() else {
        return std::ptr::null_mut();
    };

    let list_ptr = unsafe { init(PtrUninit::new(value), count) };
    unsafe { as_mut_ptr(list_ptr) }
}

pub(super) unsafe extern "C" fn stencil_decode_list_finish(
    value: *mut u8,
    shape: *const Shape,
    count: usize,
) -> u32 {
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return 1;
    };
    let Def::List(list) = shape.def else {
        return 1;
    };
    let Some(set_len) = list.set_len() else {
        return 1;
    };

    unsafe {
        set_len(PtrMut::new(value), count);
    }
    STENCIL_OK
}
