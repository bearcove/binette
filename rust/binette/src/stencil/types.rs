use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct CopyOp {
    pub(super) input_offset: usize,
    pub(super) output_offset: usize,
    pub(super) width: CopyWidth,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CopyWidth {
    One,
    Two,
    Four,
    Eight,
}

impl CopyWidth {
    pub(super) fn bytes(self) -> usize {
        match self {
            CopyWidth::One => 1,
            CopyWidth::Two => 2,
            CopyWidth::Four => 4,
            CopyWidth::Eight => 8,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum StencilFailure {
    InvalidBool {
        path: String,
        position: usize,
    },
    InvalidOptionTag {
        path: String,
        position: usize,
    },
    UnknownVariantIndex {
        position: usize,
    },
    UnreadableWriterVariant {
        position: usize,
        variant_index: u32,
        variant: String,
    },
    Helper {
        path: String,
    },
}

#[derive(Debug, Clone)]
pub(super) enum StencilOp {
    Copy(CopyOp),
    Bool {
        input_offset: usize,
        output_offset: Option<usize>,
        failure_index: usize,
    },
    RootEnum {
        input_offset: usize,
        tag_output_offset: usize,
        cases: Vec<EnumCase>,
        bodies: Vec<Vec<StencilOp>>,
        unknown_failure_index: usize,
    },
    RootOption {
        input_offset: usize,
        tag_output_offset: usize,
        tag_output_width: usize,
        none_value: usize,
        some_value: Option<usize>,
        body: Vec<StencilOp>,
        invalid_failure_index: usize,
    },
}

#[derive(Debug, Clone)]
pub(super) enum HybridStencilOp {
    Helper {
        helper_index: usize,
    },
    Copy {
        ops: Vec<CopyOp>,
        input_len: usize,
        failure_index: usize,
    },
    Bool {
        output_offset: usize,
        failure_index: usize,
    },
}

pub(super) enum StencilHelper {
    SequenceBytes {
        output_offset: usize,
        thunks: LocalSequenceDecodeThunks,
        failure_index: usize,
    },
    SequenceFixedElements {
        output_offset: usize,
        thunks: LocalSequenceFixedDecodeThunks,
        element_ops: Vec<StencilOp>,
        element_input_len: usize,
        element_stride: usize,
        failure_index: usize,
    },
    DirectSequenceBytes {
        output_offset: usize,
        layout: DirectSequenceDecodeLayout,
        primitive: Primitive,
        failure_index: usize,
    },
    DirectSequenceFixedElements {
        output_offset: usize,
        layout: DirectSequenceDecodeLayout,
        element_ops: Vec<StencilOp>,
        element_input_len: usize,
        failure_index: usize,
    },
    DirectOptionSequenceBytes {
        output_offset: usize,
        option: DirectOptionDecodeLayout,
        sequence: DirectSequenceDecodeLayout,
        primitive: Primitive,
        failure_index: usize,
    },
    DirectOptionFixed {
        output_offset: usize,
        option: DirectOptionDecodeLayout,
        element_ops: Vec<StencilOp>,
        element_input_len: usize,
        element_output_len: usize,
        failure_index: usize,
    },
    OptionSequenceBytes {
        output_offset: usize,
        thunks: LocalOptionSequenceDecodeThunks,
        failure_index: usize,
    },
    Enum {
        output_offset: usize,
        cases: Vec<LocalEnumDecodeCase>,
        failure_index: usize,
    },
    Skip {
        writer_type: TypeRef,
        failure_index: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DirectSequenceDecodeLayout {
    pub(super) ptr_offset: usize,
    pub(super) len_offset: usize,
    pub(super) cap_offset: usize,
    pub(super) element_stride: usize,
    pub(super) element_align: usize,
}

#[derive(Debug, Clone)]
pub(super) struct DirectOptionDecodeLayout {
    pub(super) tag_offset: usize,
    pub(super) tag_width: usize,
    pub(super) none_value: usize,
    pub(super) none_bytes: Option<Vec<u8>>,
    pub(super) some_value: Option<usize>,
    pub(super) some_offset: usize,
    pub(super) option_size: usize,
}

pub(super) struct LocalEnumDecodeCase {
    pub(super) wire_index: u32,
    pub(super) construct_thunks: LocalVariantConstructThunks,
    pub(super) payload: LocalEnumDecodePayload,
}

pub(super) enum LocalEnumDecodePayload {
    Unit,
    Fixed {
        ops: Vec<StencilOp>,
        input_len: usize,
        payload_layout: LocalValueLayout,
    },
    Nested {
        decoder: Box<LocalStencilDecoder>,
        payload_layout: LocalValueLayout,
    },
    SequenceBytes,
}

pub(super) struct StencilRuntime {
    pub(super) writer_registry: SchemaRegistry,
    pub(super) helpers: Vec<StencilHelper>,
}

#[derive(Debug, Clone)]
pub(super) enum EncodeStencilOp {
    Helper {
        helper_index: usize,
    },
    Direct {
        ops: Vec<CopyOp>,
        output_len: usize,
    },
    Bytes {
        input_offset: usize,
        kind: EncodeBytesKind,
        layout: EncodeBytesLayout,
    },
    Enum {
        input_offset: usize,
        selector: EncodeEnumSelector,
        cases: Vec<EncodeEnumCase>,
    },
    Option {
        input_offset: usize,
        layout: EncodeOptionLayout,
        some_ops: Vec<EncodeStencilOp>,
    },
    List {
        input_offset: usize,
        layout: EncodeListLayout,
        element_ops: Vec<EncodeStencilOp>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EncodeBytesKind {
    String,
    Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EncodeBytesLayout {
    Direct {
        ptr_offset: usize,
        len_offset: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EncodeOptionLayout {
    DirectTag {
        tag_offset: usize,
        tag_width: usize,
        none_value: usize,
        some_offset: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EncodeListLayout {
    Vec {
        ptr_offset: usize,
        len_offset: usize,
        element_stride: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) enum EncodeEnumSelector {
    DirectTag { offset: usize },
}

#[derive(Debug, Clone)]
pub(super) struct EncodeEnumCase {
    pub(super) local_index: u32,
    pub(super) wire_index: u32,
    pub(super) ops: Vec<EncodeStencilOp>,
}

pub(super) enum StencilEncodeHelper {
    SequenceBytes {
        input_offset: usize,
        thunks: LocalSequenceEncodeThunks,
        failure_index: usize,
    },
    SequenceFixedElements {
        input_offset: usize,
        thunks: LocalSequenceElementPtrEncodeThunks,
        element_ops: Vec<CopyOp>,
        element_output_len: usize,
        failure_index: usize,
    },
    SequenceOwnedFixedElements {
        input_offset: usize,
        thunks: LocalSequenceElementProjectIntoEncodeThunks,
        element_layout: LocalValueLayout,
        element_ops: Vec<CopyOp>,
        element_output_len: usize,
        failure_index: usize,
    },
    SequenceProjectedElements {
        input_offset: usize,
        thunks: LocalSequenceElementProjectIntoEncodeThunks,
        element_layout: LocalValueLayout,
        element_encoder: Box<LocalStencilEncoder>,
        failure_index: usize,
    },
    Enum {
        input_offset: usize,
        tag_thunks: LocalEnumTagThunks,
        cases: Vec<LocalEnumEncodeCase>,
        failure_index: usize,
    },
    OptionSequenceBytes {
        input_offset: usize,
        option_thunks: LocalOptionEncodeThunks,
        sequence_thunks: LocalSequenceEncodeThunks,
        failure_index: usize,
    },
}

pub(super) struct LocalEnumEncodeCase {
    pub(super) local_index: u32,
    pub(super) wire_index: u32,
    pub(super) payload: LocalEnumEncodePayload,
}

pub(super) enum LocalEnumEncodePayload {
    Unit,
    Fixed {
        project_thunks: LocalVariantProjectThunks,
        ops: Vec<CopyOp>,
        output_len: usize,
    },
    OwnedFixed {
        project_into_thunks: LocalVariantProjectIntoThunks,
        payload_layout: LocalValueLayout,
        ops: Vec<CopyOp>,
        output_len: usize,
    },
    OwnedNested {
        project_into_thunks: LocalVariantProjectIntoThunks,
        payload_layout: LocalValueLayout,
        encoder: Box<LocalStencilEncoder>,
    },
    SequenceBytes {
        project_thunks: LocalVariantProjectThunks,
        thunks: LocalSequenceEncodeThunks,
    },
    OwnedSequenceBytes {
        project_into_thunks: LocalVariantProjectIntoThunks,
        payload_layout: LocalValueLayout,
        thunks: LocalSequenceEncodeThunks,
    },
}

pub(super) struct StencilEncodeRuntime {
    pub(super) helpers: Vec<StencilEncodeHelper>,
}

pub(super) struct FixedEncodeCompiler {
    pub(super) ops: Vec<CopyOp>,
    pub(super) output_offset: usize,
}

pub(super) struct FixedEncodeSegment {
    pub(super) ops: Vec<CopyOp>,
    pub(super) output_len: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EnumCase {
    pub(super) writer_index: u32,
    pub(super) reader_discriminant: Option<u8>,
    pub(super) body_index: Option<usize>,
    pub(super) failure_index: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TaggedLength {
    pub(super) variant_index: u32,
    pub(super) expected: usize,
}

pub(super) enum LengthCheck {
    Exact(usize),
    RootU8Tag {
        position: usize,
        cases: Vec<ByteTaggedLength>,
    },
    RootU32Tag {
        position: usize,
        cases: Vec<TaggedLength>,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ByteTaggedLength {
    pub(super) tag: u8,
    pub(super) expected: usize,
}

impl LengthCheck {
    pub(super) fn fixed_expected_len(&self) -> Option<usize> {
        match self {
            LengthCheck::Exact(len) => Some(*len),
            LengthCheck::RootU8Tag { .. } => None,
            LengthCheck::RootU32Tag { .. } => None,
        }
    }

    pub(super) fn validate(&self, input: &[u8]) -> Result<(), StencilError> {
        match self {
            LengthCheck::Exact(expected) => {
                if input.len() != *expected {
                    return Err(StencilError::InputLength {
                        expected: *expected,
                        actual: input.len(),
                    });
                }
            }
            LengthCheck::RootU8Tag { position, cases } => {
                let needed = position + 1;
                if input.len() < needed {
                    return Err(StencilError::InputLength {
                        expected: needed,
                        actual: input.len(),
                    });
                }
                let tag = input[*position];
                if let Some(case) = cases.iter().find(|case| case.tag == tag)
                    && input.len() != case.expected
                {
                    return Err(StencilError::InputLength {
                        expected: case.expected,
                        actual: input.len(),
                    });
                }
            }
            LengthCheck::RootU32Tag { position, cases } => {
                let needed = position + 4;
                if input.len() < needed {
                    return Err(StencilError::InputLength {
                        expected: needed,
                        actual: input.len(),
                    });
                }
                let variant_index =
                    u32::from_le_bytes(input[*position..needed].try_into().unwrap());
                if let Some(case) = cases
                    .iter()
                    .find(|case| case.variant_index == variant_index)
                    && input.len() != case.expected
                {
                    return Err(StencilError::InputLength {
                        expected: case.expected,
                        actual: input.len(),
                    });
                }
            }
        }
        Ok(())
    }
}
