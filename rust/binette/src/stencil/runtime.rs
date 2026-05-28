use super::*;
use crate::compact::CompactReader;

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
        StencilHelper::SequenceBytes {
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
        StencilHelper::SequenceFixedElements {
            output_offset,
            thunks,
            element_ops,
            element_input_len,
            element_stride,
            ..
        } => {
            if tail.len() < 4 {
                return hybrid_error_for_helper(helper);
            }
            let count = u32::from_le_bytes(tail[..4].try_into().unwrap()) as usize;
            let Some(elements_input_len) = element_input_len.checked_mul(count) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(end) = 4usize.checked_add(elements_input_len) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(input_elements) = tail.get(4..end) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(output_len) = element_stride.checked_mul(count) else {
                return hybrid_error_for_helper(helper);
            };
            let mut elements = vec![0u8; output_len];
            for index in 0..count {
                let input_base = index * element_input_len;
                let output_base = index * element_stride;
                let Some(input_element) =
                    input_elements.get(input_base..input_base + element_input_len)
                else {
                    return hybrid_error_for_helper(helper);
                };
                let Some(output_element) =
                    elements.get_mut(output_base..output_base + element_stride)
                else {
                    return hybrid_error_for_helper(helper);
                };
                if !run_fixed_decode_ops(element_ops, input_element, output_element) {
                    return hybrid_error_for_helper(helper);
                }
            }
            let output = unsafe { out.add(*output_offset) };
            let context = thunks.context as *mut std::ffi::c_void;
            if !unsafe {
                (thunks.write_elements)(output, elements.as_ptr(), count, *element_stride, context)
            } {
                return hybrid_error_for_helper(helper);
            }
            end
        }
        StencilHelper::SequenceElements {
            output_offset,
            thunks,
            element_decoder,
            element_stride,
            ..
        } => {
            if tail.len() < 4 {
                return hybrid_error_for_helper(helper);
            }
            let count = u32::from_le_bytes(tail[..4].try_into().unwrap()) as usize;
            let Some(output_len) = element_stride.checked_mul(count) else {
                return hybrid_error_for_helper(helper);
            };
            let mut elements = vec![0u8; output_len];
            let mut input_cursor = 4usize;
            for index in 0..count {
                let output_base = index * element_stride;
                let Some(output_element) =
                    elements.get_mut(output_base..output_base + element_stride)
                else {
                    return hybrid_error_for_helper(helper);
                };
                let Some(input_element) = tail.get(input_cursor..) else {
                    return hybrid_error_for_helper(helper);
                };
                let consumed = match unsafe {
                    element_decoder
                        .decode_raw_prefix_into(input_element, output_element.as_mut_ptr())
                } {
                    Ok(consumed) => consumed,
                    Err(_) => return hybrid_error_for_helper(helper),
                };
                let Some(next_cursor) = input_cursor.checked_add(consumed) else {
                    return hybrid_error_for_helper(helper);
                };
                input_cursor = next_cursor;
            }
            let output = unsafe { out.add(*output_offset) };
            let context = thunks.context as *mut std::ffi::c_void;
            if !unsafe {
                (thunks.write_elements)(output, elements.as_ptr(), count, *element_stride, context)
            } {
                return hybrid_error_for_helper(helper);
            }
            input_cursor
        }
        StencilHelper::DirectSequenceBytes {
            output_offset,
            layout,
            primitive,
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
            if *primitive == Primitive::String && std::str::from_utf8(bytes).is_err() {
                return hybrid_error_for_helper(helper);
            }
            let output = unsafe { out.add(*output_offset) };
            if !unsafe { write_direct_sequence(output, *layout, bytes, len) } {
                return hybrid_error_for_helper(helper);
            }
            end
        }
        StencilHelper::DirectSequenceFixedElements {
            output_offset,
            layout,
            element_ops,
            element_input_len,
            ..
        } => {
            if tail.len() < 4 {
                return hybrid_error_for_helper(helper);
            }
            let count = u32::from_le_bytes(tail[..4].try_into().unwrap()) as usize;
            let Some(elements_input_len) = element_input_len.checked_mul(count) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(end) = 4usize.checked_add(elements_input_len) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(input_elements) = tail.get(4..end) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(output_len) = layout.element_stride.checked_mul(count) else {
                return hybrid_error_for_helper(helper);
            };
            let mut elements = vec![0u8; output_len];
            for index in 0..count {
                let input_base = index * element_input_len;
                let output_base = index * layout.element_stride;
                let Some(input_element) =
                    input_elements.get(input_base..input_base + element_input_len)
                else {
                    return hybrid_error_for_helper(helper);
                };
                let Some(output_element) =
                    elements.get_mut(output_base..output_base + layout.element_stride)
                else {
                    return hybrid_error_for_helper(helper);
                };
                if !run_fixed_decode_ops(element_ops, input_element, output_element) {
                    return hybrid_error_for_helper(helper);
                }
            }
            let output = unsafe { out.add(*output_offset) };
            if !unsafe { write_direct_sequence(output, *layout, &elements, count) } {
                return hybrid_error_for_helper(helper);
            }
            end
        }
        StencilHelper::DirectSequenceElements {
            output_offset,
            layout,
            element_decoder,
            ..
        } => {
            if tail.len() < 4 {
                return hybrid_error_for_helper(helper);
            }
            let count = u32::from_le_bytes(tail[..4].try_into().unwrap()) as usize;
            let Some(output_len) = layout.element_stride.checked_mul(count) else {
                return hybrid_error_for_helper(helper);
            };
            let mut elements = vec![0u8; output_len];
            let mut input_cursor = 4usize;
            for index in 0..count {
                let output_base = index * layout.element_stride;
                let Some(output_element) =
                    elements.get_mut(output_base..output_base + layout.element_stride)
                else {
                    return hybrid_error_for_helper(helper);
                };
                let Some(input_element) = tail.get(input_cursor..) else {
                    return hybrid_error_for_helper(helper);
                };
                let consumed = match unsafe {
                    element_decoder
                        .decode_raw_prefix_into(input_element, output_element.as_mut_ptr())
                } {
                    Ok(consumed) => consumed,
                    Err(_) => return hybrid_error_for_helper(helper),
                };
                let Some(next_cursor) = input_cursor.checked_add(consumed) else {
                    return hybrid_error_for_helper(helper);
                };
                input_cursor = next_cursor;
            }
            let output = unsafe { out.add(*output_offset) };
            if !unsafe { write_direct_sequence(output, *layout, &elements, count) } {
                return hybrid_error_for_helper(helper);
            }
            input_cursor
        }
        StencilHelper::DirectOptionSequenceBytes {
            output_offset,
            option,
            sequence,
            primitive,
            ..
        } => decode_direct_option_sequence_bytes(
            helper,
            tail,
            unsafe { out.add(*output_offset) },
            option,
            *sequence,
            *primitive,
        ),
        StencilHelper::DirectOptionFixed {
            output_offset,
            option,
            element_ops,
            element_input_len,
            element_output_len,
            ..
        } => decode_direct_option_fixed(
            helper,
            tail,
            unsafe { out.add(*output_offset) },
            option,
            element_ops,
            *element_input_len,
            *element_output_len,
        ),
        StencilHelper::OptionSequenceBytes {
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
        StencilHelper::Enum {
            output_offset,
            cases,
            ..
        } => {
            if tail.len() < 4 {
                return hybrid_error_for_helper(helper);
            }
            let wire_index = u32::from_le_bytes(tail[..4].try_into().unwrap());
            let Some(case) = cases.iter().find(|case| case.wire_index == wire_index) else {
                return hybrid_error_for_helper(helper);
            };
            let output = unsafe { out.add(*output_offset) };
            let context = case.construct_thunks.context as *mut std::ffi::c_void;
            match &case.payload {
                LocalEnumDecodePayload::Unit => {
                    if !unsafe {
                        (case.construct_thunks.construct)(output, std::ptr::null(), 0, context)
                    } {
                        return hybrid_error_for_helper(helper);
                    }
                    4
                }
                LocalEnumDecodePayload::Fixed {
                    ops,
                    input_len,
                    payload_layout,
                } => {
                    let Some(end) = 4usize.checked_add(*input_len) else {
                        return hybrid_error_for_helper(helper);
                    };
                    let Some(input_payload) = tail.get(4..end) else {
                        return hybrid_error_for_helper(helper);
                    };
                    let constructed = unsafe {
                        with_aligned_scratch(*payload_layout, |payload| {
                            let payload_bytes =
                                slice::from_raw_parts_mut(payload, payload_layout.size);
                            if !run_fixed_decode_ops(ops, input_payload, payload_bytes) {
                                return None;
                            }
                            (case.construct_thunks.construct)(
                                output,
                                payload.cast_const(),
                                payload_layout.size,
                                context,
                            )
                            .then_some(())
                        })
                    };
                    if constructed.is_none() {
                        return hybrid_error_for_helper(helper);
                    }
                    end
                }
                LocalEnumDecodePayload::Nested {
                    decoder,
                    payload_layout,
                } => {
                    let input_payload = &tail[4..];
                    let Some(consumed) = (unsafe {
                        with_aligned_scratch(*payload_layout, |payload| {
                            let consumed = decoder
                                .decode_raw_prefix_into(input_payload, payload)
                                .ok()?;
                            (case.construct_thunks.construct)(
                                output,
                                payload.cast_const(),
                                payload_layout.size,
                                context,
                            )
                            .then_some(consumed)
                        })
                    }) else {
                        return hybrid_error_for_helper(helper);
                    };
                    let Some(end) = 4usize.checked_add(consumed) else {
                        return hybrid_error_for_helper(helper);
                    };
                    end
                }
                LocalEnumDecodePayload::SequenceBytes => {
                    if tail.len() < 8 {
                        return hybrid_error_for_helper(helper);
                    }
                    let len = u32::from_le_bytes(tail[4..8].try_into().unwrap()) as usize;
                    let Some(end) = 8usize.checked_add(len) else {
                        return hybrid_error_for_helper(helper);
                    };
                    let Some(bytes) = tail.get(8..end) else {
                        return hybrid_error_for_helper(helper);
                    };
                    if !unsafe {
                        (case.construct_thunks.construct)(
                            output,
                            bytes.as_ptr(),
                            bytes.len(),
                            context,
                        )
                    } {
                        return hybrid_error_for_helper(helper);
                    }
                    end
                }
            }
        }
        StencilHelper::DirectEnum {
            output_offset,
            tag_output_offset,
            cases,
            ..
        } => {
            if tail.len() < 4 {
                return hybrid_error_for_helper(helper);
            }
            let wire_index = u32::from_le_bytes(tail[..4].try_into().unwrap());
            let Some(case) = cases.iter().find(|case| case.wire_index == wire_index) else {
                return hybrid_error_for_helper(helper);
            };
            let output = unsafe { out.add(*output_offset) };
            unsafe { std::ptr::write(output.add(*tag_output_offset), case.reader_discriminant) };
            let payload_consumed = match &case.payload_decoder {
                Some(decoder) => {
                    match unsafe { decoder.decode_raw_prefix_into(&tail[4..], output) } {
                        Ok(consumed) => consumed,
                        Err(_) => return hybrid_error_for_helper(helper),
                    }
                }
                None => 0,
            };
            let Some(end) = 4usize.checked_add(payload_consumed) else {
                return hybrid_error_for_helper(helper);
            };
            end
        }
        StencilHelper::SubtreeDecode {
            output_offset,
            thunks,
            root,
            ..
        } => {
            let output = unsafe { out.add(*output_offset) };
            let context = thunks.context as *mut std::ffi::c_void;
            let consumed = unsafe {
                (thunks.decode)(
                    tail.as_ptr(),
                    tail.len(),
                    output,
                    root as *const _,
                    runtime.plan_nodes.as_ptr(),
                    runtime.plan_nodes.len(),
                    &runtime.writer_registry as *const _,
                    context,
                )
            };
            if consumed == usize::MAX || consumed > tail.len() {
                return hybrid_error_for_helper(helper);
            }
            consumed
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
        StencilHelper::SequenceBytes { failure_index, .. }
        | StencilHelper::SequenceFixedElements { failure_index, .. }
        | StencilHelper::SequenceElements { failure_index, .. }
        | StencilHelper::DirectSequenceBytes { failure_index, .. }
        | StencilHelper::DirectSequenceFixedElements { failure_index, .. }
        | StencilHelper::DirectSequenceElements { failure_index, .. }
        | StencilHelper::DirectOptionSequenceBytes { failure_index, .. }
        | StencilHelper::DirectOptionFixed { failure_index, .. }
        | StencilHelper::OptionSequenceBytes { failure_index, .. }
        | StencilHelper::Enum { failure_index, .. }
        | StencilHelper::DirectEnum { failure_index, .. }
        | StencilHelper::SubtreeDecode { failure_index, .. }
        | StencilHelper::Skip { failure_index, .. } => hybrid_error_for_failure(*failure_index),
    }
}

fn decode_direct_option_sequence_bytes(
    helper: &StencilHelper,
    tail: &[u8],
    output: *mut u8,
    option: &DirectOptionDecodeLayout,
    sequence: DirectSequenceDecodeLayout,
    primitive: Primitive,
) -> usize {
    let Some(tag) = tail.first().copied() else {
        return hybrid_error_for_helper(helper);
    };
    match tag {
        STENCIL_OPTION_NONE_U8 => {
            if !unsafe { write_direct_option_none(output, option) } {
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
            if primitive == Primitive::String && std::str::from_utf8(bytes).is_err() {
                return hybrid_error_for_helper(helper);
            }
            let some_output = unsafe { output.add(option.some_offset) };
            if !unsafe { write_direct_sequence(some_output, sequence, bytes, len) } {
                return hybrid_error_for_helper(helper);
            }
            if let Some(some_value) = option.some_value
                && !unsafe {
                    write_direct_option_tag(output, option.tag_offset, option.tag_width, some_value)
                }
            {
                return hybrid_error_for_helper(helper);
            }
            end
        }
        _ => hybrid_error_for_helper(helper),
    }
}

fn decode_direct_option_fixed(
    helper: &StencilHelper,
    tail: &[u8],
    output: *mut u8,
    option: &DirectOptionDecodeLayout,
    element_ops: &[StencilOp],
    element_input_len: usize,
    element_output_len: usize,
) -> usize {
    let Some(tag) = tail.first().copied() else {
        return hybrid_error_for_helper(helper);
    };
    match tag {
        STENCIL_OPTION_NONE_U8 => {
            if !unsafe { write_direct_option_none(output, option) } {
                return hybrid_error_for_helper(helper);
            }
            1
        }
        STENCIL_OPTION_SOME_U8 => {
            let Some(end) = 1usize.checked_add(element_input_len) else {
                return hybrid_error_for_helper(helper);
            };
            let Some(input_element) = tail.get(1..end) else {
                return hybrid_error_for_helper(helper);
            };
            let some_output = unsafe { output.add(option.some_offset) };
            let output_element =
                unsafe { std::slice::from_raw_parts_mut(some_output, element_output_len) };
            if !run_fixed_decode_ops(element_ops, input_element, output_element) {
                return hybrid_error_for_helper(helper);
            }
            if let Some(some_value) = option.some_value
                && !unsafe {
                    write_direct_option_tag(output, option.tag_offset, option.tag_width, some_value)
                }
            {
                return hybrid_error_for_helper(helper);
            }
            end
        }
        _ => hybrid_error_for_helper(helper),
    }
}

unsafe fn write_direct_option_none(output: *mut u8, layout: &DirectOptionDecodeLayout) -> bool {
    if let Some(bytes) = &layout.none_bytes {
        if bytes.len() != layout.option_size {
            return false;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    }
    unsafe {
        write_direct_option_tag(
            output,
            layout.tag_offset,
            layout.tag_width,
            layout.none_value,
        )
    }
}

unsafe fn write_direct_option_tag(
    output: *mut u8,
    tag_offset: usize,
    tag_width: usize,
    value: usize,
) -> bool {
    let tag_output = unsafe { output.add(tag_offset) };
    match tag_width {
        1 => {
            let Ok(value) = u8::try_from(value) else {
                return false;
            };
            unsafe { std::ptr::write(tag_output, value) };
        }
        2 => {
            let Ok(value) = u16::try_from(value) else {
                return false;
            };
            unsafe { std::ptr::write_unaligned(tag_output.cast::<u16>(), value) };
        }
        4 => {
            let Ok(value) = u32::try_from(value) else {
                return false;
            };
            unsafe { std::ptr::write_unaligned(tag_output.cast::<u32>(), value) };
        }
        8 => {
            let Ok(value) = u64::try_from(value) else {
                return false;
            };
            unsafe { std::ptr::write_unaligned(tag_output.cast::<u64>(), value) };
        }
        _ => return false,
    }
    true
}

unsafe fn write_direct_sequence(
    output: *mut u8,
    layout: DirectSequenceDecodeLayout,
    bytes: &[u8],
    element_count: usize,
) -> bool {
    let Some(expected_len) = layout.element_stride.checked_mul(element_count) else {
        return false;
    };
    if expected_len != bytes.len() {
        return false;
    }

    let ptr_value = if bytes.is_empty() {
        layout.element_align
    } else {
        let Ok(alloc_layout) =
            std::alloc::Layout::from_size_align(bytes.len(), layout.element_align)
        else {
            return false;
        };
        let ptr = unsafe { std::alloc::alloc(alloc_layout) };
        if ptr.is_null() {
            return false;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
        ptr as usize
    };

    unsafe {
        std::ptr::write(output.add(layout.ptr_offset).cast::<usize>(), ptr_value);
        std::ptr::write(output.add(layout.len_offset).cast::<usize>(), element_count);
        std::ptr::write(output.add(layout.cap_offset).cast::<usize>(), element_count);
    }
    true
}

fn run_fixed_decode_ops(ops: &[StencilOp], input: &[u8], output: &mut [u8]) -> bool {
    for op in ops {
        match op {
            StencilOp::Copy(op) => {
                let source_offset = op.input_offset;
                let output_offset = op.output_offset;
                let width = op.width.bytes();
                let Some(source) = input.get(source_offset..source_offset + width) else {
                    return false;
                };
                let Some(output) = output.get_mut(output_offset..output_offset + width) else {
                    return false;
                };
                output.copy_from_slice(source);
            }
            StencilOp::Bool {
                input_offset,
                output_offset,
                ..
            } => {
                let Some(value) = input.get(*input_offset).copied() else {
                    return false;
                };
                if value > 1 {
                    return false;
                }
                if let Some(output_offset) = output_offset {
                    let Some(output) = output.get_mut(*output_offset) else {
                        return false;
                    };
                    *output = value;
                }
            }
            StencilOp::RootEnum { .. } | StencilOp::RootOption { .. } => return false,
        }
    }
    true
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
        StencilEncodeHelper::SequenceBytes { failure_index, .. }
        | StencilEncodeHelper::SequenceFixedElements { failure_index, .. }
        | StencilEncodeHelper::SequenceOwnedFixedElements { failure_index, .. }
        | StencilEncodeHelper::SequenceProjectedElements { failure_index, .. }
        | StencilEncodeHelper::Enum { failure_index, .. }
        | StencilEncodeHelper::OptionSequenceBytes { failure_index, .. }
        | StencilEncodeHelper::SubtreeEncode { failure_index, .. } => {
            status_for_failure(*failure_index).unwrap_or(1)
        }
    };

    match helper {
        StencilEncodeHelper::SequenceBytes {
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
        StencilEncodeHelper::SequenceFixedElements {
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
        StencilEncodeHelper::SequenceOwnedFixedElements {
            input_offset,
            thunks,
            element_layout,
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
                let Some(()) = (unsafe {
                    with_projected_sequence_element(
                        value,
                        index,
                        *element_layout,
                        *thunks,
                        context,
                        |element| {
                            let element_base = out.len();
                            out.resize(element_base + element_output_len, 0);
                            for op in element_ops {
                                let source = element.add(op.input_offset);
                                copy_nonoverlapping(
                                    source,
                                    out.as_mut_ptr().add(element_base + op.output_offset),
                                    op.width.bytes(),
                                );
                            }
                            Some(())
                        },
                    )
                }) else {
                    return status;
                };
            }
        }
        StencilEncodeHelper::SequenceProjectedElements {
            input_offset,
            thunks,
            element_layout,
            element_encoder,
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
            for index in 0..len {
                let Some(element_bytes) = (unsafe {
                    with_projected_sequence_element(
                        value,
                        index,
                        *element_layout,
                        *thunks,
                        context,
                        |element| element_encoder.encode_raw_to_vec(element.cast_const()).ok(),
                    )
                }) else {
                    return status;
                };
                out.extend_from_slice(&element_bytes);
            }
        }
        StencilEncodeHelper::Enum {
            input_offset,
            tag_thunks,
            cases,
            ..
        } => {
            if value.is_null() {
                return status;
            }
            let value = value.wrapping_add(*input_offset);
            let tag_context = tag_thunks.context as *mut std::ffi::c_void;
            let local_index = unsafe { (tag_thunks.tag)(value, tag_context) };
            let Some(case) = cases.iter().find(|case| case.local_index == local_index) else {
                return status;
            };
            out.extend_from_slice(&case.wire_index.to_le_bytes());
            match &case.payload {
                LocalEnumEncodePayload::Unit => {}
                LocalEnumEncodePayload::Fixed {
                    project_thunks,
                    ops,
                    output_len,
                } => {
                    let context = project_thunks.context as *mut std::ffi::c_void;
                    let payload = unsafe { (project_thunks.project)(value, context) };
                    if payload.is_null() {
                        return status;
                    }
                    let payload_base = out.len();
                    out.resize(payload_base + output_len, 0);
                    for op in ops {
                        let source = unsafe { payload.add(op.input_offset) };
                        unsafe {
                            copy_nonoverlapping(
                                source,
                                out.as_mut_ptr().add(payload_base + op.output_offset),
                                op.width.bytes(),
                            );
                        }
                    }
                }
                LocalEnumEncodePayload::Nested {
                    project_thunks,
                    encoder,
                } => {
                    let context = project_thunks.context as *mut std::ffi::c_void;
                    let payload = unsafe { (project_thunks.project)(value, context) };
                    if payload.is_null() {
                        return status;
                    }
                    let Ok(payload_bytes) = (unsafe { encoder.encode_raw_to_vec(payload) }) else {
                        return status;
                    };
                    out.extend_from_slice(&payload_bytes);
                }
                LocalEnumEncodePayload::OwnedFixed {
                    project_into_thunks,
                    payload_layout,
                    ops,
                    output_len,
                } => {
                    let Some(()) = (unsafe {
                        with_owned_projected_payload(
                            value,
                            *payload_layout,
                            *project_into_thunks,
                            |payload| {
                                let payload_base = out.len();
                                out.resize(payload_base + output_len, 0);
                                for op in ops {
                                    let source = payload.add(op.input_offset);
                                    copy_nonoverlapping(
                                        source,
                                        out.as_mut_ptr().add(payload_base + op.output_offset),
                                        op.width.bytes(),
                                    );
                                }
                                Some(())
                            },
                        )
                    }) else {
                        return status;
                    };
                }
                LocalEnumEncodePayload::OwnedNested {
                    project_into_thunks,
                    payload_layout,
                    encoder,
                } => {
                    let Some(payload_bytes) = (unsafe {
                        with_owned_projected_payload(
                            value,
                            *payload_layout,
                            *project_into_thunks,
                            |payload| encoder.encode_raw_to_vec(payload).ok(),
                        )
                    }) else {
                        return status;
                    };
                    out.extend_from_slice(&payload_bytes);
                }
                LocalEnumEncodePayload::SequenceBytes {
                    project_thunks,
                    thunks,
                } => {
                    let project_context = project_thunks.context as *mut std::ffi::c_void;
                    let payload = unsafe { (project_thunks.project)(value, project_context) };
                    if payload.is_null() {
                        return status;
                    }
                    let sequence_context = thunks.context as *mut std::ffi::c_void;
                    let len = unsafe { (thunks.len)(payload, sequence_context) };
                    let Ok(len_u32) = u32::try_from(len) else {
                        return status;
                    };
                    out.extend_from_slice(&len_u32.to_le_bytes());
                    out.reserve(len);
                    for index in 0..len {
                        out.push(unsafe { (thunks.element_u8)(payload, index, sequence_context) });
                    }
                }
                LocalEnumEncodePayload::OwnedSequenceBytes {
                    project_into_thunks,
                    payload_layout,
                    thunks,
                } => {
                    let Some(()) = (unsafe {
                        with_owned_projected_payload(
                            value,
                            *payload_layout,
                            *project_into_thunks,
                            |payload| {
                                let sequence_context = thunks.context as *mut std::ffi::c_void;
                                let len = (thunks.len)(payload, sequence_context);
                                let Ok(len_u32) = u32::try_from(len) else {
                                    return None;
                                };
                                out.extend_from_slice(&len_u32.to_le_bytes());
                                out.reserve(len);
                                for index in 0..len {
                                    out.push((thunks.element_u8)(payload, index, sequence_context));
                                }
                                Some(())
                            },
                        )
                    }) else {
                        return status;
                    };
                }
            }
        }
        StencilEncodeHelper::OptionSequenceBytes {
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
        StencilEncodeHelper::SubtreeEncode {
            input_offset,
            thunks,
            root,
            ..
        } => {
            if value.is_null() {
                return status;
            }
            let value = value.wrapping_add(*input_offset);
            let context = thunks.context as *mut std::ffi::c_void;
            if !unsafe {
                (thunks.encode)(
                    value,
                    out,
                    std::ptr::from_ref(root).cast::<std::ffi::c_void>(),
                    runtime.nodes.as_ptr().cast::<std::ffi::c_void>(),
                    runtime.nodes.len(),
                    context,
                )
            } {
                return status;
            }
        }
    }

    STENCIL_OK
}

unsafe fn with_owned_projected_payload<T>(
    value: *const u8,
    layout: LocalValueLayout,
    thunks: LocalVariantProjectIntoThunks,
    f: impl FnOnce(*const u8) -> Option<T>,
) -> Option<T> {
    unsafe {
        with_aligned_scratch(layout, |scratch| {
            let project_context = thunks.project_context as *mut std::ffi::c_void;
            let projected = (thunks.project_into)(value, scratch, layout.size, project_context);
            if !projected {
                return None;
            }
            let result = f(scratch.cast_const());
            if let Some(drop_projected) = thunks.drop_projected {
                let drop_context = thunks.drop_context as *mut std::ffi::c_void;
                drop_projected(scratch, drop_context);
            }
            result
        })
    }
}

unsafe fn with_projected_sequence_element<T>(
    value: *const u8,
    index: usize,
    layout: LocalValueLayout,
    thunks: LocalSequenceElementProjectIntoEncodeThunks,
    context: *mut std::ffi::c_void,
    f: impl FnOnce(*mut u8) -> Option<T>,
) -> Option<T> {
    unsafe {
        with_aligned_scratch(layout, |scratch| {
            if !(thunks.element_project_into)(value, index, scratch, layout.size, context) {
                return None;
            }
            let result = f(scratch);
            if let Some(drop_projected) = thunks.element_drop_projected {
                drop_projected(scratch, context);
            }
            result
        })
    }
}

unsafe fn with_aligned_scratch<T>(
    layout: LocalValueLayout,
    f: impl FnOnce(*mut u8) -> Option<T>,
) -> Option<T> {
    let alloc_layout =
        std::alloc::Layout::from_size_align(layout.size.max(1), layout.align).ok()?;
    let scratch = if layout.size == 0 {
        std::ptr::NonNull::<u8>::dangling().as_ptr()
    } else {
        let ptr = unsafe { std::alloc::alloc(alloc_layout) };
        if ptr.is_null() {
            return None;
        }
        ptr
    };
    let result = f(scratch);
    if layout.size != 0 {
        unsafe { std::alloc::dealloc(scratch, alloc_layout) };
    }
    result
}

pub(super) unsafe extern "C" fn stencil_copy_bytes(dst: *mut u8, src: *const u8, len: usize) {
    unsafe {
        copy_nonoverlapping(src, dst, len);
    }
}

pub(super) const STENCIL_OPTION_NONE: usize = 0;
pub(super) const STENCIL_OPTION_SOME: usize = 1;
pub(super) const STENCIL_OPTION_NONE_U8: u8 = 0;
pub(super) const STENCIL_OPTION_SOME_U8: u8 = 1;

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
