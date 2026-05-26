use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::{NonNull, copy_nonoverlapping};
use std::slice;

use facet_core::{EnumRepr, EnumType, Facet, PtrConst, Shape, StructKind, Type, UserType};
use facet_reflect::Peek;
use thiserror::Error;

use crate::compact::{CompactError, CompactReader};
use crate::decode::{DecodeError, decode_plan_node_into_raw};
use crate::encode::{
    EncodeError, WriterFieldPlan, WriterNode, WriterPlan, encode_node_with_writer_node,
    writer_plan_for,
};
use crate::hash::primitive_for_type_id;
use crate::plan::{
    EnumPayloadPlan, EnumVariantPlan, PlanError, PlanNode, ReaderPlan, StructFieldPlan,
    reader_plan_for,
};
use crate::registry::SchemaRegistry;
use crate::schema::{Primitive, SchemaKind, TypeId, TypeRef};

type FixedStencilFn = unsafe extern "C" fn(input: *const u8, len: usize, out: *mut u8) -> u32;
type HybridStencilFn = unsafe extern "C" fn(
    runtime: *const StencilRuntime,
    input: *const u8,
    len: usize,
    out: *mut u8,
) -> usize;
type EncodeStencilFn = unsafe extern "C" fn(
    runtime: *const StencilEncodeRuntime,
    value: *const u8,
    out: *mut Vec<u8>,
) -> u32;

pub struct StencilDecoder<T> {
    code: ExecutableMemory,
    entry: StencilEntry,
    failures: Vec<StencilFailure>,
    _marker: PhantomData<fn() -> T>,
}

pub struct StencilEncoder<T> {
    code: ExecutableMemory,
    func: EncodeStencilFn,
    runtime: Box<StencilEncodeRuntime>,
    _marker: PhantomData<fn() -> T>,
}

enum StencilEntry {
    Fixed {
        func: FixedStencilFn,
        length_check: LengthCheck,
    },
    Hybrid {
        func: HybridStencilFn,
        runtime: Box<StencilRuntime>,
    },
}

#[derive(Debug, Error)]
pub enum StencilError {
    #[error(transparent)]
    Plan(#[from] PlanError),

    #[error(transparent)]
    Encode(#[from] EncodeError),

    #[error("unknown writer type id {type_id:?} at {path}")]
    UnknownWriterType { path: String, type_id: TypeId },

    #[error("unsupported stencil decode at {path}: {reason}")]
    Unsupported { path: String, reason: &'static str },

    #[error("expected {expected} bytes for stencil decode, got {actual}")]
    InputLength { expected: usize, actual: usize },

    #[error("invalid bool byte {value:#04x} at {path} byte {position}")]
    InvalidBool {
        path: String,
        position: usize,
        value: u8,
    },

    #[error("stencil returned unknown status {status}")]
    UnknownStatus { status: u32 },

    #[error("compact enum variant index {variant_index} is out of range at byte {position}")]
    UnknownVariantIndex { position: usize, variant_index: u32 },

    #[error("writer enum variant {variant} ({variant_index}) cannot be read at byte {position}")]
    UnreadableWriterVariant {
        position: usize,
        variant_index: u32,
        variant: String,
    },

    #[error("stencil helper failed at {path}")]
    HelperFailed { path: String },

    #[error("failed to allocate executable stencil memory")]
    ExecutableMemory,

    #[error("failed to make stencil memory executable")]
    Mprotect,
}

impl<T> StencilDecoder<T> {
    pub fn expected_len(&self) -> usize {
        self.fixed_expected_len()
            .expect("stencil decoder has variant-dependent or variable input lengths")
    }

    pub fn fixed_expected_len(&self) -> Option<usize> {
        match &self.entry {
            StencilEntry::Fixed { length_check, .. } => length_check.fixed_expected_len(),
            StencilEntry::Hybrid { .. } => None,
        }
    }

    pub fn code_len(&self) -> usize {
        self.code.len()
    }
}

impl<T: Facet<'static>> StencilDecoder<T> {
    pub fn decode(&self, input: &[u8]) -> Result<T, StencilError> {
        match &self.entry {
            StencilEntry::Fixed { func, length_check } => {
                length_check.validate(input)?;

                let mut out = MaybeUninit::<T>::uninit();
                // SAFETY: the compiled stencil was built from T::SHAPE field offsets and
                // writes every supported field exactly once before returning.
                let status = unsafe { func(input.as_ptr(), input.len(), out.as_mut_ptr().cast()) };
                if status == STENCIL_OK {
                    // SAFETY: status zero means every supported output byte was written.
                    unsafe { Ok(out.assume_init()) }
                } else {
                    Err(self.failure_for_status(status, input))
                }
            }
            StencilEntry::Hybrid { func, runtime } => {
                let mut out = MaybeUninit::<T>::uninit();
                // SAFETY: the generated function only calls stencil_decode_helper
                // with the runtime it was compiled with and writes through the
                // schema-derived offsets carried by that runtime.
                let result = unsafe {
                    func(
                        runtime.as_ref(),
                        input.as_ptr(),
                        input.len(),
                        out.as_mut_ptr().cast(),
                    )
                };
                if let Some(status) = hybrid_error_status(result) {
                    return Err(self.failure_for_status(status, input));
                }
                if result != input.len() {
                    return Err(StencilError::InputLength {
                        expected: result,
                        actual: input.len(),
                    });
                }
                // SAFETY: a successful hybrid result means every planned root
                // field was initialized and the entire input was consumed.
                unsafe { Ok(out.assume_init()) }
            }
        }
    }

    fn failure_for_status(&self, status: u32, input: &[u8]) -> StencilError {
        let Some(index) = status.checked_sub(1).map(|index| index as usize) else {
            return StencilError::UnknownStatus { status };
        };
        let Some(failure) = self.failures.get(index) else {
            return StencilError::UnknownStatus { status };
        };
        match failure {
            StencilFailure::InvalidBool { path, position } => StencilError::InvalidBool {
                path: path.clone(),
                position: *position,
                value: input[*position],
            },
            StencilFailure::UnknownVariantIndex { position } => {
                let variant_index =
                    u32::from_le_bytes(input[*position..*position + 4].try_into().unwrap());
                StencilError::UnknownVariantIndex {
                    position: *position,
                    variant_index,
                }
            }
            StencilFailure::UnreadableWriterVariant {
                position,
                variant_index,
                variant,
            } => StencilError::UnreadableWriterVariant {
                position: *position,
                variant_index: *variant_index,
                variant: variant.clone(),
            },
            StencilFailure::Helper { path } => StencilError::HelperFailed { path: path.clone() },
        }
    }
}

impl<T> StencilEncoder<T> {
    pub fn code_len(&self) -> usize {
        self.code.len()
    }
}

impl<T: Facet<'static>> StencilEncoder<T> {
    pub fn encode_to_vec(&self, value: &T) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        let status =
            unsafe { (self.func)(self.runtime.as_ref(), (value as *const T).cast(), &mut out) };
        if status == STENCIL_OK {
            Ok(out)
        } else {
            Err(EncodeError::Unsupported {
                shape: T::SHAPE,
                reason: "stencil encode helper failed",
            })
        }
    }
}

// r[impl binette.compat.plan]
// r[impl binette.mode.compact]
pub fn stencil_decoder_for<T: Facet<'static>>(
    writer_root: &TypeRef,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    let plan = reader_plan_for::<T>(writer_root, writer_registry)?;
    stencil_decoder_from_plan(&plan, writer_registry)
}

// r[impl binette.compat.plan]
// r[impl binette.mode.compact]
pub fn stencil_decoder_from_plan<T: Facet<'static>>(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    match fixed_stencil_decoder_from_plan(plan, writer_registry) {
        Ok(decoder) => Ok(decoder),
        Err(fixed_error) => {
            if matches!(&fixed_error, StencilError::Unsupported { .. })
                && let Ok(decoder) = hybrid_stencil_decoder_from_plan(plan, writer_registry)
            {
                return Ok(decoder);
            }
            Err(fixed_error)
        }
    }
}

// r[impl binette.mode.compact]
pub fn stencil_encoder_for<T: Facet<'static>>() -> Result<StencilEncoder<T>, StencilError> {
    let plan = writer_plan_for::<T>()?;
    stencil_encoder_from_plan(&plan)
}

// r[impl binette.mode.compact]
pub fn stencil_encoder_from_plan<T: Facet<'static>>(
    plan: &WriterPlan,
) -> Result<StencilEncoder<T>, StencilError> {
    let mut compiler = StencilEncodeCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler.compile_root::<T>(plan.root_node())?;

    let code = generate_encode_code(&compiler.ops)?;
    let func = code.as_encode_fn();
    Ok(StencilEncoder {
        code,
        func,
        runtime: Box::new(StencilEncodeRuntime {
            root_shape: T::SHAPE,
            helpers: compiler.helpers,
        }),
        _marker: PhantomData,
    })
}

pub fn encode_to_vec_with_stencil<T: Facet<'static>>(
    value: &T,
    encoder: &StencilEncoder<T>,
) -> Result<Vec<u8>, EncodeError> {
    encoder.encode_to_vec(value)
}

fn fixed_stencil_decoder_from_plan<T: Facet<'static>>(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    let mut compiler = StencilCompiler {
        writer_registry,
        ops: Vec::new(),
        failures: Vec::new(),
        input_offset: 0,
    };
    let length_check = compiler.compile_root::<T>(&plan.root)?;

    let code = generate_code(&compiler.ops, compiler.failures.len())?;
    let func = code.as_fixed_fn();
    Ok(StencilDecoder {
        code,
        entry: StencilEntry::Fixed { func, length_check },
        failures: compiler.failures,
        _marker: PhantomData,
    })
}

fn hybrid_stencil_decoder_from_plan<T: Facet<'static>>(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    let mut compiler = HybridStencilCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
    };
    compiler.compile_root::<T>(&plan.root)?;

    let code = generate_hybrid_code(&compiler.ops)?;
    let func = code.as_hybrid_fn();
    Ok(StencilDecoder {
        code,
        entry: StencilEntry::Hybrid {
            func,
            runtime: Box::new(StencilRuntime {
                writer_registry: writer_registry.clone(),
                helpers: compiler.helpers,
            }),
        },
        failures: compiler.failures,
        _marker: PhantomData,
    })
}

pub fn decode_from_slice_with_stencil<T: Facet<'static>>(
    input: &[u8],
    decoder: &StencilDecoder<T>,
) -> Result<T, DecodeError> {
    decoder.decode(input).map_err(|err| match err {
        StencilError::InvalidBool {
            position, value, ..
        } => CompactError::InvalidBool { position, value }.into(),
        StencilError::UnknownVariantIndex {
            position,
            variant_index,
        } => CompactError::UnknownVariantIndex {
            position,
            variant_index,
        }
        .into(),
        StencilError::UnreadableWriterVariant {
            position,
            variant_index,
            variant,
        } => DecodeError::UnreadableWriterVariant {
            position,
            variant_index,
            variant,
        },
        err => DecodeError::Unsupported {
            position: 0,
            reason: match err {
                StencilError::InputLength { .. } => "stencil input length mismatch",
                StencilError::Unsupported { reason, .. } => reason,
                StencilError::UnknownWriterType { .. } => "stencil writer type is unknown",
                StencilError::Plan(_) => "stencil plan failed",
                StencilError::Encode(_) => "stencil encode plan failed",
                StencilError::InvalidBool { .. } => unreachable!(),
                StencilError::UnknownStatus { .. } => "stencil returned an unknown status",
                StencilError::UnknownVariantIndex { .. } => unreachable!(),
                StencilError::UnreadableWriterVariant { .. } => unreachable!(),
                StencilError::HelperFailed { .. } => "stencil helper decode failed",
                StencilError::ExecutableMemory => "stencil executable memory allocation failed",
                StencilError::Mprotect => "stencil executable memory protection failed",
            },
        },
    })
}

#[derive(Debug, Clone, Copy)]
struct CopyOp {
    input_offset: usize,
    output_offset: usize,
    width: CopyWidth,
}

#[derive(Debug, Clone, Copy)]
enum CopyWidth {
    One,
    Two,
    Four,
    Eight,
}

#[derive(Debug, Clone)]
enum StencilFailure {
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
enum StencilOp {
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
enum HybridStencilOp {
    Helper { helper_index: usize },
}

#[derive(Debug, Clone)]
enum StencilHelper {
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

struct StencilRuntime {
    writer_registry: SchemaRegistry,
    helpers: Vec<StencilHelper>,
}

#[derive(Debug, Clone)]
enum EncodeStencilOp {
    Helper { helper_index: usize },
}

#[derive(Debug, Clone)]
enum StencilEncodeHelper {
    Value {
        node: WriterNode,
        failure_index: usize,
    },
    StructField {
        field: WriterFieldPlan,
        failure_index: usize,
    },
}

struct StencilEncodeRuntime {
    root_shape: &'static Shape,
    helpers: Vec<StencilEncodeHelper>,
}

#[derive(Debug, Clone, Copy)]
struct EnumCase {
    writer_index: u32,
    reader_discriminant: Option<u8>,
    body_index: Option<usize>,
    failure_index: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct TaggedLength {
    variant_index: u32,
    expected: usize,
}

enum LengthCheck {
    Exact(usize),
    RootU32Tag {
        position: usize,
        cases: Vec<TaggedLength>,
    },
}

impl LengthCheck {
    fn fixed_expected_len(&self) -> Option<usize> {
        match self {
            LengthCheck::Exact(len) => Some(*len),
            LengthCheck::RootU32Tag { .. } => None,
        }
    }

    fn validate(&self, input: &[u8]) -> Result<(), StencilError> {
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

struct StencilCompiler<'registry> {
    writer_registry: &'registry SchemaRegistry,
    ops: Vec<StencilOp>,
    failures: Vec<StencilFailure>,
    input_offset: usize,
}

impl StencilCompiler<'_> {
    fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &PlanNode,
    ) -> Result<LengthCheck, StencilError> {
        if let PlanNode::Enum { variants } = root {
            return self.compile_enum_root(T::SHAPE, variants, "$");
        }

        self.compile_node(T::SHAPE, root, 0, "$")?;
        Ok(LengthCheck::Exact(self.input_offset))
    }

    fn compile_node(
        &mut self,
        reader_shape: &'static Shape,
        node: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        match node {
            PlanNode::Primitive { primitive } => {
                self.compile_primitive_read(*primitive, output_offset, path)
            }
            PlanNode::Struct { fields } => {
                self.compile_struct_plan(reader_shape, fields, output_offset, path)
            }
            PlanNode::Tuple { elements } => {
                self.compile_tuple_plan(reader_shape, elements, output_offset, path)
            }
            PlanNode::Enum { variants } if output_offset == 0 => self
                .compile_enum_root(reader_shape, variants, path)
                .map(|_| ()),
            PlanNode::External { .. } => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "stencil external attachment decode is not implemented yet",
            }),
            _ => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports scalar structs, tuples, and root enums",
            }),
        }
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct_plan(
        &mut self,
        reader_shape: &'static Shape,
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        self.compile_struct_fields_plan(reader_fields, fields, output_offset, path)
    }

    fn compile_struct_fields_plan(
        &mut self,
        reader_fields: &'static [facet_core::Field],
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        for field in fields {
            match field {
                StructFieldPlan::Read {
                    reader_index,
                    name,
                    plan,
                    ..
                } => {
                    let reader_field = reader_fields.get(*reader_index).ok_or_else(|| {
                        StencilError::Unsupported {
                            path: format!("{path}.{name}"),
                            reason: "reader field index is out of range",
                        }
                    })?;
                    let field_offset = checked_offset(output_offset, reader_field.offset, path)?;
                    self.compile_read_plan(
                        plan,
                        reader_field.shape.get(),
                        field_offset,
                        &format!("{path}.{name}"),
                    )?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.compile_skip(writer_type, &format!("{path}.{name}"))?,
            }
        }
        Ok(())
    }

    fn compile_tuple_plan(
        &mut self,
        reader_shape: &'static Shape,
        elements: &[PlanNode],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        if reader_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "stencil tuple element count differs from reader shape",
            });
        }
        for (index, element) in elements.iter().enumerate() {
            let reader_field = &reader_fields[index];
            let field_offset = checked_offset(output_offset, reader_field.offset, path)?;
            self.compile_read_plan(
                element,
                reader_field.shape.get(),
                field_offset,
                &format!("{path}.{index}"),
            )?;
        }
        Ok(())
    }

    // r[impl binette.compat.enum]
    // r[impl binette.compat.enum.payload]
    fn compile_enum_root(
        &mut self,
        reader_shape: &'static Shape,
        variants: &[EnumVariantPlan],
        path: &str,
    ) -> Result<LengthCheck, StencilError> {
        if self.input_offset != 0 || !self.ops.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports enums at the root",
            });
        }

        let enum_type = shape_enum_type(reader_shape, path)?;
        if enum_type.enum_repr != EnumRepr::U8 {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil enum backend requires repr(u8) reader enums",
            });
        }

        let input_offset = self.input_offset;
        self.input_offset = checked_offset(self.input_offset, 4, path)?;
        let unknown_failure_index = self.failures.len();
        self.failures.push(StencilFailure::UnknownVariantIndex {
            position: input_offset,
        });

        let mut cases = Vec::with_capacity(variants.len());
        let mut bodies = Vec::new();
        let mut lengths = Vec::new();

        for variant in variants {
            match variant {
                EnumVariantPlan::Read {
                    writer_index,
                    reader_index,
                    name,
                    payload,
                } => {
                    let reader_variant =
                        enum_type.variants.get(*reader_index).ok_or_else(|| {
                            StencilError::Unsupported {
                                path: format!("{path}.{name}"),
                                reason: "reader enum variant index is out of range",
                            }
                        })?;
                    let reader_discriminant =
                        enum_discriminant_u8(reader_variant, &format!("{path}.{name}"))?;
                    let (body, expected) =
                        self.compile_branch_body(input_offset + 4, |compiler| {
                            compiler.compile_enum_payload_plan(
                                reader_variant.data.fields,
                                payload,
                                &format!("{path}.{name}"),
                            )
                        })?;
                    let body_index = bodies.len();
                    bodies.push(body);
                    lengths.push(TaggedLength {
                        variant_index: *writer_index,
                        expected,
                    });
                    cases.push(EnumCase {
                        writer_index: *writer_index,
                        reader_discriminant: Some(reader_discriminant),
                        body_index: Some(body_index),
                        failure_index: None,
                    });
                }
                EnumVariantPlan::Reject { writer_index, name } => {
                    let failure_index = self.failures.len();
                    self.failures.push(StencilFailure::UnreadableWriterVariant {
                        position: input_offset,
                        variant_index: *writer_index,
                        variant: name.clone(),
                    });
                    cases.push(EnumCase {
                        writer_index: *writer_index,
                        reader_discriminant: None,
                        body_index: None,
                        failure_index: Some(failure_index),
                    });
                }
            }
        }

        self.ops.push(StencilOp::RootEnum {
            input_offset,
            cases,
            bodies,
            unknown_failure_index,
        });

        Ok(LengthCheck::RootU32Tag {
            position: input_offset,
            cases: lengths,
        })
    }

    fn compile_branch_body(
        &mut self,
        input_offset: usize,
        compile: impl FnOnce(&mut Self) -> Result<(), StencilError>,
    ) -> Result<(Vec<StencilOp>, usize), StencilError> {
        let parent_ops = std::mem::take(&mut self.ops);
        let parent_input_offset = self.input_offset;
        self.input_offset = input_offset;

        let result = compile(self);
        let body_ops = std::mem::take(&mut self.ops);
        let body_input_offset = self.input_offset;
        self.ops = parent_ops;
        self.input_offset = parent_input_offset;

        result?;
        Ok((body_ops, body_input_offset))
    }

    fn compile_enum_payload_plan(
        &mut self,
        reader_fields: &'static [facet_core::Field],
        payload: &EnumPayloadPlan,
        path: &str,
    ) -> Result<(), StencilError> {
        match payload {
            EnumPayloadPlan::Unit => Ok(()),
            EnumPayloadPlan::Newtype(element) => {
                let reader_field =
                    reader_fields
                        .first()
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "newtype enum payload is missing reader field",
                        })?;
                self.compile_read_plan(element, reader_field.shape.get(), reader_field.offset, path)
            }
            EnumPayloadPlan::Tuple(elements) => {
                if reader_fields.len() != elements.len() {
                    return Err(StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "tuple enum payload arity differs from reader shape",
                    });
                }
                for (index, element) in elements.iter().enumerate() {
                    let reader_field = &reader_fields[index];
                    self.compile_read_plan(
                        element,
                        reader_field.shape.get(),
                        reader_field.offset,
                        &format!("{path}.{index}"),
                    )?;
                }
                Ok(())
            }
            EnumPayloadPlan::Struct(fields) => {
                self.compile_struct_fields_plan(reader_fields, fields, 0, path)
            }
        }
    }

    fn compile_read_plan(
        &mut self,
        node: &PlanNode,
        reader_shape: &'static Shape,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        self.compile_node(reader_shape, node, output_offset, path)
    }

    fn compile_skip(&mut self, type_ref: &TypeRef, path: &str) -> Result<(), StencilError> {
        self.compile_skip_type(type_ref, path)
    }

    fn compile_skip_type(&mut self, type_ref: &TypeRef, path: &str) -> Result<(), StencilError> {
        if let Some(primitive) = primitive_for_plain_type_ref(type_ref) {
            return self.compile_primitive_skip(primitive, path);
        }

        let kind = self.schema_for(type_ref, path)?.kind.clone();
        self.compile_skip_kind(&kind, path)
    }

    fn compile_skip_kind(&mut self, kind: &SchemaKind, path: &str) -> Result<(), StencilError> {
        match kind {
            SchemaKind::Primitive(primitive) => self.compile_primitive_skip(*primitive, path),
            SchemaKind::Struct { fields, .. } => {
                for field in fields {
                    self.compile_skip_type(&field.type_ref, &format!("{path}.{}", field.name))?;
                }
                Ok(())
            }
            SchemaKind::Tuple { elements } => {
                for (index, element) in elements.iter().enumerate() {
                    self.compile_skip_type(element, &format!("{path}.{index}"))?;
                }
                Ok(())
            }
            _ => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports scalar structs and tuples",
            }),
        }
    }

    fn compile_primitive_read(
        &mut self,
        primitive: Primitive,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        if primitive == Primitive::Bool {
            return self.emit_bool(path, Some(output_offset));
        }

        let Some(widths) = primitive_widths(primitive) else {
            return Err(unsupported_primitive(path, primitive));
        };
        self.emit_primitive_copies(path, output_offset, widths)
    }

    fn compile_primitive_skip(
        &mut self,
        primitive: Primitive,
        path: &str,
    ) -> Result<(), StencilError> {
        if primitive == Primitive::Bool {
            return self.emit_bool(path, None);
        }

        let Some(widths) = primitive_widths(primitive) else {
            return Err(unsupported_primitive(path, primitive));
        };
        for width in widths {
            self.input_offset = checked_offset(self.input_offset, width.bytes(), path)?;
        }
        Ok(())
    }

    // r[impl binette.scalar.unsigned]
    // r[impl binette.scalar.signed]
    // r[impl binette.scalar.float]
    fn emit_primitive_copies(
        &mut self,
        path: &str,
        output_offset: usize,
        widths: &'static [CopyWidth],
    ) -> Result<(), StencilError> {
        let mut output_offset = output_offset;
        for width in widths {
            self.ops.push(StencilOp::Copy(CopyOp {
                input_offset: self.input_offset,
                output_offset,
                width: *width,
            }));
            self.input_offset = checked_offset(self.input_offset, width.bytes(), path)?;
            output_offset = checked_offset(output_offset, width.bytes(), path)?;
        }
        Ok(())
    }

    // r[impl binette.scalar.bool]
    fn emit_bool(&mut self, path: &str, output_offset: Option<usize>) -> Result<(), StencilError> {
        let input_offset = self.input_offset;
        let failure_index = self.failures.len();
        self.failures.push(StencilFailure::InvalidBool {
            path: path.to_owned(),
            position: input_offset,
        });
        self.ops.push(StencilOp::Bool {
            input_offset,
            output_offset,
            failure_index,
        });
        self.input_offset = checked_offset(self.input_offset, 1, path)?;
        Ok(())
    }

    fn schema_for(
        &self,
        type_ref: &TypeRef,
        path: &str,
    ) -> Result<&crate::schema::Schema, StencilError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } if args.is_empty() => self
                .writer_registry
                .get(*type_id)
                .ok_or_else(|| StencilError::UnknownWriterType {
                    path: path.to_owned(),
                    type_id: *type_id,
                }),
            TypeRef::Concrete { .. } | TypeRef::Var { .. } => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend does not support generic type refs",
            }),
        }
    }
}

struct HybridStencilCompiler {
    ops: Vec<HybridStencilOp>,
    helpers: Vec<StencilHelper>,
    failures: Vec<StencilFailure>,
}

impl HybridStencilCompiler {
    fn compile_root<T: Facet<'static>>(&mut self, root: &PlanNode) -> Result<(), StencilError> {
        match root {
            PlanNode::Struct { fields } => self.compile_struct_root(T::SHAPE, fields, 0, "$"),
            _ => self.push_decode_helper(root, T::SHAPE, 0, "$"),
        }
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct_root(
        &mut self,
        reader_shape: &'static Shape,
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        for field in fields {
            match field {
                StructFieldPlan::Read {
                    reader_index,
                    name,
                    plan,
                    ..
                } => {
                    let reader_field = reader_fields.get(*reader_index).ok_or_else(|| {
                        StencilError::Unsupported {
                            path: format!("{path}.{name}"),
                            reason: "reader field index is out of range",
                        }
                    })?;
                    let field_offset = checked_offset(output_offset, reader_field.offset, path)?;
                    self.push_decode_helper(
                        plan,
                        reader_field.shape.get(),
                        field_offset,
                        &format!("{path}.{name}"),
                    )?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.push_skip_helper(writer_type, &format!("{path}.{name}"))?,
            }
        }
        Ok(())
    }

    fn push_decode_helper(
        &mut self,
        plan: &PlanNode,
        reader_shape: &'static Shape,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::Decode {
            plan: plan.clone(),
            reader_shape,
            output_offset,
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_skip_helper(&mut self, writer_type: &TypeRef, path: &str) -> Result<(), StencilError> {
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::Skip {
            writer_type: writer_type.clone(),
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_helper_failure(&mut self, path: &str) -> Result<usize, StencilError> {
        let failure_index = self.failures.len();
        let _ = status_for_failure(failure_index)?;
        self.failures.push(StencilFailure::Helper {
            path: path.to_owned(),
        });
        Ok(failure_index)
    }
}

struct StencilEncodeCompiler {
    ops: Vec<EncodeStencilOp>,
    helpers: Vec<StencilEncodeHelper>,
    failures: Vec<StencilFailure>,
}

impl StencilEncodeCompiler {
    fn compile_root<T: Facet<'static>>(&mut self, root: &WriterNode) -> Result<(), StencilError> {
        match root {
            WriterNode::Struct { fields } => self.compile_struct_root::<T>(fields),
            _ => self.push_value_helper(root, "$"),
        }
    }

    // r[impl binette.aggregate.struct.compact]
    fn compile_struct_root<T: Facet<'static>>(
        &mut self,
        fields: &[WriterFieldPlan],
    ) -> Result<(), StencilError> {
        let _ = shape_struct_fields(T::SHAPE, "$")?;
        for field in fields {
            self.push_field_helper(field)?;
        }
        Ok(())
    }

    fn push_field_helper(&mut self, field: &WriterFieldPlan) -> Result<(), StencilError> {
        let failure_index = self.push_helper_failure(&format!("$.{}", field.name))?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilEncodeHelper::StructField {
            field: field.clone(),
            failure_index,
        });
        self.ops.push(EncodeStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_value_helper(&mut self, node: &WriterNode, path: &str) -> Result<(), StencilError> {
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilEncodeHelper::Value {
            node: node.clone(),
            failure_index,
        });
        self.ops.push(EncodeStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_helper_failure(&mut self, path: &str) -> Result<usize, StencilError> {
        let failure_index = self.failures.len();
        let _ = status_for_failure(failure_index)?;
        self.failures.push(StencilFailure::Helper {
            path: path.to_owned(),
        });
        Ok(failure_index)
    }
}

impl CopyWidth {
    fn bytes(self) -> usize {
        match self {
            CopyWidth::One => 1,
            CopyWidth::Two => 2,
            CopyWidth::Four => 4,
            CopyWidth::Eight => 8,
        }
    }
}

const WIDTH_0: &[CopyWidth] = &[];
const WIDTH_1: &[CopyWidth] = &[CopyWidth::One];
const WIDTH_2: &[CopyWidth] = &[CopyWidth::Two];
const WIDTH_4: &[CopyWidth] = &[CopyWidth::Four];
const WIDTH_8: &[CopyWidth] = &[CopyWidth::Eight];
const WIDTH_16: &[CopyWidth] = &[CopyWidth::Eight, CopyWidth::Eight];

fn primitive_widths(primitive: Primitive) -> Option<&'static [CopyWidth]> {
    match primitive {
        Primitive::Unit => Some(WIDTH_0),
        Primitive::U8 | Primitive::I8 => Some(WIDTH_1),
        Primitive::U16 | Primitive::I16 => Some(WIDTH_2),
        Primitive::U32 | Primitive::I32 | Primitive::F32 => Some(WIDTH_4),
        Primitive::U64 | Primitive::I64 | Primitive::F64 => Some(WIDTH_8),
        Primitive::U128 | Primitive::I128 => Some(WIDTH_16),
        Primitive::Bool
        | Primitive::Never
        | Primitive::Char
        | Primitive::String
        | Primitive::Bytes
        | Primitive::Payload => None,
    }
}

fn unsupported_primitive(path: &str, primitive: Primitive) -> StencilError {
    StencilError::Unsupported {
        path: path.to_owned(),
        reason: match primitive {
            Primitive::Bool => "bool stencil decode still needs validation",
            Primitive::Never => "never has no compact value",
            Primitive::Char => "char stencil decode still needs scalar validation",
            Primitive::String | Primitive::Bytes | Primitive::Payload => {
                "variable-width stencil decode is not implemented yet"
            }
            _ => "unsupported primitive stencil decode",
        },
    }
}

fn primitive_for_plain_type_ref(type_ref: &TypeRef) -> Option<Primitive> {
    match type_ref {
        TypeRef::Concrete { type_id, args } if args.is_empty() => primitive_for_type_id(*type_id),
        TypeRef::Concrete { .. } | TypeRef::Var { .. } => None,
    }
}

fn shape_struct_fields(
    shape: &'static Shape,
    path: &str,
) -> Result<&'static [facet_core::Field], StencilError> {
    let Type::User(user) = shape.ty else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader shape is not a struct",
        });
    };
    let UserType::Struct(struct_type) = user else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader shape is not a struct",
        });
    };
    match struct_type.kind {
        StructKind::Struct | StructKind::TupleStruct | StructKind::Tuple => Ok(struct_type.fields),
        StructKind::Unit => Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "unit struct stencil decode is not implemented yet",
        }),
    }
}

fn shape_enum_type(shape: &'static Shape, path: &str) -> Result<EnumType, StencilError> {
    let Type::User(UserType::Enum(enum_type)) = shape.ty else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader shape is not an enum",
        });
    };
    Ok(enum_type)
}

fn enum_discriminant_u8(variant: &facet_core::Variant, path: &str) -> Result<u8, StencilError> {
    let discriminant = variant
        .discriminant
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader enum variant is missing a discriminant",
        })?;
    u8::try_from(discriminant).map_err(|_| StencilError::Unsupported {
        path: path.to_owned(),
        reason: "reader enum discriminant does not fit repr(u8)",
    })
}

fn checked_offset(offset: usize, width: usize, path: &str) -> Result<usize, StencilError> {
    offset
        .checked_add(width)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset overflow",
        })
}

const STENCIL_OK: u32 = 0;
const HYBRID_ERROR_FLAG: usize = 1usize << (usize::BITS - 1);

fn hybrid_error_status(value: usize) -> Option<u32> {
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

unsafe extern "C" fn stencil_decode_helper(
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
            reader_shape,
            output_offset,
            ..
        } => {
            let output = unsafe { out.add(*output_offset) };
            match unsafe {
                decode_plan_node_into_raw(
                    tail,
                    plan,
                    &runtime.writer_registry,
                    reader_shape,
                    output,
                )
            } {
                Ok(consumed) => consumed,
                Err(_) => return hybrid_error_for_helper(helper),
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
        StencilHelper::Decode { failure_index, .. } | StencilHelper::Skip { failure_index, .. } => {
            hybrid_error_for_failure(*failure_index)
        }
    }
}

unsafe extern "C" fn stencil_encode_helper(
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
        StencilEncodeHelper::Value { failure_index, .. }
        | StencilEncodeHelper::StructField { failure_index, .. } => {
            status_for_failure(*failure_index).unwrap_or(1)
        }
    };
    let root: Peek<'_, 'static> =
        unsafe { Peek::unchecked_new(PtrConst::new(value), runtime.root_shape) };

    match helper {
        StencilEncodeHelper::Value { node, .. } => {
            if encode_node_with_writer_node(out, root, node).is_err() {
                return status;
            }
        }
        StencilEncodeHelper::StructField { field, .. } => {
            let Ok(struct_peek) = root.into_struct() else {
                return status;
            };
            let Ok(field_peek) = struct_peek.field(field.facet_index) else {
                return status;
            };
            if encode_node_with_writer_node(out, field_peek, &field.node).is_err() {
                return status;
            }
        }
    }

    STENCIL_OK
}

fn generate_code(
    ops: &[StencilOp],
    failure_count: usize,
) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(ops.len() * 16 + (failure_count + 1) * 8);
        let mut branches = Vec::new();
        for op in ops {
            emit_op(&mut code, op, &mut branches)?;
        }
        push_u32(&mut code, mov_w0_immediate(STENCIL_OK)?);
        push_u32(&mut code, AARCH64_RET);

        let mut failure_offsets = Vec::with_capacity(failure_count);
        for index in 0..failure_count {
            failure_offsets.push(code.len());
            push_u32(&mut code, mov_w0_immediate(status_for_failure(index)?)?);
            push_u32(&mut code, AARCH64_RET);
        }
        for branch in branches {
            let Some(target) = failure_offsets.get(branch.failure_index).copied() else {
                return Err(StencilError::Unsupported {
                    path: "$code".to_owned(),
                    reason: "stencil branch references missing failure stub",
                });
            };
            let word = match branch.kind {
                BranchKind::CondHi => patch_cond_branch_imm19(AARCH64_B_HI, branch.offset, target)?,
                BranchKind::Uncond => patch_uncond_branch_imm26(AARCH64_B, branch.offset, target)?,
            };
            code[branch.offset..branch.offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = ops;
        let _ = failure_count;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

fn generate_hybrid_code(ops: &[HybridStencilOp]) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(ops.len() * 48 + 128);
        emit_hybrid_prologue(&mut code);

        let mut error_branches = Vec::new();
        for op in ops {
            emit_hybrid_op(&mut code, op, &mut error_branches)?;
        }

        push_u32(&mut code, AARCH64_MOV_X0_X23);
        let epilogue_offset = code.len();
        emit_hybrid_epilogue(&mut code);

        for branch_offset in error_branches {
            let word =
                patch_test_bit_branch_imm14(AARCH64_TBNZ_X0_63, branch_offset, epilogue_offset)?;
            code[branch_offset..branch_offset + 4].copy_from_slice(&word.to_le_bytes());
        }

        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = ops;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

fn generate_encode_code(ops: &[EncodeStencilOp]) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(ops.len() * 40 + 96);
        emit_encode_prologue(&mut code);

        let mut error_branches = Vec::new();
        for op in ops {
            emit_encode_op(&mut code, op, &mut error_branches)?;
        }

        push_u32(&mut code, mov_w0_immediate(STENCIL_OK)?);
        let epilogue_offset = code.len();
        emit_encode_epilogue(&mut code);

        for branch_offset in error_branches {
            let word = patch_cond_branch_imm19(AARCH64_B_NE, branch_offset, epilogue_offset)?;
            code[branch_offset..branch_offset + 4].copy_from_slice(&word.to_le_bytes());
        }

        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = ops;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_RET: u32 = 0xD65F_03C0;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_CMP_W9_1: u32 = 0x7100_053F;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B_HI: u32 = 0x5400_0008;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B_EQ: u32 = 0x5400_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B_NE: u32 = 0x5400_0001;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B: u32 = 0x1400_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_CMP_W0_0: u32 = 0x7100_001F;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X29_X30_PRE: u32 = 0xA9BF_7BFD;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X29_SP: u32 = 0x9100_03FD;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X19_X20_PRE: u32 = 0xA9BF_53F3;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X21_X22_PRE: u32 = 0xA9BF_5BF5;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X23_X24_PRE: u32 = 0xA9BF_63F7;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X19_X0: u32 = 0xAA00_03F3;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X20_X1: u32 = 0xAA01_03F4;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X21_X2: u32 = 0xAA02_03F5;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X22_X3: u32 = 0xAA03_03F6;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X23_0: u32 = 0xD280_0017;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X0_X19: u32 = 0xAA13_03E0;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X1_X20: u32 = 0xAA14_03E1;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X2_X21: u32 = 0xAA15_03E2;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X3_X22: u32 = 0xAA16_03E3;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X4_X23: u32 = 0xAA17_03E4;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X23_X0: u32 = 0xAA00_03F7;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X0_X23: u32 = 0xAA17_03E0;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_BLR_X16: u32 = 0xD63F_0200;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_TBNZ_X0_63: u32 = 0xB7F8_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X23_X24_POST: u32 = 0xA8C1_63F7;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X21_X22_POST: u32 = 0xA8C1_5BF5;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X19_X20_POST: u32 = 0xA8C1_53F3;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X29_X30_POST: u32 = 0xA8C1_7BFD;

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
struct BranchFixup {
    offset: usize,
    failure_index: usize,
    kind: BranchKind,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
enum BranchKind {
    CondHi,
    Uncond,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_op(
    code: &mut Vec<u8>,
    op: &StencilOp,
    branches: &mut Vec<BranchFixup>,
) -> Result<(), StencilError> {
    match op {
        StencilOp::Copy(op) => emit_copy_op(code, *op),
        StencilOp::Bool {
            input_offset,
            output_offset,
            failure_index,
        } => emit_bool_op(
            code,
            *input_offset,
            *output_offset,
            *failure_index,
            branches,
        ),
        StencilOp::RootEnum {
            input_offset,
            cases,
            bodies,
            unknown_failure_index,
        } => emit_root_enum_op(
            code,
            *input_offset,
            cases,
            bodies,
            *unknown_failure_index,
            branches,
        ),
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_copy_op(code: &mut Vec<u8>, op: CopyOp) -> Result<(), StencilError> {
    let (load, store) = match op.width {
        CopyWidth::One => (0x3840_0009, 0x3800_0049),
        CopyWidth::Two => (0x7840_0009, 0x7800_0049),
        CopyWidth::Four => (0xB840_0009, 0xB800_0049),
        CopyWidth::Eight => (0xF840_0009, 0xF800_0049),
    };
    push_u32(code, patch_ldur_stur_imm9(load, op.input_offset, "$input")?);
    push_u32(
        code,
        patch_ldur_stur_imm9(store, op.output_offset, "$output")?,
    );
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_bool_op(
    code: &mut Vec<u8>,
    input_offset: usize,
    output_offset: Option<usize>,
    failure_index: usize,
    branches: &mut Vec<BranchFixup>,
) -> Result<(), StencilError> {
    push_u32(
        code,
        patch_ldur_stur_imm9(AARCH64_LDURB_W9_X0, input_offset, "$input")?,
    );
    push_u32(code, AARCH64_CMP_W9_1);
    let branch_offset = code.len();
    push_u32(code, 0);
    branches.push(BranchFixup {
        offset: branch_offset,
        failure_index,
        kind: BranchKind::CondHi,
    });
    if let Some(output_offset) = output_offset {
        push_u32(
            code,
            patch_ldur_stur_imm9(AARCH64_STURB_W9_X2, output_offset, "$output")?,
        );
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDURB_W9_X0: u32 = 0x3840_0009;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDUR_W9_X0: u32 = 0xB840_0009;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STURB_W9_X2: u32 = 0x3800_0049;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STURB_W10_X2: u32 = 0x3800_004A;

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_root_enum_op(
    code: &mut Vec<u8>,
    input_offset: usize,
    cases: &[EnumCase],
    bodies: &[Vec<StencilOp>],
    unknown_failure_index: usize,
    branches: &mut Vec<BranchFixup>,
) -> Result<(), StencilError> {
    push_u32(
        code,
        patch_ldur_stur_imm9(AARCH64_LDUR_W9_X0, input_offset, "$input")?,
    );

    let mut case_branches = Vec::with_capacity(cases.len());
    for (case_index, case) in cases.iter().enumerate() {
        push_u32(code, cmp_w9_immediate(case.writer_index)?);
        let offset = code.len();
        push_u32(code, 0);
        case_branches.push((offset, case_index));
    }

    let unknown_branch = code.len();
    push_u32(code, 0);
    branches.push(BranchFixup {
        offset: unknown_branch,
        failure_index: unknown_failure_index,
        kind: BranchKind::Uncond,
    });

    let mut case_offsets = Vec::with_capacity(cases.len());
    let mut done_branches = Vec::new();
    for case in cases {
        case_offsets.push(code.len());
        if let Some(failure_index) = case.failure_index {
            let offset = code.len();
            push_u32(code, 0);
            branches.push(BranchFixup {
                offset,
                failure_index,
                kind: BranchKind::Uncond,
            });
            continue;
        }

        let Some(reader_discriminant) = case.reader_discriminant else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "readable enum case is missing reader discriminant",
            });
        };
        push_u32(code, mov_w10_immediate(u32::from(reader_discriminant))?);
        push_u32(
            code,
            patch_ldur_stur_imm9(AARCH64_STURB_W10_X2, 0, "$output")?,
        );

        let Some(body_index) = case.body_index else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "readable enum case is missing body ops",
            });
        };
        let Some(body) = bodies.get(body_index) else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "enum body index is out of range",
            });
        };
        for op in body {
            emit_op(code, op, branches)?;
        }
        let offset = code.len();
        push_u32(code, 0);
        done_branches.push(offset);
    }

    let done = code.len();
    for (offset, case_index) in case_branches {
        let Some(target) = case_offsets.get(case_index).copied() else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "enum case branch target is missing",
            });
        };
        let word = patch_cond_branch_imm19(AARCH64_B_EQ, offset, target)?;
        code[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    for offset in done_branches {
        let word = patch_uncond_branch_imm26(AARCH64_B, offset, done)?;
        code[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_prologue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_STP_X29_X30_PRE);
    push_u32(code, AARCH64_MOV_X29_SP);
    push_u32(code, AARCH64_STP_X19_X20_PRE);
    push_u32(code, AARCH64_STP_X21_X22_PRE);
    push_u32(code, AARCH64_STP_X23_X24_PRE);
    push_u32(code, AARCH64_MOV_X19_X0);
    push_u32(code, AARCH64_MOV_X20_X1);
    push_u32(code, AARCH64_MOV_X21_X2);
    push_u32(code, AARCH64_MOV_X22_X3);
    push_u32(code, AARCH64_MOV_X23_0);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_epilogue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_LDP_X23_X24_POST);
    push_u32(code, AARCH64_LDP_X21_X22_POST);
    push_u32(code, AARCH64_LDP_X19_X20_POST);
    push_u32(code, AARCH64_LDP_X29_X30_POST);
    push_u32(code, AARCH64_RET);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_op(
    code: &mut Vec<u8>,
    op: &HybridStencilOp,
    error_branches: &mut Vec<usize>,
) -> Result<(), StencilError> {
    match op {
        HybridStencilOp::Helper { helper_index } => {
            push_u32(code, AARCH64_MOV_X0_X19);
            push_u32(code, AARCH64_MOV_X1_X20);
            push_u32(code, AARCH64_MOV_X2_X21);
            push_u32(code, AARCH64_MOV_X3_X22);
            push_u32(code, AARCH64_MOV_X4_X23);
            let helper_index =
                u64::try_from(*helper_index).map_err(|_| StencilError::Unsupported {
                    path: "$code".to_owned(),
                    reason: "stencil helper index exceeds u64",
                })?;
            emit_mov_x_immediate(code, 5, helper_index)?;
            emit_mov_x_immediate(code, 16, stencil_decode_helper as *const () as usize as u64)?;
            push_u32(code, AARCH64_BLR_X16);
            let branch_offset = code.len();
            push_u32(code, 0);
            error_branches.push(branch_offset);
            push_u32(code, AARCH64_MOV_X23_X0);
        }
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_prologue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_STP_X29_X30_PRE);
    push_u32(code, AARCH64_MOV_X29_SP);
    push_u32(code, AARCH64_STP_X19_X20_PRE);
    push_u32(code, AARCH64_STP_X21_X22_PRE);
    push_u32(code, AARCH64_MOV_X19_X0);
    push_u32(code, AARCH64_MOV_X20_X1);
    push_u32(code, AARCH64_MOV_X21_X2);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_epilogue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_LDP_X21_X22_POST);
    push_u32(code, AARCH64_LDP_X19_X20_POST);
    push_u32(code, AARCH64_LDP_X29_X30_POST);
    push_u32(code, AARCH64_RET);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_op(
    code: &mut Vec<u8>,
    op: &EncodeStencilOp,
    error_branches: &mut Vec<usize>,
) -> Result<(), StencilError> {
    match op {
        EncodeStencilOp::Helper { helper_index } => {
            push_u32(code, AARCH64_MOV_X0_X19);
            push_u32(code, AARCH64_MOV_X1_X20);
            push_u32(code, AARCH64_MOV_X2_X21);
            let helper_index =
                u64::try_from(*helper_index).map_err(|_| StencilError::Unsupported {
                    path: "$code".to_owned(),
                    reason: "stencil helper index exceeds u64",
                })?;
            emit_mov_x_immediate(code, 3, helper_index)?;
            emit_mov_x_immediate(code, 16, stencil_encode_helper as *const () as usize as u64)?;
            push_u32(code, AARCH64_BLR_X16);
            push_u32(code, AARCH64_CMP_W0_0);
            let branch_offset = code.len();
            push_u32(code, 0);
            error_branches.push(branch_offset);
        }
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_ldur_stur_imm9(base: u32, offset: usize, path: &str) -> Result<u32, StencilError> {
    let imm = i32::try_from(offset).map_err(|_| StencilError::Unsupported {
        path: path.to_owned(),
        reason: "stencil offset exceeds AArch64 imm9 range",
    })?;
    if !(-256..=255).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset exceeds AArch64 imm9 range",
        });
    }
    let imm9 = (imm as u32) & 0x1ff;
    Ok(base | (imm9 << 12))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_cond_branch_imm19(
    base: u32,
    branch_offset: usize,
    target: usize,
) -> Result<u32, StencilError> {
    let branch_offset = isize::try_from(branch_offset).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch offset exceeds isize",
    })?;
    let target = isize::try_from(target).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch target exceeds isize",
    })?;
    let delta = target
        .checked_sub(branch_offset)
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch offset overflow",
        })?;
    if delta % 4 != 0 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target is not instruction-aligned",
        });
    }
    let imm = delta / 4;
    if !(-(1 << 18)..=(1 << 18) - 1).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target exceeds AArch64 imm19 range",
        });
    }
    Ok(base | (((imm as u32) & 0x7ffff) << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_uncond_branch_imm26(
    base: u32,
    branch_offset: usize,
    target: usize,
) -> Result<u32, StencilError> {
    let branch_offset = isize::try_from(branch_offset).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch offset exceeds isize",
    })?;
    let target = isize::try_from(target).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch target exceeds isize",
    })?;
    let delta = target
        .checked_sub(branch_offset)
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch offset overflow",
        })?;
    if delta % 4 != 0 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target is not instruction-aligned",
        });
    }
    let imm = delta / 4;
    if !(-(1 << 25)..=(1 << 25) - 1).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target exceeds AArch64 imm26 range",
        });
    }
    Ok(base | ((imm as u32) & 0x03ff_ffff))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_test_bit_branch_imm14(
    base: u32,
    branch_offset: usize,
    target: usize,
) -> Result<u32, StencilError> {
    let branch_offset = isize::try_from(branch_offset).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch offset exceeds isize",
    })?;
    let target = isize::try_from(target).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch target exceeds isize",
    })?;
    let delta = target
        .checked_sub(branch_offset)
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch offset overflow",
        })?;
    if delta % 4 != 0 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target is not instruction-aligned",
        });
    }
    let imm = delta / 4;
    if !(-(1 << 13)..=(1 << 13) - 1).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target exceeds AArch64 imm14 range",
        });
    }
    Ok(base | (((imm as u32) & 0x3fff) << 5))
}

fn status_for_failure(index: usize) -> Result<u32, StencilError> {
    u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "too many stencil failure stubs",
        })
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_mov_x_immediate(code: &mut Vec<u8>, rd: u8, value: u64) -> Result<(), StencilError> {
    if rd > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    let rd = u32::from(rd);
    for shift_index in 0..4 {
        let imm = ((value >> (shift_index * 16)) & 0xffff) as u32;
        let word = if shift_index == 0 {
            0xD280_0000 | (imm << 5) | rd
        } else {
            0xF280_0000 | ((shift_index as u32) << 21) | (imm << 5) | rd
        };
        push_u32(code, word);
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mov_w0_immediate(value: u32) -> Result<u32, StencilError> {
    if value > u16::MAX as u32 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil status exceeds mov immediate range",
        });
    }
    Ok(0x5280_0000 | (value << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mov_w10_immediate(value: u32) -> Result<u32, StencilError> {
    if value > u16::MAX as u32 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil immediate exceeds mov immediate range",
        });
    }
    Ok(0x5280_000A | (value << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn cmp_w9_immediate(value: u32) -> Result<u32, StencilError> {
    if value > 0xfff {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil enum variant index exceeds cmp immediate range",
        });
    }
    Ok(0x7100_013F | (value << 10))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn push_u32(code: &mut Vec<u8>, word: u32) {
    code.extend_from_slice(&word.to_le_bytes());
}

struct ExecutableMemory {
    ptr: NonNull<u8>,
    len: usize,
}

// SAFETY: after construction the mapping is RX and immutable; Drop only unmaps
// once ownership ends.
unsafe impl Send for ExecutableMemory {}
// SAFETY: callers only execute immutable code bytes through shared references.
unsafe impl Sync for ExecutableMemory {}

impl ExecutableMemory {
    fn new(code: &[u8]) -> Result<Self, StencilError> {
        let len = code.len().max(1);
        let flags = libc::MAP_PRIVATE | libc::MAP_ANON | map_jit_flag();
        // SAFETY: mmap is called with a null hint, anonymous fd, and checked result.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(StencilError::ExecutableMemory);
        }

        // SAFETY: ptr is a valid writable mapping of at least len bytes.
        unsafe {
            copy_nonoverlapping(code.as_ptr(), ptr.cast::<u8>(), code.len());
            flush_instruction_cache(ptr, code.len());
            if libc::mprotect(ptr, len, libc::PROT_READ | libc::PROT_EXEC) != 0 {
                let _ = libc::munmap(ptr, len);
                return Err(StencilError::Mprotect);
            }
        }

        let Some(ptr) = NonNull::new(ptr.cast::<u8>()) else {
            // SAFETY: ptr/len are the live mapping returned by mmap.
            unsafe {
                let _ = libc::munmap(ptr, len);
            }
            return Err(StencilError::ExecutableMemory);
        };

        Ok(Self { ptr, len })
    }

    fn len(&self) -> usize {
        self.len
    }

    fn as_fixed_fn(&self) -> FixedStencilFn {
        // SAFETY: the mapping contains generated code ending in ret and has RX
        // permissions for the lifetime of self.
        unsafe { std::mem::transmute(self.ptr.as_ptr()) }
    }

    fn as_hybrid_fn(&self) -> HybridStencilFn {
        // SAFETY: the mapping contains generated code ending in ret and has RX
        // permissions for the lifetime of self.
        unsafe { std::mem::transmute(self.ptr.as_ptr()) }
    }

    fn as_encode_fn(&self) -> EncodeStencilFn {
        // SAFETY: the mapping contains generated code ending in ret and has RX
        // permissions for the lifetime of self.
        unsafe { std::mem::transmute(self.ptr.as_ptr()) }
    }
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        // SAFETY: ptr/len are the live mapping returned by mmap.
        unsafe {
            let _ = libc::munmap(self.ptr.as_ptr().cast::<c_void>(), self.len);
        }
    }
}

#[cfg(target_os = "macos")]
fn map_jit_flag() -> i32 {
    libc::MAP_JIT
}

#[cfg(not(target_os = "macos"))]
fn map_jit_flag() -> i32 {
    0
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn flush_instruction_cache(ptr: *mut c_void, len: usize) {
    unsafe extern "C" {
        fn sys_icache_invalidate(start: *mut c_void, len: usize);
    }
    // SAFETY: caller provides the writable code range that was just populated.
    unsafe { sys_icache_invalidate(ptr, len) };
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
unsafe fn flush_instruction_cache(_ptr: *mut c_void, _len: usize) {}
