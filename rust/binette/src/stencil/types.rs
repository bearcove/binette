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
    RootList {
        shape: &'static Shape,
        element_ops: Vec<CopyOp>,
        element_input_len: usize,
        element_stride: usize,
        failure_index: usize,
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
        reader_shape: &'static Shape,
        output_offset: usize,
        failure_index: usize,
    },
    Skip {
        writer_type: TypeRef,
        failure_index: usize,
    },
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
    RootU32Tag {
        position: usize,
        cases: Vec<TaggedLength>,
    },
    RootList {
        count_position: usize,
        element_input_len: usize,
    },
}

impl LengthCheck {
    pub(super) fn fixed_expected_len(&self) -> Option<usize> {
        match self {
            LengthCheck::Exact(len) => Some(*len),
            LengthCheck::RootU32Tag { .. } => None,
            LengthCheck::RootList { .. } => None,
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
            LengthCheck::RootList {
                count_position,
                element_input_len,
            } => {
                let needed = count_position + 4;
                if input.len() < needed {
                    return Err(StencilError::InputLength {
                        expected: needed,
                        actual: input.len(),
                    });
                }
                let count = u32::from_le_bytes(input[*count_position..needed].try_into().unwrap());
                let body_len = (count as usize)
                    .checked_mul(*element_input_len)
                    .ok_or_else(|| StencilError::Unsupported {
                        path: "$".to_owned(),
                        reason: "stencil list input length overflows usize",
                    })?;
                let expected =
                    needed
                        .checked_add(body_len)
                        .ok_or_else(|| StencilError::Unsupported {
                            path: "$".to_owned(),
                            reason: "stencil list input length overflows usize",
                        })?;
                if input.len() != expected {
                    return Err(StencilError::InputLength {
                        expected,
                        actual: input.len(),
                    });
                }
            }
        }
        Ok(())
    }
}
