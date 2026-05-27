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
        cases: Vec<EnumCase>,
        bodies: Vec<Vec<StencilOp>>,
        unknown_failure_index: usize,
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
    List {
        shape: &'static Shape,
        output_offset: usize,
        element_ops: Vec<HybridStencilOp>,
        element_stride: usize,
        failure_index: usize,
    },
}

#[derive(Debug, Clone)]
pub(super) enum StencilHelper {
    Decode {
        plan: PlanNode,
        plan_nodes: Vec<PlanNode>,
        reader_shape: &'static Shape,
        output_offset: usize,
        failure_index: usize,
    },
    LocalSequenceBytes {
        output_offset: usize,
        thunks: LocalSequenceDecodeThunks,
        failure_index: usize,
    },
    LocalSequenceFixedElements {
        output_offset: usize,
        thunks: LocalSequenceFixedDecodeThunks,
        element_ops: Vec<StencilOp>,
        element_input_len: usize,
        element_stride: usize,
        failure_index: usize,
    },
    LocalOptionSequenceBytes {
        output_offset: usize,
        thunks: LocalOptionSequenceDecodeThunks,
        failure_index: usize,
    },
    LocalEnum {
        output_offset: usize,
        cases: Vec<LocalEnumDecodeCase>,
        failure_index: usize,
    },
    Skip {
        writer_type: TypeRef,
        failure_index: usize,
    },
}

#[derive(Debug, Clone)]
pub(super) struct LocalEnumDecodeCase {
    pub(super) wire_index: u32,
    pub(super) construct_thunks: LocalVariantConstructThunks,
    pub(super) payload: LocalEnumDecodePayload,
}

#[derive(Debug, Clone)]
pub(super) enum LocalEnumDecodePayload {
    Unit,
    Fixed {
        ops: Vec<StencilOp>,
        input_len: usize,
        local_size: usize,
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
        shape: &'static Shape,
        input_offset: usize,
        kind: EncodeBytesKind,
    },
    Enum {
        shape: &'static Shape,
        input_offset: usize,
        cases: Vec<EncodeEnumCase>,
    },
    Option {
        shape: &'static Shape,
        input_offset: usize,
        layout: EncodeOptionLayout,
        some_ops: Vec<EncodeStencilOp>,
    },
    List {
        shape: &'static Shape,
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
pub(super) enum EncodeOptionLayout {
    Facet,
    NicheString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EncodeListLayout {
    Facet,
    Vec {
        ptr_offset: usize,
        len_offset: usize,
        element_stride: usize,
    },
}

#[derive(Debug, Clone)]
pub(super) struct EncodeEnumCase {
    pub(super) facet_index: usize,
    pub(super) wire_index: u32,
    pub(super) ops: Vec<EncodeStencilOp>,
}

impl EncodeBytesKind {
    pub(super) fn abi_tag(self) -> usize {
        match self {
            EncodeBytesKind::String => STENCIL_ENCODE_BYTES_STRING,
            EncodeBytesKind::Bytes => STENCIL_ENCODE_BYTES_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum StencilEncodeHelper {
    Node {
        node: WriterNode,
        shape: &'static Shape,
        input_offset: usize,
        failure_index: usize,
    },
    LocalSequenceBytes {
        input_offset: usize,
        thunks: LocalSequenceEncodeThunks,
        failure_index: usize,
    },
    LocalSequenceFixedElements {
        input_offset: usize,
        thunks: LocalSequenceElementPtrEncodeThunks,
        element_ops: Vec<CopyOp>,
        element_output_len: usize,
        failure_index: usize,
    },
    LocalEnum {
        input_offset: usize,
        tag_thunks: LocalEnumTagThunks,
        cases: Vec<LocalEnumEncodeCase>,
        failure_index: usize,
    },
    LocalOptionSequenceBytes {
        input_offset: usize,
        option_thunks: LocalOptionEncodeThunks,
        sequence_thunks: LocalSequenceEncodeThunks,
        failure_index: usize,
    },
}

#[derive(Debug, Clone)]
pub(super) struct LocalEnumEncodeCase {
    pub(super) local_index: u32,
    pub(super) wire_index: u32,
    pub(super) payload: LocalEnumEncodePayload,
}

#[derive(Debug, Clone)]
pub(super) enum LocalEnumEncodePayload {
    Unit,
    Fixed {
        project_thunks: LocalVariantProjectThunks,
        ops: Vec<CopyOp>,
        output_len: usize,
    },
    SequenceBytes {
        project_thunks: LocalVariantProjectThunks,
        thunks: LocalSequenceEncodeThunks,
    },
}

pub(super) struct StencilEncodeRuntime {
    pub(super) helpers: Vec<StencilEncodeHelper>,
    pub(super) nodes: Vec<WriterNode>,
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
    RootU32Tag {
        position: usize,
        cases: Vec<TaggedLength>,
    },
}

impl LengthCheck {
    pub(super) fn fixed_expected_len(&self) -> Option<usize> {
        match self {
            LengthCheck::Exact(len) => Some(*len),
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
