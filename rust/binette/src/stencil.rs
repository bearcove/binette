use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::{NonNull, copy_nonoverlapping};

use facet_core::{Facet, Shape, StructKind, Type};
use thiserror::Error;

use crate::decode::DecodeError;
use crate::hash::primitive_for_type_id;
use crate::plan::{PlanError, PlanNode, ReaderPlan, StructFieldPlan, reader_plan_for};
use crate::registry::SchemaRegistry;
use crate::schema::{Primitive, SchemaKind, TypeId, TypeRef};

type StencilFn = unsafe extern "C" fn(input: *const u8, len: usize, out: *mut u8);

pub struct StencilDecoder<T> {
    code: ExecutableMemory,
    func: StencilFn,
    expected_len: usize,
    _marker: PhantomData<fn() -> T>,
}

#[derive(Debug, Error)]
pub enum StencilError {
    #[error(transparent)]
    Plan(#[from] PlanError),

    #[error("unknown writer type id {type_id:?} at {path}")]
    UnknownWriterType { path: String, type_id: TypeId },

    #[error("unsupported stencil decode at {path}: {reason}")]
    Unsupported { path: String, reason: &'static str },

    #[error("expected {expected} bytes for stencil decode, got {actual}")]
    InputLength { expected: usize, actual: usize },

    #[error("failed to allocate executable stencil memory")]
    ExecutableMemory,

    #[error("failed to make stencil memory executable")]
    Mprotect,
}

impl<T> StencilDecoder<T> {
    pub fn expected_len(&self) -> usize {
        self.expected_len
    }

    pub fn code_len(&self) -> usize {
        self.code.len()
    }
}

impl<T: Facet<'static>> StencilDecoder<T> {
    pub fn decode(&self, input: &[u8]) -> Result<T, StencilError> {
        if input.len() != self.expected_len {
            return Err(StencilError::InputLength {
                expected: self.expected_len,
                actual: input.len(),
            });
        }

        let mut out = MaybeUninit::<T>::uninit();
        // SAFETY: the compiled stencil was built from T::SHAPE field offsets and
        // writes every supported field exactly once before returning.
        unsafe {
            (self.func)(input.as_ptr(), input.len(), out.as_mut_ptr().cast::<u8>());
            Ok(out.assume_init())
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
    let mut compiler = StencilCompiler {
        writer_registry,
        copy_ops: Vec::new(),
        input_offset: 0,
    };
    compiler.compile_root::<T>(&plan.root)?;

    let code = generate_code(&compiler.copy_ops)?;
    let expected_len = compiler.input_offset;
    let func = code.as_fn();
    Ok(StencilDecoder {
        code,
        func,
        expected_len,
        _marker: PhantomData,
    })
}

pub fn decode_from_slice_with_stencil<T: Facet<'static>>(
    input: &[u8],
    decoder: &StencilDecoder<T>,
) -> Result<T, DecodeError> {
    decoder
        .decode(input)
        .map_err(|err| DecodeError::Unsupported {
            position: 0,
            reason: match err {
                StencilError::InputLength { .. } => "stencil input length mismatch",
                StencilError::Unsupported { reason, .. } => reason,
                StencilError::UnknownWriterType { .. } => "stencil writer type is unknown",
                StencilError::Plan(_) => "stencil plan failed",
                StencilError::ExecutableMemory => "stencil executable memory allocation failed",
                StencilError::Mprotect => "stencil executable memory protection failed",
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

struct StencilCompiler<'registry> {
    writer_registry: &'registry SchemaRegistry,
    copy_ops: Vec<CopyOp>,
    input_offset: usize,
}

impl StencilCompiler<'_> {
    fn compile_root<T: Facet<'static>>(&mut self, root: &PlanNode) -> Result<(), StencilError> {
        match root {
            PlanNode::Direct { writer, .. } => self.compile_direct_root::<T>(writer, "$"),
            PlanNode::Struct { fields } => self.compile_struct_plan(T::SHAPE, fields, "$"),
            _ => Err(StencilError::Unsupported {
                path: "$".to_owned(),
                reason: "first stencil backend only supports scalar struct roots",
            }),
        }
    }

    fn compile_direct_root<T: Facet<'static>>(
        &mut self,
        writer: &TypeRef,
        path: &str,
    ) -> Result<(), StencilError> {
        if let Some(primitive) = primitive_for_plain_type_ref(writer) {
            let Some(widths) = primitive_widths(primitive) else {
                return Err(unsupported_primitive(path, primitive));
            };
            self.emit_primitive_copies(path, 0, widths)?;
            return Ok(());
        }

        let schema = self.schema_for(writer, path)?;
        let SchemaKind::Struct { fields, .. } = &schema.kind else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports scalar struct roots",
            });
        };
        let fields = fields.clone();
        let reader_fields = shape_struct_fields(T::SHAPE, path)?;
        if reader_fields.len() != fields.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct stencil struct field count differs from reader shape",
            });
        }

        for (index, field) in fields.iter().enumerate() {
            let output_offset = reader_fields[index].offset;
            self.compile_read_field(
                &field.type_ref,
                output_offset,
                &format!("{path}.{}", field.name),
            )?;
        }
        Ok(())
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct_plan(
        &mut self,
        reader_shape: &'static Shape,
        fields: &[StructFieldPlan],
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
                    let output_offset = reader_fields
                        .get(*reader_index)
                        .ok_or_else(|| StencilError::Unsupported {
                            path: format!("{path}.{name}"),
                            reason: "reader field index is out of range",
                        })?
                        .offset;
                    self.compile_read_plan(plan, output_offset, &format!("{path}.{name}"))?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.compile_skip(writer_type, &format!("{path}.{name}"))?,
            }
        }
        Ok(())
    }

    fn compile_read_plan(
        &mut self,
        node: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let PlanNode::Direct { writer, .. } = node else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports direct scalar fields",
            });
        };
        self.compile_read_field(writer, output_offset, path)
    }

    fn compile_read_field(
        &mut self,
        type_ref: &TypeRef,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let primitive =
            primitive_for_plain_type_ref(type_ref).ok_or_else(|| StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports primitive scalar fields",
            })?;
        let Some(widths) = primitive_widths(primitive) else {
            return Err(unsupported_primitive(path, primitive));
        };
        self.emit_primitive_copies(path, output_offset, widths)
    }

    fn compile_skip(&mut self, type_ref: &TypeRef, path: &str) -> Result<(), StencilError> {
        let primitive =
            primitive_for_plain_type_ref(type_ref).ok_or_else(|| StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports primitive scalar skips",
            })?;
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
            self.copy_ops.push(CopyOp {
                input_offset: self.input_offset,
                output_offset,
                width: *width,
            });
            self.input_offset = checked_offset(self.input_offset, width.bytes(), path)?;
            output_offset = checked_offset(output_offset, width.bytes(), path)?;
        }
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
    let facet_core::UserType::Struct(struct_type) = user else {
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

fn checked_offset(offset: usize, width: usize, path: &str) -> Result<usize, StencilError> {
    offset
        .checked_add(width)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset overflow",
        })
}

fn generate_code(copy_ops: &[CopyOp]) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(copy_ops.len() * 8 + 4);
        for op in copy_ops {
            emit_copy_op(&mut code, *op)?;
        }
        push_u32(&mut code, AARCH64_RET);
        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = copy_ops;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_RET: u32 = 0xD65F_03C0;

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

    fn as_fn(&self) -> StencilFn {
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
