use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::{NonNull, copy_nonoverlapping};
use std::slice;

use facet_core::Facet;
use thiserror::Error;

use crate::compact::CompactError;
use crate::decode::DecodeError;
use crate::encode::{
    EncodeError, WriterFieldPlan, WriterNode, WriterPlan, WriterTupleElementPlan,
    WriterVariantPayloadPlan, WriterVariantPlan, writer_plan_for,
};
use crate::hash::primitive_for_type_id;
use crate::local_access::{
    LocalEnumTagThunks, LocalOptionEncodeThunks, LocalOptionSequenceDecodeThunks,
    LocalSequenceDecodeThunks, LocalSequenceElementProjectIntoEncodeThunks,
    LocalSequenceElementPtrEncodeThunks, LocalSequenceEncodeThunks, LocalSequenceFixedDecodeThunks,
    LocalThunkBindings, LocalTypeDescriptor, LocalValueLayout, LocalVariantConstructThunks,
    LocalVariantProjectIntoThunks, LocalVariantProjectThunks, rust_facet_descriptor_for,
    rust_facet_thunk_bindings_for,
};
use crate::plan::{
    EnumPayloadPlan, EnumVariantPlan, PlanError, PlanNode, ReaderPlan, StructFieldPlan,
    reader_plan_for,
};
use crate::registry::SchemaRegistry;
use crate::schema::{Primitive, SchemaKind, TypeId, TypeRef};

mod aarch64;
mod compile;
mod memory;
mod runtime;
#[cfg(all(test, target_arch = "aarch64", target_endian = "little"))]
mod tests;
mod types;

use self::aarch64::{
    generate_code, generate_direct_encode_code, generate_encode_code, generate_hybrid_code,
    status_for_failure,
};
use self::compile::{
    LocalDecodeStencilCompiler, LocalEncodeStencilCompiler, LocalHybridDecodeStencilCompiler,
};
use self::memory::ExecutableMemory;
use self::runtime::{
    STENCIL_OK, STENCIL_OPTION_NONE, STENCIL_OPTION_SOME, hybrid_error_status, stencil_copy_bytes,
    stencil_decode_helper, stencil_encode_helper, stencil_encode_reserve,
};
use self::types::{
    ByteTaggedLength, CopyOp, CopyWidth, DirectEnumDecodeCase, DirectOptionDecodeLayout,
    DirectSequenceDecodeLayout, EncodeBytesKind, EncodeBytesLayout, EncodeEnumCase,
    EncodeEnumSelector, EncodeListLayout, EncodeOptionLayout, EncodeStencilOp, EnumCase,
    FixedEncodeCompiler, FixedEncodeSegment, HybridStencilOp, LengthCheck, LocalEnumDecodeCase,
    LocalEnumDecodePayload, LocalEnumEncodeCase, LocalEnumEncodePayload, StencilEncodeHelper,
    StencilEncodeRuntime, StencilFailure, StencilHelper, StencilOp, StencilRuntime, TaggedLength,
};

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
type DirectEncodeStencilFn = unsafe extern "C" fn(value: *const u8, out: *mut Vec<u8>) -> u32;

pub struct StencilDecoder<T> {
    code: ExecutableMemory,
    entry: StencilEntry,
    failures: Vec<StencilFailure>,
    report: StencilReport,
    _marker: PhantomData<fn() -> T>,
}

pub struct StencilEncoder<T> {
    code: ExecutableMemory,
    entry: EncodeStencilEntry,
    report: StencilReport,
    _marker: PhantomData<fn() -> T>,
}

pub struct LocalStencilEncoder {
    code: ExecutableMemory,
    entry: LocalEncodeStencilEntry,
    report: StencilReport,
}

pub struct LocalStencilDecoder {
    code: ExecutableMemory,
    entry: LocalDecodeStencilEntry,
    failures: Vec<StencilFailure>,
    report: StencilReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StencilMode {
    Strict,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StencilReport {
    pub mode: StencilMode,
    pub code_len: usize,
    pub native_ops: usize,
    pub helper_count: usize,
    pub helper_paths: Vec<String>,
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

enum EncodeStencilEntry {
    Direct {
        func: DirectEncodeStencilFn,
    },
    Helper {
        func: EncodeStencilFn,
        runtime: Box<StencilEncodeRuntime>,
    },
}

enum LocalEncodeStencilEntry {
    Direct {
        func: DirectEncodeStencilFn,
    },
    Helper {
        func: EncodeStencilFn,
        runtime: Box<StencilEncodeRuntime>,
    },
}

enum LocalDecodeStencilEntry {
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

    #[error("invalid option tag byte {value:#04x} at {path} byte {position}")]
    InvalidOptionTag {
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

    pub fn report(&self) -> &StencilReport {
        &self.report
    }

    fn from_local(local: LocalStencilDecoder) -> Self {
        let entry = match local.entry {
            LocalDecodeStencilEntry::Fixed { func, length_check } => {
                StencilEntry::Fixed { func, length_check }
            }
            LocalDecodeStencilEntry::Hybrid { func, runtime } => {
                StencilEntry::Hybrid { func, runtime }
            }
        };
        Self {
            code: local.code,
            entry,
            failures: local.failures,
            report: local.report,
            _marker: PhantomData,
        }
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
                if status != STENCIL_OK {
                    return Err(failure_for_status(&self.failures, status, input));
                }
                // SAFETY: status zero means every supported output byte was written.
                unsafe { Ok(out.assume_init()) }
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
                    return Err(failure_for_status(&self.failures, status, input));
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
}

impl<T> StencilEncoder<T> {
    pub fn code_len(&self) -> usize {
        self.code.len()
    }

    pub fn report(&self) -> &StencilReport {
        &self.report
    }

    fn from_local(local: LocalStencilEncoder) -> Self {
        let entry = match local.entry {
            LocalEncodeStencilEntry::Direct { func } => EncodeStencilEntry::Direct { func },
            LocalEncodeStencilEntry::Helper { func, runtime } => {
                EncodeStencilEntry::Helper { func, runtime }
            }
        };
        Self {
            code: local.code,
            entry,
            report: local.report,
            _marker: PhantomData,
        }
    }
}

impl LocalStencilEncoder {
    pub fn code_len(&self) -> usize {
        self.code.len()
    }

    pub fn report(&self) -> &StencilReport {
        &self.report
    }

    /// Encode a local value through this descriptor-compiled strict stencil.
    ///
    /// # Safety
    ///
    /// `value` must point to a live object whose process-local layout matches
    /// the [`LocalTypeDescriptor`] used to build this encoder. The pointer must
    /// remain valid for the duration of the call.
    pub unsafe fn encode_raw_to_vec(&self, value: *const u8) -> Result<Vec<u8>, StencilError> {
        let mut out = Vec::new();
        let status = match &self.entry {
            LocalEncodeStencilEntry::Direct { func } => {
                // SAFETY: the caller promises that `value` points to a live
                // local value matching the descriptor used to compile this encoder.
                unsafe { func(value, &mut out) }
            }
            LocalEncodeStencilEntry::Helper { func, runtime } => {
                // SAFETY: the caller promises that `value` points to a live
                // local value matching the descriptor used to compile this encoder.
                unsafe { func(runtime.as_ref(), value, &mut out) }
            }
        };
        if status == STENCIL_OK {
            Ok(out)
        } else {
            Err(StencilError::UnknownStatus { status })
        }
    }
}

impl LocalStencilDecoder {
    pub fn expected_len(&self) -> usize {
        match &self.entry {
            LocalDecodeStencilEntry::Fixed { length_check, .. } => length_check
                .fixed_expected_len()
                .expect("local stencil decoder has variable input length"),
            LocalDecodeStencilEntry::Hybrid { .. } => {
                panic!("local hybrid stencil decoder has variable input length")
            }
        }
    }

    pub fn code_len(&self) -> usize {
        self.code.len()
    }

    pub fn report(&self) -> &StencilReport {
        &self.report
    }

    /// Decode compact bytes into a local value through this descriptor-compiled strict stencil.
    ///
    /// # Safety
    ///
    /// `out` must point to writable storage large enough for the
    /// [`LocalTypeDescriptor`] used to build this decoder. The storage must be
    /// valid to write for the duration of the call.
    pub unsafe fn decode_raw_into(&self, input: &[u8], out: *mut u8) -> Result<(), StencilError> {
        let consumed = unsafe { self.decode_raw_prefix_into(input, out) }?;
        if consumed != input.len() {
            return Err(StencilError::InputLength {
                expected: consumed,
                actual: input.len(),
            });
        }
        Ok(())
    }

    pub(super) unsafe fn decode_raw_prefix_into(
        &self,
        input: &[u8],
        out: *mut u8,
    ) -> Result<usize, StencilError> {
        match &self.entry {
            LocalDecodeStencilEntry::Fixed { func, length_check } => {
                length_check.validate(input)?;
                // SAFETY: the caller promises that `out` points to writable storage
                // matching the descriptor used to compile this decoder.
                let status = unsafe { func(input.as_ptr(), input.len(), out) };
                if status == STENCIL_OK {
                    Ok(input.len())
                } else {
                    Err(failure_for_status(&self.failures, status, input))
                }
            }
            LocalDecodeStencilEntry::Hybrid { func, runtime } => {
                // SAFETY: the caller promises that `out` points to writable storage
                // matching the descriptor used to compile this decoder.
                let result = unsafe { func(runtime.as_ref(), input.as_ptr(), input.len(), out) };
                if let Some(status) = hybrid_error_status(result) {
                    return Err(failure_for_status(&self.failures, status, input));
                }
                Ok(result)
            }
        }
    }
}

impl<T: Facet<'static>> StencilEncoder<T> {
    pub fn encode_to_vec(&self, value: &T) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        let status = match &self.entry {
            EncodeStencilEntry::Direct { func } => unsafe {
                func((value as *const T).cast(), &mut out)
            },
            EncodeStencilEntry::Helper { func, runtime } => unsafe {
                func(runtime.as_ref(), (value as *const T).cast(), &mut out)
            },
        };
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
pub fn strict_stencil_decoder_for<T: Facet<'static>>(
    writer_root: &TypeRef,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    let plan = reader_plan_for::<T>(writer_root, writer_registry)?;
    strict_stencil_decoder_from_plan(&plan, writer_registry)
}

// r[impl binette.compat.plan]
// r[impl binette.mode.compact]
pub fn hybrid_stencil_decoder_for<T: Facet<'static>>(
    writer_root: &TypeRef,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    let plan = reader_plan_for::<T>(writer_root, writer_registry)?;
    hybrid_stencil_decoder_from_plan(&plan, writer_registry)
}

// r[impl binette.compat.plan]
// r[impl binette.mode.compact]
pub fn stencil_decoder_from_plan<T: Facet<'static>>(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    hybrid_stencil_decoder_from_plan(plan, writer_registry)
}

// r[impl binette.compat.plan]
// r[impl binette.mode.compact]
pub fn strict_stencil_decoder_from_plan<T: Facet<'static>>(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    fixed_stencil_decoder_from_plan(plan, writer_registry)
}

// r[impl binette.compat.plan]
// r[impl binette.mode.compact]
pub fn hybrid_stencil_decoder_from_plan<T: Facet<'static>>(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
) -> Result<StencilDecoder<T>, StencilError> {
    let descriptor = rust_facet_descriptor_for::<T>().map_err(|_| StencilError::Unsupported {
        path: "$".to_owned(),
        reason: "failed to build Rust local access descriptor for stencil compilation",
    })?;
    let thunks = rust_facet_thunk_bindings_for::<T>();
    hybrid_local_stencil_decoder_from_plan(plan, writer_registry, &descriptor, &thunks)
        .map(StencilDecoder::from_local)
}

// r[impl binette.mode.compact]
pub fn stencil_encoder_for<T: Facet<'static>>() -> Result<StencilEncoder<T>, StencilError> {
    let plan = writer_plan_for::<T>()?;
    stencil_encoder_from_plan(&plan)
}

// r[impl binette.mode.compact]
pub fn strict_stencil_encoder_for<T: Facet<'static>>() -> Result<StencilEncoder<T>, StencilError> {
    let plan = writer_plan_for::<T>()?;
    strict_stencil_encoder_from_plan(&plan)
}

// r[impl binette.mode.compact]
pub fn hybrid_stencil_encoder_for<T: Facet<'static>>() -> Result<StencilEncoder<T>, StencilError> {
    let plan = writer_plan_for::<T>()?;
    hybrid_stencil_encoder_from_plan(&plan)
}

// r[impl binette.mode.compact]
pub fn stencil_encoder_from_plan<T: Facet<'static>>(
    plan: &WriterPlan,
) -> Result<StencilEncoder<T>, StencilError> {
    hybrid_stencil_encoder_from_plan(plan)
}

// r[impl binette.mode.compact]
pub fn strict_stencil_encoder_from_plan<T: Facet<'static>>(
    plan: &WriterPlan,
) -> Result<StencilEncoder<T>, StencilError> {
    strict_encode_stencil_encoder_from_plan(plan)
}

// r[impl binette.mode.compact]
pub fn hybrid_stencil_encoder_from_plan<T: Facet<'static>>(
    plan: &WriterPlan,
) -> Result<StencilEncoder<T>, StencilError> {
    let descriptor = rust_facet_descriptor_for::<T>().map_err(|_| StencilError::Unsupported {
        path: "$".to_owned(),
        reason: "failed to build Rust local access descriptor for stencil compilation",
    })?;
    let thunks = rust_facet_thunk_bindings_for::<T>();
    hybrid_local_stencil_encoder_from_plan(plan, &descriptor, &thunks)
        .map(StencilEncoder::from_local)
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.descriptor+2]
// r[impl binette.local-access.strict-hybrid]
// r[impl binette.mode.compact]
pub fn strict_local_stencil_encoder_from_plan(
    plan: &WriterPlan,
    descriptor: &LocalTypeDescriptor,
) -> Result<LocalStencilEncoder, StencilError> {
    if !matches!(&descriptor.schema, crate::local_access::LocalSchemaRef::Type(type_ref) if type_ref == plan.root())
    {
        return Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "local descriptor root schema differs from writer plan root",
        });
    }

    let mut fixed_compiler = FixedEncodeCompiler {
        ops: Vec::new(),
        output_offset: 0,
    };
    match fixed_compiler.compile_descriptor_root(descriptor, plan.root_node()) {
        Ok(output_len) => {
            let code = generate_direct_encode_code(&fixed_compiler.ops, output_len)?;
            let report = StencilReport {
                mode: StencilMode::Strict,
                code_len: code.len(),
                native_ops: fixed_compiler.ops.len(),
                helper_count: 0,
                helper_paths: Vec::new(),
            };
            let func = code.as_direct_encode_fn();
            return Ok(LocalStencilEncoder {
                code,
                entry: LocalEncodeStencilEntry::Direct { func },
                report,
            });
        }
        Err(err) if !matches!(&err, StencilError::Unsupported { .. }) => return Err(err),
        Err(_) => {}
    }

    let empty_thunks = LocalThunkBindings::new();
    let mut compiler = LocalEncodeStencilCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
        thunks: &empty_thunks,
    };
    compiler.compile_root(descriptor, plan.root_node())?;
    if !compiler.helpers.is_empty() {
        return Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "strict local encode stencil does not support helper fallbacks",
        });
    }

    let code = generate_encode_code(&compiler.ops)?;
    let report = StencilReport {
        mode: StencilMode::Strict,
        code_len: code.len(),
        native_ops: encode_native_op_count(&compiler.ops),
        helper_count: 0,
        helper_paths: Vec::new(),
    };
    let func = code.as_encode_fn();
    Ok(LocalStencilEncoder {
        code,
        entry: LocalEncodeStencilEntry::Helper {
            func,
            runtime: Box::new(StencilEncodeRuntime {
                helpers: compiler.helpers,
            }),
        },
        report,
    })
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.descriptor+2]
// r[impl binette.local-access.strict-hybrid]
// r[impl binette.mode.compact]
pub fn hybrid_local_stencil_encoder_from_plan(
    plan: &WriterPlan,
    descriptor: &LocalTypeDescriptor,
    thunks: &LocalThunkBindings,
) -> Result<LocalStencilEncoder, StencilError> {
    match strict_local_stencil_encoder_from_plan(plan, descriptor) {
        Ok(encoder) => return Ok(encoder),
        Err(err) if !matches!(&err, StencilError::Unsupported { .. }) => return Err(err),
        Err(_) => {}
    }

    if !matches!(&descriptor.schema, crate::local_access::LocalSchemaRef::Type(type_ref) if type_ref == plan.root())
    {
        return Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "local descriptor root schema differs from writer plan root",
        });
    }

    let mut compiler = LocalEncodeStencilCompiler {
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
        thunks,
    };
    compiler.compile_root(descriptor, plan.root_node())?;

    let code = generate_encode_code(&compiler.ops)?;
    let report = encode_report(&code, &compiler.ops, &compiler.helpers, &compiler.failures);
    let func = code.as_encode_fn();
    Ok(LocalStencilEncoder {
        code,
        entry: LocalEncodeStencilEntry::Helper {
            func,
            runtime: Box::new(StencilEncodeRuntime {
                helpers: compiler.helpers,
            }),
        },
        report,
    })
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.descriptor+2]
// r[impl binette.local-access.strict-hybrid]
// r[impl binette.mode.compact]
pub fn strict_local_stencil_decoder_from_plan(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
    descriptor: &LocalTypeDescriptor,
) -> Result<LocalStencilDecoder, StencilError> {
    if !matches!(&descriptor.schema, crate::local_access::LocalSchemaRef::Type(type_ref) if type_ref == plan.reader_root())
    {
        return Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "local descriptor root schema differs from reader plan root",
        });
    }

    let mut compiler = LocalDecodeStencilCompiler {
        writer_registry,
        plan_nodes: plan.nodes(),
        ops: Vec::new(),
        failures: Vec::new(),
        input_offset: 0,
    };
    let length_check = compiler.compile_root(descriptor, &plan.root)?;

    let code = generate_code(&compiler.ops, compiler.failures.len())?;
    let report = StencilReport {
        mode: StencilMode::Strict,
        code_len: code.len(),
        native_ops: fixed_decode_native_op_count(&compiler.ops),
        helper_count: 0,
        helper_paths: Vec::new(),
    };
    let func = code.as_fixed_fn();
    Ok(LocalStencilDecoder {
        code,
        entry: LocalDecodeStencilEntry::Fixed { func, length_check },
        failures: compiler.failures,
        report,
    })
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.descriptor+2]
// r[impl binette.local-access.strict-hybrid]
// r[impl binette.mode.compact]
pub fn hybrid_local_stencil_decoder_from_plan(
    plan: &ReaderPlan,
    writer_registry: &SchemaRegistry,
    descriptor: &LocalTypeDescriptor,
    thunks: &LocalThunkBindings,
) -> Result<LocalStencilDecoder, StencilError> {
    match strict_local_stencil_decoder_from_plan(plan, writer_registry, descriptor) {
        Ok(decoder) => return Ok(decoder),
        Err(err) if !matches!(&err, StencilError::Unsupported { .. }) => return Err(err),
        Err(_) => {}
    }

    if !matches!(&descriptor.schema, crate::local_access::LocalSchemaRef::Type(type_ref) if type_ref == plan.reader_root())
    {
        return Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "local descriptor root schema differs from reader plan root",
        });
    }

    let mut compiler = LocalHybridDecodeStencilCompiler {
        writer_registry,
        plan_nodes: plan.nodes(),
        ops: Vec::new(),
        helpers: Vec::new(),
        failures: Vec::new(),
        thunks,
    };
    compiler.compile_root(descriptor, &plan.root)?;

    let code = generate_hybrid_code(&compiler.ops)?;
    let report = decode_report(&code, &compiler.ops, &compiler.helpers, &compiler.failures);
    let func = code.as_hybrid_fn();
    Ok(LocalStencilDecoder {
        code,
        entry: LocalDecodeStencilEntry::Hybrid {
            func,
            runtime: Box::new(StencilRuntime {
                writer_registry: writer_registry.clone(),
                helpers: compiler.helpers,
            }),
        },
        failures: compiler.failures,
        report,
    })
}

fn fixed_encode_stencil_encoder_from_plan<T: Facet<'static>>(
    plan: &WriterPlan,
) -> Result<StencilEncoder<T>, StencilError> {
    let descriptor = rust_facet_descriptor_for::<T>().map_err(|_| StencilError::Unsupported {
        path: "$".to_owned(),
        reason: "failed to build Rust local access descriptor for stencil compilation",
    })?;
    strict_local_stencil_encoder_from_plan(plan, &descriptor).map(StencilEncoder::from_local)
}

fn strict_encode_stencil_encoder_from_plan<T: Facet<'static>>(
    plan: &WriterPlan,
) -> Result<StencilEncoder<T>, StencilError> {
    fixed_encode_stencil_encoder_from_plan(plan)
}

fn decode_report(
    code: &ExecutableMemory,
    ops: &[HybridStencilOp],
    helpers: &[StencilHelper],
    failures: &[StencilFailure],
) -> StencilReport {
    let helper_paths = decode_helper_paths(helpers, failures);
    StencilReport {
        mode: if helper_paths.is_empty() {
            StencilMode::Strict
        } else {
            StencilMode::Hybrid
        },
        code_len: code.len(),
        native_ops: hybrid_decode_native_op_count(ops),
        helper_count: helper_paths.len(),
        helper_paths,
    }
}

fn encode_report(
    code: &ExecutableMemory,
    ops: &[EncodeStencilOp],
    helpers: &[StencilEncodeHelper],
    failures: &[StencilFailure],
) -> StencilReport {
    let helper_paths = encode_helper_paths(helpers, failures);
    StencilReport {
        mode: if helper_paths.is_empty() {
            StencilMode::Strict
        } else {
            StencilMode::Hybrid
        },
        code_len: code.len(),
        native_ops: encode_native_op_count(ops),
        helper_count: helper_paths.len(),
        helper_paths,
    }
}

fn decode_helper_paths(helpers: &[StencilHelper], failures: &[StencilFailure]) -> Vec<String> {
    helpers
        .iter()
        .filter_map(|helper| match helper {
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
            | StencilHelper::Skip { failure_index, .. } => helper_path(failures, *failure_index),
        })
        .collect()
}

fn encode_helper_paths(
    helpers: &[StencilEncodeHelper],
    failures: &[StencilFailure],
) -> Vec<String> {
    helpers
        .iter()
        .filter_map(|helper| match helper {
            StencilEncodeHelper::SequenceBytes { failure_index, .. }
            | StencilEncodeHelper::SequenceFixedElements { failure_index, .. }
            | StencilEncodeHelper::SequenceOwnedFixedElements { failure_index, .. }
            | StencilEncodeHelper::SequenceProjectedElements { failure_index, .. }
            | StencilEncodeHelper::Enum { failure_index, .. }
            | StencilEncodeHelper::OptionSequenceBytes { failure_index, .. } => {
                helper_path(failures, *failure_index)
            }
        })
        .collect()
}

fn helper_path(failures: &[StencilFailure], failure_index: usize) -> Option<String> {
    match failures.get(failure_index) {
        Some(StencilFailure::Helper { path }) => Some(path.clone()),
        _ => None,
    }
}

fn fixed_decode_native_op_count(ops: &[StencilOp]) -> usize {
    ops.iter()
        .map(|op| match op {
            StencilOp::Copy(_) | StencilOp::Bool { .. } => 1,
            StencilOp::RootOption { body, .. } => 1 + fixed_decode_native_op_count(body),
            StencilOp::RootEnum { bodies, .. } => {
                1 + bodies
                    .iter()
                    .map(|body| fixed_decode_native_op_count(body))
                    .sum::<usize>()
            }
        })
        .sum()
}

fn hybrid_decode_native_op_count(ops: &[HybridStencilOp]) -> usize {
    ops.iter()
        .map(|op| match op {
            HybridStencilOp::Helper { .. } => 0,
            HybridStencilOp::Copy { .. } | HybridStencilOp::Bool { .. } => 1,
        })
        .sum()
}

fn failure_for_status(failures: &[StencilFailure], status: u32, input: &[u8]) -> StencilError {
    let Some(index) = status.checked_sub(1).map(|index| index as usize) else {
        return StencilError::UnknownStatus { status };
    };
    let Some(failure) = failures.get(index) else {
        return StencilError::UnknownStatus { status };
    };
    match failure {
        StencilFailure::InvalidBool { path, position } => StencilError::InvalidBool {
            path: path.clone(),
            position: *position,
            value: input[*position],
        },
        StencilFailure::InvalidOptionTag { path, position } => StencilError::InvalidOptionTag {
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

fn encode_native_op_count(ops: &[EncodeStencilOp]) -> usize {
    ops.iter()
        .map(|op| match op {
            EncodeStencilOp::Helper { .. } => 0,
            EncodeStencilOp::Direct { .. }
            | EncodeStencilOp::Bytes { .. }
            | EncodeStencilOp::Enum { .. }
            | EncodeStencilOp::Option { .. }
            | EncodeStencilOp::List { .. } => {
                1 + match op {
                    EncodeStencilOp::Enum { cases, .. } => cases
                        .iter()
                        .map(|case| encode_native_op_count(&case.ops))
                        .sum::<usize>(),
                    EncodeStencilOp::Option { some_ops, .. } => encode_native_op_count(some_ops),
                    EncodeStencilOp::List { element_ops, .. } => {
                        encode_native_op_count(element_ops)
                    }
                    _ => 0,
                }
            }
        })
        .sum()
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
    let descriptor = rust_facet_descriptor_for::<T>().map_err(|_| StencilError::Unsupported {
        path: "$".to_owned(),
        reason: "failed to build Rust local access descriptor for stencil compilation",
    })?;
    match strict_local_stencil_decoder_from_plan(plan, writer_registry, &descriptor) {
        Ok(decoder) => Ok(StencilDecoder::from_local(decoder)),
        Err(err) if !matches!(&err, StencilError::Unsupported { .. }) => Err(err),
        Err(err) => Err(err),
    }
}

pub fn decode_from_slice_with_stencil<T: Facet<'static>>(
    input: &[u8],
    decoder: &StencilDecoder<T>,
) -> Result<T, DecodeError> {
    decoder.decode(input).map_err(|err| match err {
        StencilError::InvalidBool {
            position, value, ..
        } => CompactError::InvalidBool { position, value }.into(),
        StencilError::InvalidOptionTag {
            position, value, ..
        } => CompactError::InvalidOptionTag { position, value }.into(),
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
                StencilError::InvalidOptionTag { .. } => unreachable!(),
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
