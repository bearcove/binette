use std::ffi::c_void;
use std::mem::{align_of, size_of};

use crate::schema::TypeRef;

mod c_abi;
mod import;
pub use c_abi::{
    BINETTE_LOCAL_ACCESS_DIRECT, BINETTE_LOCAL_ACCESS_THUNK, BINETTE_LOCAL_BACKEND_RUST_FACET,
    BINETTE_LOCAL_BACKEND_SWIFT, BINETTE_LOCAL_KIND_ENUM, BINETTE_LOCAL_KIND_EXTERNAL_ATTACHMENT,
    BINETTE_LOCAL_KIND_OPAQUE, BINETTE_LOCAL_KIND_OPTION, BINETTE_LOCAL_KIND_SCALAR,
    BINETTE_LOCAL_KIND_SEQUENCE, BINETTE_LOCAL_KIND_STRUCT, BINETTE_LOCAL_OPTION_DIRECT_TAG,
    BINETTE_LOCAL_OPTION_NICHE, BINETTE_LOCAL_OPTION_THUNK, BINETTE_LOCAL_SCALAR_BYTES,
    BINETTE_LOCAL_SCALAR_PLAIN, BINETTE_LOCAL_SCALAR_STRING, BINETTE_LOCAL_SCHEMA_REF_POSITION,
    BINETTE_LOCAL_SCHEMA_REF_TYPE, BINETTE_LOCAL_SEQUENCE_DIRECT_CONTIGUOUS,
    BINETTE_LOCAL_SEQUENCE_INLINE_FIXED, BINETTE_LOCAL_SEQUENCE_THUNK, BinetteLocalAccessTag,
    BinetteLocalBackendAbi, BinetteLocalDescriptorAbi, BinetteLocalEnumAbi,
    BinetteLocalEnumTagAccessAbi, BinetteLocalEnumTagThunkAbi, BinetteLocalFieldAbi,
    BinetteLocalKindAbi, BinetteLocalKindTag, BinetteLocalLayoutAbi, BinetteLocalOptionAbi,
    BinetteLocalOptionRepresentationAbi, BinetteLocalOptionRepresentationTag,
    BinetteLocalOptionThunksAbi, BinetteLocalScalarAbi, BinetteLocalScalarTag,
    BinetteLocalSchemaRefAbi, BinetteLocalSchemaRefTag, BinetteLocalSequenceAbi,
    BinetteLocalSequenceStorageAbi, BinetteLocalSequenceStorageTag, BinetteLocalSequenceThunksAbi,
    BinetteLocalStrAbi, BinetteLocalStructAbi, BinetteLocalVariantAbi,
    BinetteLocalVariantConstructAbi, BinetteLocalVariantDropAbi,
    BinetteLocalVariantProjectAccessAbi, BinetteLocalVariantProjectIntoAbi,
    BinetteLocalVariantProjectThunkAbi, LocalDescriptorAbiError, LocalDescriptorAbiImport,
};
pub use import::{
    LocalAccessExport, LocalDescriptorExport, LocalDescriptorExportError,
    LocalDescriptorHandoffError, LocalDescriptorImport, LocalDescriptorImportError,
    LocalDescriptorImportKind, LocalFieldExport, LocalFieldImport, LocalKindExport,
    LocalLayoutExport, LocalStorageExport, LocalVariantExport, LocalVariantImport,
    local_descriptor_exports_from_json,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalBackend {
    RustFacet,
    SwiftProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalValueLayout {
    pub size: usize,
    pub align: usize,
    pub stride: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSchemaRef {
    Type(TypeRef),
    Position { owner: TypeRef, path: String },
}

// r[impl binette.local-access.descriptor+2]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTypeDescriptor {
    pub schema: LocalSchemaRef,
    pub backend: LocalBackend,
    pub layout: LocalValueLayout,
    pub kind: LocalTypeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalTypeKind {
    Scalar(LocalScalarAccess),
    Struct {
        fields: Vec<LocalFieldDescriptor>,
    },
    Enum {
        tag: LocalAccess,
        variants: Vec<LocalVariantDescriptor>,
    },
    Sequence {
        element: Box<LocalTypeDescriptor>,
        storage: LocalSequenceStorage,
    },
    Option {
        some: Box<LocalTypeDescriptor>,
        representation: LocalOptionRepresentation,
    },
    ExternalAttachment {
        kind: String,
    },
    Opaque {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalScalarAccess {
    Plain,
    String(LocalSequenceStorage),
    Bytes(LocalSequenceStorage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFieldDescriptor {
    pub name: String,
    pub access: LocalAccess,
    pub descriptor: Box<LocalTypeDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVariantDescriptor {
    pub name: String,
    pub index: u32,
    pub access: LocalAccess,
    pub project_into: Option<LocalThunk>,
    pub drop_projected: Option<LocalThunk>,
    pub construct: Option<LocalThunk>,
    pub payload: Option<Box<LocalTypeDescriptor>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalAccess {
    Direct { offset: usize },
    Thunk(LocalThunk),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalThunk {
    pub backend: LocalBackend,
    pub name: String,
}

pub type LocalSequenceLenThunk =
    unsafe extern "C" fn(value: *const u8, context: *mut c_void) -> usize;
pub type LocalSequenceU8Thunk =
    unsafe extern "C" fn(value: *const u8, index: usize, context: *mut c_void) -> u8;
pub type LocalSequenceElementPtrThunk =
    unsafe extern "C" fn(value: *const u8, index: usize, context: *mut c_void) -> *const u8;
pub type LocalSequenceWriteBytesThunk =
    unsafe extern "C" fn(value: *mut u8, ptr: *const u8, len: usize, context: *mut c_void) -> bool;
pub type LocalSequenceWriteFixedElementsThunk = unsafe extern "C" fn(
    value: *mut u8,
    ptr: *const u8,
    count: usize,
    element_stride: usize,
    context: *mut c_void,
) -> bool;
pub type LocalOptionIsSomeThunk =
    unsafe extern "C" fn(value: *const u8, context: *mut c_void) -> bool;
pub type LocalOptionSomeThunk =
    unsafe extern "C" fn(value: *const u8, context: *mut c_void) -> *const u8;
pub type LocalOptionWriteNoneThunk =
    unsafe extern "C" fn(value: *mut u8, context: *mut c_void) -> bool;
pub type LocalOptionWriteSomeBytesThunk =
    unsafe extern "C" fn(value: *mut u8, ptr: *const u8, len: usize, context: *mut c_void) -> bool;
pub type LocalEnumTagThunk = unsafe extern "C" fn(value: *const u8, context: *mut c_void) -> u32;
pub type LocalVariantProjectThunk =
    unsafe extern "C" fn(value: *const u8, context: *mut c_void) -> *const u8;
pub type LocalVariantProjectIntoThunk = unsafe extern "C" fn(
    value: *const u8,
    out: *mut u8,
    out_len: usize,
    context: *mut c_void,
) -> bool;
pub type LocalVariantDropProjectedThunk =
    unsafe extern "C" fn(value: *mut u8, context: *mut c_void);
pub type LocalVariantConstructThunk = unsafe extern "C" fn(
    value: *mut u8,
    payload: *const u8,
    payload_len: usize,
    context: *mut c_void,
) -> bool;

#[derive(Debug, Clone, Copy)]
pub struct LocalSequenceEncodeThunks {
    pub len: LocalSequenceLenThunk,
    pub element_u8: LocalSequenceU8Thunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalSequenceElementPtrEncodeThunks {
    pub len: LocalSequenceLenThunk,
    pub element_ptr: LocalSequenceElementPtrThunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalSequenceDecodeThunks {
    pub write_bytes: LocalSequenceWriteBytesThunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalSequenceFixedDecodeThunks {
    pub write_elements: LocalSequenceWriteFixedElementsThunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalOptionEncodeThunks {
    pub is_some: LocalOptionIsSomeThunk,
    pub some: LocalOptionSomeThunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalOptionSequenceDecodeThunks {
    pub write_none: LocalOptionWriteNoneThunk,
    pub write_some_bytes: LocalOptionWriteSomeBytesThunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalEnumTagThunks {
    pub tag: LocalEnumTagThunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalVariantProjectThunks {
    pub project: LocalVariantProjectThunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalVariantProjectIntoThunks {
    pub project_into: LocalVariantProjectIntoThunk,
    pub drop_projected: Option<LocalVariantDropProjectedThunk>,
    pub project_context: usize,
    pub drop_context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalVariantConstructThunks {
    pub construct: LocalVariantConstructThunk,
    pub context: usize,
}

#[derive(Debug, Clone)]
pub struct LocalSequenceThunkBinding {
    pub len: LocalThunk,
    pub element: LocalThunk,
    pub thunks: LocalSequenceEncodeThunks,
}

#[derive(Debug, Clone)]
pub struct LocalSequenceElementPtrThunkBinding {
    pub len: LocalThunk,
    pub element: LocalThunk,
    pub thunks: LocalSequenceElementPtrEncodeThunks,
}

#[derive(Debug, Clone)]
pub struct LocalSequenceDecodeThunkBinding {
    pub write: LocalThunk,
    pub thunks: LocalSequenceDecodeThunks,
}

#[derive(Debug, Clone)]
pub struct LocalSequenceFixedDecodeThunkBinding {
    pub write: LocalThunk,
    pub thunks: LocalSequenceFixedDecodeThunks,
}

#[derive(Debug, Clone)]
pub struct LocalOptionThunkBinding {
    pub is_some: LocalThunk,
    pub some: LocalThunk,
    pub thunks: LocalOptionEncodeThunks,
}

#[derive(Debug, Clone)]
pub struct LocalOptionSequenceDecodeThunkBinding {
    pub write_none: LocalThunk,
    pub write_some_bytes: LocalThunk,
    pub thunks: LocalOptionSequenceDecodeThunks,
}

#[derive(Debug, Clone)]
pub struct LocalEnumTagThunkBinding {
    pub tag: LocalThunk,
    pub thunks: LocalEnumTagThunks,
}

#[derive(Debug, Clone)]
pub struct LocalVariantProjectThunkBinding {
    pub project: LocalThunk,
    pub thunks: LocalVariantProjectThunks,
}

#[derive(Debug, Clone)]
pub struct LocalVariantProjectIntoThunkBinding {
    pub project_into: LocalThunk,
    pub drop_projected: Option<LocalThunk>,
    pub thunks: LocalVariantProjectIntoThunks,
}

#[derive(Debug, Clone)]
pub struct LocalVariantConstructThunkBinding {
    pub construct: LocalThunk,
    pub thunks: LocalVariantConstructThunks,
}

#[derive(Debug, Default, Clone)]
pub struct LocalThunkBindings {
    sequence_u8: Vec<LocalSequenceThunkBinding>,
    sequence_element_ptr: Vec<LocalSequenceElementPtrThunkBinding>,
    sequence_decode: Vec<LocalSequenceDecodeThunkBinding>,
    sequence_fixed_decode: Vec<LocalSequenceFixedDecodeThunkBinding>,
    option: Vec<LocalOptionThunkBinding>,
    option_sequence_decode: Vec<LocalOptionSequenceDecodeThunkBinding>,
    enum_tag: Vec<LocalEnumTagThunkBinding>,
    variant_project: Vec<LocalVariantProjectThunkBinding>,
    variant_project_into: Vec<LocalVariantProjectIntoThunkBinding>,
    variant_construct: Vec<LocalVariantConstructThunkBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSequenceStorage {
    InlineFixed {
        offset: usize,
        element_count: usize,
        element_stride: usize,
    },
    DirectContiguous {
        pointer: LocalAccess,
        length: LocalAccess,
        capacity: Option<LocalAccess>,
        element_stride: usize,
    },
    Thunk {
        len: LocalThunk,
        element: LocalThunk,
        write: Option<LocalThunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalOptionRepresentation {
    Tag {
        tag: LocalAccess,
        tag_width: usize,
        none_value: usize,
        some_value: usize,
        some: LocalAccess,
    },
    Niche {
        tag: LocalAccess,
        tag_width: usize,
        none_value: usize,
        none_bytes: Option<Vec<u8>>,
        some: LocalAccess,
    },
    Thunk {
        is_some: LocalThunk,
        some: LocalThunk,
        write_none: Option<LocalThunk>,
        write_some_bytes: Option<LocalThunk>,
    },
}

impl LocalValueLayout {
    pub fn new(size: usize, align: usize, stride: usize) -> Self {
        Self {
            size,
            align,
            stride,
        }
    }

    pub fn of<T>() -> Self {
        Self::new(size_of::<T>(), align_of::<T>(), size_of::<T>())
    }
}

impl LocalTypeDescriptor {
    pub fn new(
        schema: impl Into<LocalSchemaRef>,
        backend: LocalBackend,
        layout: LocalValueLayout,
        kind: LocalTypeKind,
    ) -> Self {
        Self {
            schema: schema.into(),
            backend,
            layout,
            kind,
        }
    }

    pub fn rust_facet(
        schema: impl Into<LocalSchemaRef>,
        layout: LocalValueLayout,
        kind: LocalTypeKind,
    ) -> Self {
        Self::new(schema, LocalBackend::RustFacet, layout, kind)
    }
}

impl From<TypeRef> for LocalSchemaRef {
    fn from(value: TypeRef) -> Self {
        Self::Type(value)
    }
}

impl LocalThunk {
    pub fn new(backend: LocalBackend, name: impl Into<String>) -> Self {
        Self {
            backend,
            name: name.into(),
        }
    }
}

impl LocalThunkBindings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sequence_u8(
        mut self,
        len: LocalThunk,
        element: LocalThunk,
        thunks: LocalSequenceEncodeThunks,
    ) -> Self {
        self.sequence_u8.push(LocalSequenceThunkBinding {
            len,
            element,
            thunks,
        });
        self
    }

    pub fn with_sequence_decode(
        mut self,
        write: LocalThunk,
        thunks: LocalSequenceDecodeThunks,
    ) -> Self {
        self.sequence_decode
            .push(LocalSequenceDecodeThunkBinding { write, thunks });
        self
    }

    pub fn with_sequence_fixed_decode(
        mut self,
        write: LocalThunk,
        thunks: LocalSequenceFixedDecodeThunks,
    ) -> Self {
        self.sequence_fixed_decode
            .push(LocalSequenceFixedDecodeThunkBinding { write, thunks });
        self
    }

    pub fn with_sequence_element_ptr(
        mut self,
        len: LocalThunk,
        element: LocalThunk,
        thunks: LocalSequenceElementPtrEncodeThunks,
    ) -> Self {
        self.sequence_element_ptr
            .push(LocalSequenceElementPtrThunkBinding {
                len,
                element,
                thunks,
            });
        self
    }

    pub fn with_option(
        mut self,
        is_some: LocalThunk,
        some: LocalThunk,
        thunks: LocalOptionEncodeThunks,
    ) -> Self {
        self.option.push(LocalOptionThunkBinding {
            is_some,
            some,
            thunks,
        });
        self
    }

    pub fn with_option_sequence_decode(
        mut self,
        write_none: LocalThunk,
        write_some_bytes: LocalThunk,
        thunks: LocalOptionSequenceDecodeThunks,
    ) -> Self {
        self.option_sequence_decode
            .push(LocalOptionSequenceDecodeThunkBinding {
                write_none,
                write_some_bytes,
                thunks,
            });
        self
    }

    pub fn with_enum_tag(mut self, tag: LocalThunk, thunks: LocalEnumTagThunks) -> Self {
        self.enum_tag.push(LocalEnumTagThunkBinding { tag, thunks });
        self
    }

    pub fn with_variant_project(
        mut self,
        project: LocalThunk,
        thunks: LocalVariantProjectThunks,
    ) -> Self {
        self.variant_project
            .push(LocalVariantProjectThunkBinding { project, thunks });
        self
    }

    pub fn with_variant_project_into(
        mut self,
        project_into: LocalThunk,
        drop_projected: Option<LocalThunk>,
        thunks: LocalVariantProjectIntoThunks,
    ) -> Self {
        self.variant_project_into
            .push(LocalVariantProjectIntoThunkBinding {
                project_into,
                drop_projected,
                thunks,
            });
        self
    }

    pub fn with_variant_construct(
        mut self,
        construct: LocalThunk,
        thunks: LocalVariantConstructThunks,
    ) -> Self {
        self.variant_construct
            .push(LocalVariantConstructThunkBinding { construct, thunks });
        self
    }

    pub fn sequence_u8(
        &self,
        len: &LocalThunk,
        element: &LocalThunk,
    ) -> Option<LocalSequenceEncodeThunks> {
        self.sequence_u8
            .iter()
            .find(|binding| &binding.len == len && &binding.element == element)
            .map(|binding| binding.thunks)
    }

    pub fn sequence_decode(&self, write: &LocalThunk) -> Option<LocalSequenceDecodeThunks> {
        self.sequence_decode
            .iter()
            .find(|binding| &binding.write == write)
            .map(|binding| binding.thunks)
    }

    pub fn sequence_fixed_decode(
        &self,
        write: &LocalThunk,
    ) -> Option<LocalSequenceFixedDecodeThunks> {
        self.sequence_fixed_decode
            .iter()
            .find(|binding| &binding.write == write)
            .map(|binding| binding.thunks)
    }

    pub fn sequence_element_ptr(
        &self,
        len: &LocalThunk,
        element: &LocalThunk,
    ) -> Option<LocalSequenceElementPtrEncodeThunks> {
        self.sequence_element_ptr
            .iter()
            .find(|binding| &binding.len == len && &binding.element == element)
            .map(|binding| binding.thunks)
    }

    pub fn option(
        &self,
        is_some: &LocalThunk,
        some: &LocalThunk,
    ) -> Option<LocalOptionEncodeThunks> {
        self.option
            .iter()
            .find(|binding| &binding.is_some == is_some && &binding.some == some)
            .map(|binding| binding.thunks)
    }

    pub fn option_sequence_decode(
        &self,
        write_none: &LocalThunk,
        write_some_bytes: &LocalThunk,
    ) -> Option<LocalOptionSequenceDecodeThunks> {
        self.option_sequence_decode
            .iter()
            .find(|binding| {
                &binding.write_none == write_none && &binding.write_some_bytes == write_some_bytes
            })
            .map(|binding| binding.thunks)
    }

    pub fn enum_tag(&self, tag: &LocalThunk) -> Option<LocalEnumTagThunks> {
        self.enum_tag
            .iter()
            .find(|binding| &binding.tag == tag)
            .map(|binding| binding.thunks)
    }

    pub fn variant_project(&self, project: &LocalThunk) -> Option<LocalVariantProjectThunks> {
        self.variant_project
            .iter()
            .find(|binding| &binding.project == project)
            .map(|binding| binding.thunks)
    }

    pub fn variant_project_into(
        &self,
        project_into: &LocalThunk,
        drop_projected: Option<&LocalThunk>,
    ) -> Option<LocalVariantProjectIntoThunks> {
        self.variant_project_into
            .iter()
            .find(|binding| {
                &binding.project_into == project_into
                    && binding.drop_projected.as_ref() == drop_projected
            })
            .map(|binding| binding.thunks)
    }

    pub fn variant_construct(&self, construct: &LocalThunk) -> Option<LocalVariantConstructThunks> {
        self.variant_construct
            .iter()
            .find(|binding| &binding.construct == construct)
            .map(|binding| binding.thunks)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod rust_layout {
    use std::alloc::{Layout as AllocLayout, alloc_zeroed, dealloc};
    use std::collections::HashMap;
    use std::ptr::NonNull;
    use std::slice;

    use super::*;
    use facet_core::{
        Def, EnumRepr, Facet, PtrConst, PtrMut, PtrUninit, Shape, StructKind, Type, UserType,
    };

    use crate::error::SchemaError;
    use crate::facet::schema_bundle_for_shape;
    use crate::hash::primitive_for_type_id;
    use crate::layout::{OptionStringLayout, VecLayout, option_string_layout, string_layout};
    use crate::schema::{Primitive, Schema, SchemaKind, TypeId, VariantPayload};

    #[derive(Debug, thiserror::Error)]
    pub enum LocalAccessError {
        #[error(transparent)]
        Schema(#[from] SchemaError),

        #[error("unsupported local access descriptor for {type_name}: {reason}")]
        Unsupported {
            type_name: &'static str,
            reason: &'static str,
        },
    }

    // r[impl binette.local-access.backends]
    // r[impl binette.local-access.descriptor+2]
    pub fn rust_facet_descriptor_for_shape(
        shape: &'static Shape,
    ) -> Result<LocalTypeDescriptor, LocalAccessError> {
        let bundle = schema_bundle_for_shape(shape)?;
        let schemas = bundle
            .schemas
            .iter()
            .map(|schema| (schema.id, schema))
            .collect::<HashMap<_, _>>();
        RustFacetDescriptorBuilder {
            schemas: &schemas,
            type_args: HashMap::new(),
            stack: Vec::new(),
        }
        .build(shape, &bundle.root)
    }

    // r[impl binette.local-access.backends]
    // r[impl binette.local-access.descriptor+2]
    pub fn rust_facet_descriptor_for<T: Facet<'static>>()
    -> Result<LocalTypeDescriptor, LocalAccessError> {
        rust_facet_descriptor_for_shape(T::SHAPE)
    }

    struct RustFacetDescriptorBuilder<'schema> {
        schemas: &'schema HashMap<TypeId, &'schema Schema>,
        type_args: HashMap<String, TypeRef>,
        stack: Vec<TypeId>,
    }

    impl RustFacetDescriptorBuilder<'_> {
        fn build(
            &mut self,
            shape: &'static Shape,
            type_ref: &TypeRef,
        ) -> Result<LocalTypeDescriptor, LocalAccessError> {
            let resolved = self.resolve_type_ref(type_ref)?;
            let (schema_type_params, kind) = self.schema_kind(&resolved)?;
            let layout = layout_for_shape(shape)?;

            if let TypeRef::Concrete { type_id, args } = &resolved {
                if self.stack.contains(type_id) {
                    return Ok(LocalTypeDescriptor::rust_facet(
                        resolved,
                        layout,
                        LocalTypeKind::Opaque {
                            reason: "recursive local descriptor edge".to_owned(),
                        },
                    ));
                }

                let previous_type_args = self.type_args.clone();
                if !schema_type_params.is_empty() {
                    self.type_args.clear();
                    for (name, arg) in schema_type_params.iter().zip(args) {
                        self.type_args.insert(name.clone(), arg.clone());
                    }
                }
                self.stack.push(*type_id);
                let local_kind = self.build_kind(shape, &resolved, &kind)?;
                self.stack.pop();
                self.type_args = previous_type_args;
                return Ok(LocalTypeDescriptor::rust_facet(
                    resolved, layout, local_kind,
                ));
            }

            Err(LocalAccessError::Unsupported {
                type_name: shape.type_identifier,
                reason: "unresolved local type variable",
            })
        }

        fn build_kind(
            &mut self,
            shape: &'static Shape,
            type_ref: &TypeRef,
            kind: &SchemaKind,
        ) -> Result<LocalTypeKind, LocalAccessError> {
            match kind {
                SchemaKind::Primitive(primitive) => Ok(LocalTypeKind::Scalar(
                    self.primitive_access(shape, *primitive)?,
                )),
                SchemaKind::Struct { fields, .. } => {
                    let shape_fields = struct_fields_for_shape(shape)?;
                    let fields = shape_fields
                        .iter()
                        .zip(fields)
                        .map(|(shape_field, schema_field)| {
                            let descriptor =
                                self.build(shape_field.shape.get(), &schema_field.type_ref)?;
                            Ok(LocalFieldDescriptor {
                                name: schema_field.name.clone(),
                                access: LocalAccess::Direct {
                                    offset: shape_field.offset,
                                },
                                descriptor: Box::new(descriptor),
                            })
                        })
                        .collect::<Result<Vec<_>, LocalAccessError>>()?;
                    Ok(LocalTypeKind::Struct { fields })
                }
                SchemaKind::Tuple { elements } => {
                    let shape_fields = struct_fields_for_shape(shape)?;
                    let fields = shape_fields
                        .iter()
                        .zip(elements)
                        .enumerate()
                        .map(|(index, (shape_field, type_ref))| {
                            let descriptor = self.build(shape_field.shape.get(), type_ref)?;
                            Ok(LocalFieldDescriptor {
                                name: index.to_string(),
                                access: LocalAccess::Direct {
                                    offset: shape_field.offset,
                                },
                                descriptor: Box::new(descriptor),
                            })
                        })
                        .collect::<Result<Vec<_>, LocalAccessError>>()?;
                    Ok(LocalTypeKind::Struct { fields })
                }
                SchemaKind::Array {
                    dimensions,
                    element,
                } => {
                    let Def::Array(array) = shape.def else {
                        return Err(LocalAccessError::Unsupported {
                            type_name: shape.type_identifier,
                            reason: "array schema does not match local array shape",
                        });
                    };
                    let count = dimensions_element_count(dimensions, shape.type_identifier)?;
                    if count != array.n {
                        return Err(LocalAccessError::Unsupported {
                            type_name: shape.type_identifier,
                            reason: "local array length differs from schema dimensions",
                        });
                    }
                    let element_shape = array.t();
                    let element = self.build(element_shape, element)?;
                    Ok(LocalTypeKind::Sequence {
                        element: Box::new(element),
                        storage: LocalSequenceStorage::InlineFixed {
                            offset: 0,
                            element_count: count,
                            element_stride: layout_for_shape(element_shape)?.stride,
                        },
                    })
                }
                SchemaKind::List { element } => {
                    let Def::List(list) = shape.def else {
                        return Err(LocalAccessError::Unsupported {
                            type_name: shape.type_identifier,
                            reason: "list schema does not match local list shape",
                        });
                    };
                    let element_shape = list.t();
                    let element = self.build(element_shape, element)?;
                    let element_stride = layout_for_shape(element_shape)?.stride;
                    let storage = rust_vec_storage(element_stride).unwrap_or_else(|| {
                        LocalSequenceStorage::Thunk {
                            len: LocalThunk::new(LocalBackend::RustFacet, "Facet.List.len"),
                            element: LocalThunk::new(LocalBackend::RustFacet, "Facet.List.element"),
                            write: None,
                        }
                    });
                    Ok(LocalTypeKind::Sequence {
                        element: Box::new(element),
                        storage,
                    })
                }
                SchemaKind::Option { element } => {
                    let Def::Option(option) = shape.def else {
                        return Err(LocalAccessError::Unsupported {
                            type_name: shape.type_identifier,
                            reason: "option schema does not match local option shape",
                        });
                    };
                    let element_shape = option.t();
                    let element = self.build(element_shape, element)?;
                    let representation = if matches!(
                        element.kind,
                        LocalTypeKind::Scalar(LocalScalarAccess::String(_))
                    ) {
                        rust_option_string_representation().unwrap_or_else(|| {
                            LocalOptionRepresentation::Thunk {
                                is_some: LocalThunk::new(
                                    LocalBackend::RustFacet,
                                    "Facet.Option.is_some",
                                ),
                                some: LocalThunk::new(LocalBackend::RustFacet, "Facet.Option.some"),
                                write_none: None,
                                write_some_bytes: None,
                            }
                        })
                    } else if let Some(representation) =
                        rust_option_direct_tag_representation(shape, element_shape, &element)
                    {
                        representation
                    } else {
                        LocalOptionRepresentation::Thunk {
                            is_some: LocalThunk::new(
                                LocalBackend::RustFacet,
                                "Facet.Option.is_some",
                            ),
                            some: LocalThunk::new(LocalBackend::RustFacet, "Facet.Option.some"),
                            write_none: None,
                            write_some_bytes: None,
                        }
                    };
                    Ok(LocalTypeKind::Option {
                        some: Box::new(element),
                        representation,
                    })
                }
                SchemaKind::Enum { variants, .. } => {
                    let shape_enum = enum_type_for_shape(shape)?;
                    let direct_u8_tag = shape_enum.enum_repr == EnumRepr::U8;
                    let variants = shape_enum
                        .variants
                        .iter()
                        .zip(variants)
                        .map(|(shape_variant, schema_variant)| {
                            let payload = match &schema_variant.payload {
                                VariantPayload::Unit => None,
                                VariantPayload::Newtype { type_ref } => shape_variant
                                    .data
                                    .fields
                                    .first()
                                    .map(|field| self.build(field.shape.get(), type_ref))
                                    .transpose()?
                                    .map(Box::new),
                                VariantPayload::Tuple { elements } => Some(Box::new(
                                    self.variant_fields_descriptor(
                                        shape,
                                        type_ref,
                                        schema_variant.name.as_str(),
                                        shape_variant.data.fields,
                                        elements
                                            .iter()
                                            .enumerate()
                                            .map(|(index, type_ref)| (index.to_string(), type_ref)),
                                    )?,
                                )),
                                VariantPayload::Struct { fields } => Some(Box::new(
                                    self.variant_fields_descriptor(
                                        shape,
                                        type_ref,
                                        schema_variant.name.as_str(),
                                        shape_variant.data.fields,
                                        fields
                                            .iter()
                                            .map(|field| (field.name.clone(), &field.type_ref)),
                                    )?,
                                )),
                            };
                            let (index, access) = if direct_u8_tag {
                                let discriminant = shape_variant.discriminant.ok_or({
                                    LocalAccessError::Unsupported {
                                        type_name: shape.type_identifier,
                                        reason: "repr(u8) enum variant is missing a discriminant",
                                    }
                                })?;
                                let index =
                                    u32::try_from(discriminant).map_err(|_| {
                                        LocalAccessError::Unsupported {
                                            type_name: shape.type_identifier,
                                            reason: "repr(u8) enum variant discriminant does not fit u32",
                                        }
                                    })?;
                                let access = match shape_variant.data.fields.first() {
                                    Some(field) => LocalAccess::Direct {
                                        offset: field.offset,
                                    },
                                    None => LocalAccess::Direct { offset: 0 },
                                };
                                (index, access)
                            } else {
                                (
                                    schema_variant.index,
                                    LocalAccess::Thunk(LocalThunk::new(
                                        LocalBackend::RustFacet,
                                        format!("Facet.Enum.{}", schema_variant.name),
                                    )),
                                )
                            };
                            Ok(LocalVariantDescriptor {
                                name: schema_variant.name.clone(),
                                index,
                                access,
                                project_into: None,
                                drop_projected: None,
                                construct: None,
                                payload,
                            })
                        })
                        .collect::<Result<Vec<_>, LocalAccessError>>()?;
                    Ok(LocalTypeKind::Enum {
                        tag: if direct_u8_tag {
                            LocalAccess::Direct { offset: 0 }
                        } else {
                            LocalAccess::Thunk(LocalThunk::new(
                                LocalBackend::RustFacet,
                                "Facet.Enum.discriminant",
                            ))
                        },
                        variants,
                    })
                }
                SchemaKind::External { kind, .. } => {
                    Ok(LocalTypeKind::ExternalAttachment { kind: kind.clone() })
                }
                SchemaKind::Dynamic | SchemaKind::Set { .. } | SchemaKind::Map { .. } => {
                    Ok(LocalTypeKind::Opaque {
                        reason:
                            "local descriptor lowering for this schema kind is not implemented yet"
                                .to_owned(),
                    })
                }
            }
        }

        fn variant_fields_descriptor<'a>(
            &mut self,
            owner_shape: &'static Shape,
            owner: &TypeRef,
            variant_name: &str,
            shape_fields: &'static [facet_core::Field],
            fields: impl Iterator<Item = (String, &'a TypeRef)>,
        ) -> Result<LocalTypeDescriptor, LocalAccessError> {
            let fields = shape_fields
                .iter()
                .zip(fields)
                .map(|(shape_field, (name, type_ref))| {
                    let descriptor = self.build(shape_field.shape.get(), type_ref)?;
                    Ok(LocalFieldDescriptor {
                        name,
                        access: LocalAccess::Direct {
                            offset: shape_field.offset,
                        },
                        descriptor: Box::new(descriptor),
                    })
                })
                .collect::<Result<Vec<_>, LocalAccessError>>()?;
            Ok(LocalTypeDescriptor::rust_facet(
                LocalSchemaRef::Position {
                    owner: owner.clone(),
                    path: format!("variant.{variant_name}"),
                },
                layout_for_shape(owner_shape)?,
                LocalTypeKind::Struct { fields },
            ))
        }

        fn primitive_access(
            &self,
            _shape: &'static Shape,
            primitive: Primitive,
        ) -> Result<LocalScalarAccess, LocalAccessError> {
            match primitive {
                Primitive::String => Ok(rust_string_storage()
                    .map(LocalScalarAccess::String)
                    .unwrap_or(LocalScalarAccess::Plain)),
                Primitive::Bytes | Primitive::Payload => Ok(rust_vec_storage(1)
                    .map(LocalScalarAccess::Bytes)
                    .unwrap_or(LocalScalarAccess::Plain)),
                _ => Ok(LocalScalarAccess::Plain),
            }
        }

        fn resolve_type_ref(&self, type_ref: &TypeRef) -> Result<TypeRef, LocalAccessError> {
            match type_ref {
                TypeRef::Concrete { type_id, args } => Ok(TypeRef::generic(
                    *type_id,
                    args.iter()
                        .map(|arg| self.resolve_type_ref(arg))
                        .collect::<Result<Vec<_>, _>>()?,
                )),
                TypeRef::Var { name } => {
                    self.type_args
                        .get(name)
                        .cloned()
                        .ok_or(LocalAccessError::Unsupported {
                            type_name: "<type parameter>",
                            reason: "unbound local descriptor type parameter",
                        })
                }
            }
        }

        fn schema_kind(
            &self,
            type_ref: &TypeRef,
        ) -> Result<(Vec<String>, SchemaKind), LocalAccessError> {
            let TypeRef::Concrete { type_id, .. } = type_ref else {
                return Err(LocalAccessError::Unsupported {
                    type_name: "<type parameter>",
                    reason: "schema lookup requires a concrete type reference",
                });
            };
            if let Some(schema) = self.schemas.get(type_id) {
                return Ok((schema.type_params.clone(), schema.kind.clone()));
            }
            if let Some(primitive) = primitive_for_type_id(*type_id) {
                return Ok((Vec::new(), SchemaKind::Primitive(primitive)));
            }
            Err(LocalAccessError::Unsupported {
                type_name: "<schema>",
                reason: "schema is not present in local descriptor bundle",
            })
        }
    }

    // r[impl binette.local-access.backends]
    // r[impl binette.local-access.runtime-facts]
    pub fn rust_string_descriptor(schema: TypeRef) -> Option<LocalTypeDescriptor> {
        let storage = rust_string_storage()?;
        Some(LocalTypeDescriptor::rust_facet(
            schema,
            LocalValueLayout::of::<String>(),
            LocalTypeKind::Scalar(LocalScalarAccess::String(storage)),
        ))
    }

    // r[impl binette.local-access.backends]
    // r[impl binette.local-access.runtime-facts]
    pub fn rust_vec_descriptor(
        schema: TypeRef,
        element: LocalTypeDescriptor,
        element_stride: usize,
    ) -> Option<LocalTypeDescriptor> {
        let storage = rust_vec_storage(element_stride)?;
        Some(LocalTypeDescriptor::rust_facet(
            schema,
            LocalValueLayout::of::<Vec<()>>(),
            LocalTypeKind::Sequence {
                element: Box::new(element),
                storage,
            },
        ))
    }

    // r[impl binette.local-access.backends]
    // r[impl binette.local-access.runtime-facts]
    pub fn rust_option_string_descriptor(schema: TypeRef) -> Option<LocalTypeDescriptor> {
        let option = option_string_layout()?;
        let string_schema = schema.clone();
        let string = rust_string_descriptor(string_schema)?;
        Some(LocalTypeDescriptor::rust_facet(
            schema,
            LocalValueLayout::of::<Option<String>>(),
            LocalTypeKind::Option {
                some: Box::new(string),
                representation: option_string_representation_from_layout(option),
            },
        ))
    }

    pub fn rust_string_storage() -> Option<LocalSequenceStorage> {
        Some(sequence_storage_from_vec_layout(string_layout()?, 1))
    }

    pub fn rust_vec_storage(element_stride: usize) -> Option<LocalSequenceStorage> {
        Some(sequence_storage_from_vec_layout(
            crate::layout::vec_layout()?,
            element_stride,
        ))
    }

    pub fn rust_option_string_representation() -> Option<LocalOptionRepresentation> {
        let layout = option_string_layout()?;
        layout
            .same_size_niche
            .then(|| option_string_representation_from_layout(layout))
    }

    pub fn rust_option_direct_tag_representation(
        option_shape: &'static Shape,
        element_shape: &'static Shape,
        element: &LocalTypeDescriptor,
    ) -> Option<LocalOptionRepresentation> {
        let Def::Option(option) = option_shape.def else {
            return None;
        };
        let option_layout = layout_for_shape(option_shape).ok()?;
        let element_layout = layout_for_shape(element_shape).ok()?;
        if let Some(representation) = rust_option_explicit_discriminant_representation(
            option_shape,
            &option_layout,
            &element_layout,
        ) {
            return Some(representation);
        }
        if let Some(representation) = rust_option_descriptor_niche_representation(
            option_shape,
            &option_layout,
            &element_layout,
            element,
        ) {
            return Some(representation);
        }
        if let Some(representation) = rust_option_niche_representation(option_shape, &option_layout)
        {
            return Some(representation);
        }
        let mut none = Scratch::new(option_layout.size, option_layout.align)?;
        let mut some = Scratch::new(option_layout.size, option_layout.align)?;
        let mut element = Scratch::new(element_layout.size, element_layout.align)?;

        unsafe {
            (option.vtable.init_none)(PtrUninit::new_sized(none.as_mut_ptr()));
            element_shape.call_default_in_place(PtrUninit::new_sized(element.as_mut_ptr()))?;
            (option.vtable.init_some)(
                PtrUninit::new_sized(some.as_mut_ptr()),
                PtrMut::new_sized(element.as_mut_ptr()),
            );
        }

        let some_ptr = unsafe { (option.vtable.get_value)(PtrConst::new_sized(some.as_mut_ptr())) };
        if some_ptr.is_null() {
            unsafe {
                option_shape.call_drop_in_place(PtrMut::new_sized(some.as_mut_ptr()));
                option_shape.call_drop_in_place(PtrMut::new_sized(none.as_mut_ptr()));
            }
            return None;
        }
        let some_base = some.as_mut_ptr() as usize;
        let some_ptr = some_ptr as usize;
        let some_offset = some_ptr.checked_sub(some_base)?;
        if element_layout.size == 0 {
            if some_offset > option_layout.size {
                unsafe {
                    option_shape.call_drop_in_place(PtrMut::new_sized(some.as_mut_ptr()));
                    option_shape.call_drop_in_place(PtrMut::new_sized(none.as_mut_ptr()));
                }
                return None;
            }
        } else if some_offset.checked_add(element_layout.size)? > option_layout.size {
            unsafe {
                option_shape.call_drop_in_place(PtrMut::new_sized(some.as_mut_ptr()));
                option_shape.call_drop_in_place(PtrMut::new_sized(none.as_mut_ptr()));
            }
            return None;
        }

        let none_bytes = unsafe { none.bytes(option_layout.size) };
        let some_bytes = unsafe { some.bytes(option_layout.size) };
        let payload_range = some_offset..some_offset.saturating_add(element_layout.size);
        let all_candidates = option_tag_candidates(none_bytes, some_bytes);
        let candidate = unique_option_tag_candidate(
            all_candidates
                .iter()
                .copied()
                .filter(|(offset, _, _)| !payload_range.contains(offset)),
        )
        .or_else(|| {
            (option_layout.size == element_layout.size)
                .then(|| unique_option_tag_candidate(all_candidates.iter().copied()))
                .flatten()
        });

        unsafe {
            option_shape.call_drop_in_place(PtrMut::new_sized(some.as_mut_ptr()));
            option_shape.call_drop_in_place(PtrMut::new_sized(none.as_mut_ptr()));
        }

        let (tag_offset, none_value, some_value) = candidate?;
        Some(LocalOptionRepresentation::Tag {
            tag: LocalAccess::Direct { offset: tag_offset },
            tag_width: 1,
            none_value: usize::from(none_value),
            some_value: usize::from(some_value),
            some: LocalAccess::Direct {
                offset: some_offset,
            },
        })
    }

    fn rust_option_explicit_discriminant_representation(
        option_shape: &'static Shape,
        option_layout: &LocalValueLayout,
        element_layout: &LocalValueLayout,
    ) -> Option<LocalOptionRepresentation> {
        let Type::User(UserType::Enum(enum_type)) = option_shape.ty else {
            return None;
        };
        if enum_type.enum_repr != EnumRepr::Rust || option_layout.size <= element_layout.size {
            return None;
        }
        let tag_width = option_layout.size.checked_sub(element_layout.size)?;
        if !matches!(tag_width, 1 | 2 | 4 | 8) {
            return None;
        }
        Some(LocalOptionRepresentation::Tag {
            tag: LocalAccess::Direct { offset: 0 },
            tag_width,
            none_value: 0,
            some_value: 1,
            some: LocalAccess::Direct { offset: tag_width },
        })
    }

    fn rust_option_descriptor_niche_representation(
        option_shape: &'static Shape,
        option_layout: &LocalValueLayout,
        element_layout: &LocalValueLayout,
        element: &LocalTypeDescriptor,
    ) -> Option<LocalOptionRepresentation> {
        if option_layout.size == 0 || option_layout.size != element_layout.size {
            return None;
        }

        let option_string = option_string_layout()?;
        let tag_width = size_of::<usize>();
        let candidates = descriptor_niche_tag_offsets(element, 0);
        let tag_offset = candidates.into_iter().find(|offset| {
            read_option_none_word(option_shape, option_layout, *offset)
                .is_some_and(|value| value == option_string.none_tag_value)
        })?;

        Some(LocalOptionRepresentation::Niche {
            tag: LocalAccess::Direct { offset: tag_offset },
            tag_width,
            none_value: option_string.none_tag_value,
            none_bytes: read_option_none_bytes(option_shape, option_layout),
            some: LocalAccess::Direct { offset: 0 },
        })
    }

    fn descriptor_niche_tag_offsets(descriptor: &LocalTypeDescriptor, base: usize) -> Vec<usize> {
        let mut offsets = Vec::new();
        collect_descriptor_niche_tag_offsets(descriptor, base, &mut offsets);
        offsets
    }

    fn collect_descriptor_niche_tag_offsets(
        descriptor: &LocalTypeDescriptor,
        base: usize,
        offsets: &mut Vec<usize>,
    ) {
        match &descriptor.kind {
            LocalTypeKind::Scalar(LocalScalarAccess::String(storage))
            | LocalTypeKind::Scalar(LocalScalarAccess::Bytes(storage))
            | LocalTypeKind::Sequence { storage, .. } => {
                collect_sequence_capacity_offset(storage, base, offsets);
            }
            LocalTypeKind::Struct { fields } => {
                for field in fields {
                    let LocalAccess::Direct { offset } = field.access else {
                        continue;
                    };
                    collect_descriptor_niche_tag_offsets(&field.descriptor, base + offset, offsets);
                }
            }
            _ => {}
        }
    }

    fn collect_sequence_capacity_offset(
        storage: &LocalSequenceStorage,
        base: usize,
        offsets: &mut Vec<usize>,
    ) {
        let LocalSequenceStorage::DirectContiguous {
            capacity:
                Some(LocalAccess::Direct {
                    offset: capacity_offset,
                }),
            ..
        } = storage
        else {
            return;
        };
        offsets.push(base + capacity_offset);
    }

    fn read_option_none_word(
        option_shape: &'static Shape,
        option_layout: &LocalValueLayout,
        offset: usize,
    ) -> Option<usize> {
        let Def::Option(option) = option_shape.def else {
            return None;
        };
        let width = size_of::<usize>();
        if offset.checked_add(width)? > option_layout.size {
            return None;
        }

        let mut none = Scratch::new(option_layout.size, option_layout.align)?;
        unsafe {
            (option.vtable.init_none)(PtrUninit::new_sized(none.as_mut_ptr()));
        }
        let bytes = unsafe { none.bytes(option_layout.size) };
        let value = bytes
            .get(offset..offset + width)
            .and_then(read_little_endian_usize);
        unsafe {
            option_shape.call_drop_in_place(PtrMut::new_sized(none.as_mut_ptr()));
        }
        value
    }

    fn read_option_none_bytes(
        option_shape: &'static Shape,
        option_layout: &LocalValueLayout,
    ) -> Option<Vec<u8>> {
        let Def::Option(option) = option_shape.def else {
            return None;
        };
        let mut none = Scratch::new(option_layout.size, option_layout.align)?;
        unsafe {
            (option.vtable.init_none)(PtrUninit::new_sized(none.as_mut_ptr()));
        }
        let bytes = unsafe { none.bytes(option_layout.size).to_vec() };
        unsafe {
            option_shape.call_drop_in_place(PtrMut::new_sized(none.as_mut_ptr()));
        }
        Some(bytes)
    }

    fn rust_option_niche_representation(
        option_shape: &'static Shape,
        option_layout: &LocalValueLayout,
    ) -> Option<LocalOptionRepresentation> {
        let Type::User(UserType::Enum(enum_type)) = option_shape.ty else {
            return None;
        };
        if enum_type.enum_repr != EnumRepr::RustNPO || option_layout.size == 0 {
            return None;
        }

        let Def::Option(option) = option_shape.def else {
            return None;
        };
        let mut none = Scratch::new(option_layout.size, option_layout.align)?;
        unsafe {
            (option.vtable.init_none)(PtrUninit::new_sized(none.as_mut_ptr()));
        }
        let none_bytes = unsafe { none.bytes(option_layout.size).to_vec() };
        let (tag_offset, tag_width, none_value) = unique_niche_none_tag(&none_bytes)?;
        unsafe {
            option_shape.call_drop_in_place(PtrMut::new_sized(none.as_mut_ptr()));
        }

        Some(LocalOptionRepresentation::Niche {
            tag: LocalAccess::Direct { offset: tag_offset },
            tag_width,
            none_value,
            none_bytes: Some(none_bytes),
            some: LocalAccess::Direct { offset: 0 },
        })
    }

    fn option_string_representation_from_layout(
        layout: OptionStringLayout,
    ) -> LocalOptionRepresentation {
        LocalOptionRepresentation::Niche {
            tag: LocalAccess::Direct {
                offset: layout.none_tag_offset,
            },
            tag_width: size_of::<usize>(),
            none_value: layout.none_tag_value,
            none_bytes: Some(layout.none_bytes),
            some: LocalAccess::Direct { offset: 0 },
        }
    }

    fn sequence_storage_from_vec_layout(
        layout: VecLayout,
        element_stride: usize,
    ) -> LocalSequenceStorage {
        LocalSequenceStorage::DirectContiguous {
            pointer: LocalAccess::Direct {
                offset: layout.ptr_offset,
            },
            length: LocalAccess::Direct {
                offset: layout.len_offset,
            },
            capacity: Some(LocalAccess::Direct {
                offset: layout.cap_offset,
            }),
            element_stride,
        }
    }

    struct Scratch {
        ptr: NonNull<u8>,
        layout: AllocLayout,
    }

    impl Scratch {
        fn new(size: usize, align: usize) -> Option<Self> {
            let layout = AllocLayout::from_size_align(size.max(1), align).ok()?;
            let ptr = unsafe { alloc_zeroed(layout) };
            let ptr = NonNull::new(ptr)?;
            Some(Self { ptr, layout })
        }

        fn as_mut_ptr(&mut self) -> *mut u8 {
            self.ptr.as_ptr()
        }

        unsafe fn bytes(&self, len: usize) -> &[u8] {
            unsafe { slice::from_raw_parts(self.ptr.as_ptr(), len) }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            unsafe { dealloc(self.ptr.as_ptr(), self.layout) };
        }
    }

    fn option_tag_candidates(none: &[u8], some: &[u8]) -> Vec<(usize, u8, u8)> {
        none.iter()
            .copied()
            .zip(some.iter().copied())
            .enumerate()
            .filter_map(|(offset, (none, some))| (none != some).then_some((offset, none, some)))
            .collect()
    }

    fn unique_option_tag_candidate(
        candidates: impl IntoIterator<Item = (usize, u8, u8)>,
    ) -> Option<(usize, u8, u8)> {
        let mut candidates = candidates.into_iter();
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    }

    fn unique_niche_none_tag(bytes: &[u8]) -> Option<(usize, usize, usize)> {
        [8usize, 4, 2, 1]
            .into_iter()
            .filter(|width| bytes.len() >= *width)
            .find_map(|width| {
                let mut candidates =
                    bytes
                        .chunks_exact(width)
                        .enumerate()
                        .filter_map(|(index, chunk)| {
                            let value = read_little_endian_usize(chunk)?;
                            (value != 0).then_some((index * width, width, value))
                        });
                let candidate = candidates.next()?;
                candidates.next().is_none().then_some(candidate)
            })
    }

    fn read_little_endian_usize(bytes: &[u8]) -> Option<usize> {
        match bytes.len() {
            1 => Some(usize::from(bytes[0])),
            2 => Some(usize::from(u16::from_le_bytes(bytes.try_into().ok()?))),
            4 => Some(u32::from_le_bytes(bytes.try_into().ok()?).try_into().ok()?),
            8 => usize::try_from(u64::from_le_bytes(bytes.try_into().ok()?)).ok(),
            _ => None,
        }
    }

    fn layout_for_shape(shape: &'static Shape) -> Result<LocalValueLayout, LocalAccessError> {
        let layout = shape
            .layout
            .sized_layout()
            .map_err(|_| LocalAccessError::Unsupported {
                type_name: shape.type_identifier,
                reason: "local descriptor shape is unsized",
            })?;
        Ok(LocalValueLayout::new(
            layout.size(),
            layout.align(),
            layout.size(),
        ))
    }

    fn struct_fields_for_shape(
        shape: &'static Shape,
    ) -> Result<&'static [facet_core::Field], LocalAccessError> {
        let Type::User(UserType::Struct(struct_type)) = shape.ty else {
            return Err(LocalAccessError::Unsupported {
                type_name: shape.type_identifier,
                reason: "local descriptor shape is not a struct",
            });
        };
        match struct_type.kind {
            StructKind::Struct | StructKind::TupleStruct | StructKind::Tuple => {
                Ok(struct_type.fields)
            }
            StructKind::Unit => Err(LocalAccessError::Unsupported {
                type_name: shape.type_identifier,
                reason: "unit struct local descriptor is not implemented yet",
            }),
        }
    }

    fn enum_type_for_shape(
        shape: &'static Shape,
    ) -> Result<facet_core::EnumType, LocalAccessError> {
        let Type::User(UserType::Enum(enum_type)) = shape.ty else {
            return Err(LocalAccessError::Unsupported {
                type_name: shape.type_identifier,
                reason: "local descriptor shape is not an enum",
            });
        };
        Ok(enum_type)
    }

    fn dimensions_element_count(
        dimensions: &[u64],
        type_name: &'static str,
    ) -> Result<usize, LocalAccessError> {
        dimensions.iter().try_fold(1usize, |count, dimension| {
            let dimension =
                usize::try_from(*dimension).map_err(|_| LocalAccessError::Unsupported {
                    type_name,
                    reason: "array dimension exceeds usize",
                })?;
            count
                .checked_mul(dimension)
                .ok_or(LocalAccessError::Unsupported {
                    type_name,
                    reason: "array dimension product overflows usize",
                })
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use rust_layout::{
    LocalAccessError, rust_facet_descriptor_for, rust_facet_descriptor_for_shape,
};
#[cfg(not(target_arch = "wasm32"))]
pub use rust_layout::{rust_option_string_descriptor, rust_string_descriptor, rust_vec_descriptor};
#[cfg(not(target_arch = "wasm32"))]
pub use rust_layout::{rust_option_string_representation, rust_string_storage, rust_vec_storage};

#[cfg(test)]
mod tests {
    use super::*;
    use facet::Facet;

    use crate::hash::primitive_type_id;
    use crate::schema::{Primitive, TypeId};

    #[derive(Facet)]
    struct DescriptorInner {
        code: u16,
        enabled: bool,
    }

    #[derive(Facet)]
    struct DescriptorOuter {
        id: u64,
        inner: DescriptorInner,
        values: [u16; 3],
        title: String,
    }

    #[derive(Facet)]
    #[allow(dead_code)]
    #[repr(u8)]
    enum DescriptorEvent {
        Empty,
        Count(u32),
        Named { label: String, code: u16 },
    }

    // r[verify binette.local-access.descriptor+2]
    #[test]
    fn descriptor_keeps_schema_backend_and_layout_separate() {
        let schema = TypeRef::concrete(primitive_type_id(Primitive::U8));
        let descriptor = LocalTypeDescriptor::rust_facet(
            schema.clone(),
            LocalValueLayout::of::<u8>(),
            LocalTypeKind::Scalar(LocalScalarAccess::Plain),
        );

        assert_eq!(descriptor.schema, LocalSchemaRef::Type(schema));
        assert_eq!(descriptor.backend, LocalBackend::RustFacet);
        assert_eq!(descriptor.layout, LocalValueLayout::of::<u8>());
    }

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.runtime-facts]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_string_probe_lowers_to_direct_sequence_access() {
        let descriptor =
            rust_string_descriptor(TypeRef::concrete(primitive_type_id(Primitive::String)))
                .expect("current Rust String layout is probeable");

        assert_eq!(descriptor.backend, LocalBackend::RustFacet);
        assert_eq!(descriptor.layout, LocalValueLayout::of::<String>());
        let LocalTypeKind::Scalar(LocalScalarAccess::String(
            LocalSequenceStorage::DirectContiguous {
                pointer,
                length,
                capacity,
                element_stride,
            },
        )) = descriptor.kind
        else {
            panic!("expected direct string storage");
        };

        assert!(matches!(pointer, LocalAccess::Direct { .. }));
        assert!(matches!(length, LocalAccess::Direct { .. }));
        assert!(matches!(capacity, Some(LocalAccess::Direct { .. })));
        assert_eq!(element_stride, 1);
    }

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.runtime-facts]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_vec_probe_lowers_to_direct_sequence_access_with_child_descriptor() {
        let element = LocalTypeDescriptor::rust_facet(
            TypeRef::concrete(primitive_type_id(Primitive::U8)),
            LocalValueLayout::of::<u8>(),
            LocalTypeKind::Scalar(LocalScalarAccess::Plain),
        );
        let descriptor = rust_vec_descriptor(TypeRef::concrete(TypeId(0xCAFE)), element.clone(), 1)
            .expect("current Rust Vec layout is probeable");

        let LocalTypeKind::Sequence {
            element: child,
            storage:
                LocalSequenceStorage::DirectContiguous {
                    pointer,
                    length,
                    capacity,
                    element_stride,
                },
        } = descriptor.kind
        else {
            panic!("expected direct vec storage");
        };

        assert_eq!(*child, element);
        assert!(matches!(pointer, LocalAccess::Direct { .. }));
        assert!(matches!(length, LocalAccess::Direct { .. }));
        assert!(matches!(capacity, Some(LocalAccess::Direct { .. })));
        assert_eq!(element_stride, 1);
    }

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.runtime-facts]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_option_string_probe_lowers_to_niche_descriptor() {
        let descriptor = rust_option_string_descriptor(TypeRef::concrete(TypeId(0x51_71_5A_7E)))
            .expect("current Rust Option<String> layout is probeable");

        assert_eq!(descriptor.layout, LocalValueLayout::of::<Option<String>>());
        let LocalTypeKind::Option {
            some,
            representation:
                LocalOptionRepresentation::Niche {
                    tag,
                    tag_width,
                    some: some_access,
                    ..
                },
        } = descriptor.kind
        else {
            panic!("expected niche option storage");
        };

        let LocalTypeKind::Scalar(LocalScalarAccess::String(string)) = some.kind else {
            panic!("expected string payload descriptor");
        };
        assert!(matches!(
            string,
            LocalSequenceStorage::DirectContiguous { .. }
        ));
        assert!(matches!(tag, LocalAccess::Direct { .. }));
        assert_eq!(tag_width, size_of::<usize>());
        assert_eq!(some_access, LocalAccess::Direct { offset: 0 });
    }

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.runtime-facts]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_option_bool_probe_lowers_to_direct_tag_descriptor() {
        let descriptor = rust_facet_descriptor_for::<Option<bool>>()
            .expect("current Rust Option<bool> layout is probeable");

        let LocalTypeKind::Option {
            some: some_descriptor,
            representation,
        } = descriptor.kind
        else {
            panic!("expected option storage");
        };

        assert!(matches!(
            some_descriptor.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::Plain)
        ));
        let LocalOptionRepresentation::Niche {
            tag,
            tag_width,
            none_value,
            some: some_access,
            ..
        } = representation
        else {
            panic!("expected direct niche option storage");
        };
        assert!(matches!(tag, LocalAccess::Direct { .. }));
        assert!(matches!(some_access, LocalAccess::Direct { .. }));
        assert_eq!(tag_width, 1);
        assert!(none_value <= usize::from(u8::MAX));
    }

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.runtime-facts]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_option_tuple_probe_lowers_to_direct_tag_descriptor() {
        type Value = Option<(u16, String)>;

        let descriptor =
            rust_facet_descriptor_for::<Value>().expect("current Rust Option layout is probeable");

        let LocalTypeKind::Option {
            some,
            representation:
                LocalOptionRepresentation::Niche {
                    tag,
                    tag_width,
                    some: some_access,
                    none_value,
                    ..
                },
        } = descriptor.kind
        else {
            panic!("expected direct niche option storage, got {descriptor:?}");
        };

        assert_eq!(some.layout, LocalValueLayout::of::<(u16, String)>());
        assert!(matches!(tag, LocalAccess::Direct { .. }));
        assert!(matches!(some_access, LocalAccess::Direct { .. }));
        assert!(matches!(tag_width, 1 | 2 | 4 | 8));
        assert_ne!(none_value, 0);
    }

    #[test]
    fn swift_backend_can_be_described_without_rust_facet() {
        let thunk = LocalThunk::new(LocalBackend::SwiftProbe, "SwiftArray.count");
        let descriptor = LocalTypeDescriptor::new(
            TypeRef::concrete(TypeId(0x57_1F_7A_11)),
            LocalBackend::SwiftProbe,
            LocalValueLayout::new(24, 8, 24),
            LocalTypeKind::Sequence {
                element: Box::new(LocalTypeDescriptor::new(
                    TypeRef::concrete(primitive_type_id(Primitive::I64)),
                    LocalBackend::SwiftProbe,
                    LocalValueLayout::of::<i64>(),
                    LocalTypeKind::Scalar(LocalScalarAccess::Plain),
                )),
                storage: LocalSequenceStorage::Thunk {
                    len: thunk.clone(),
                    element: LocalThunk::new(LocalBackend::SwiftProbe, "SwiftArray.element"),
                    write: None,
                },
            },
        );

        assert_eq!(descriptor.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Sequence {
            storage: LocalSequenceStorage::Thunk { len, .. },
            ..
        } = descriptor.kind
        else {
            panic!("expected Swift thunk-backed sequence");
        };
        assert_eq!(len, thunk);
    }

    // r[verify binette.local-access.swift-probes+2]
    // r[verify binette.local-access.descriptor+2]
    #[test]
    fn swift_probe_import_lowers_to_runtime_descriptor_tree() {
        let u8_schema = TypeRef::concrete(primitive_type_id(Primitive::U8));
        let i32_schema = TypeRef::concrete(primitive_type_id(Primitive::I32));
        let bool_schema = TypeRef::concrete(primitive_type_id(Primitive::Bool));
        let string_schema = TypeRef::concrete(primitive_type_id(Primitive::String));
        let leaf_schema = TypeRef::concrete(TypeId(0x5E_AE_00_01));
        let nested_schema = TypeRef::concrete(TypeId(0x5E_AE_00_02));
        let enum_schema = TypeRef::concrete(TypeId(0x5E_AE_00_03));

        let u8_descriptor = LocalDescriptorImport::swift_probe(
            u8_schema,
            LocalValueLayout::new(1, 1, 1),
            LocalDescriptorImportKind::Scalar(LocalScalarAccess::Plain),
        );
        let i32_descriptor = LocalDescriptorImport::swift_probe(
            i32_schema,
            LocalValueLayout::new(4, 4, 4),
            LocalDescriptorImportKind::Scalar(LocalScalarAccess::Plain),
        );
        let bool_descriptor = LocalDescriptorImport::swift_probe(
            bool_schema,
            LocalValueLayout::new(1, 1, 1),
            LocalDescriptorImportKind::Scalar(LocalScalarAccess::Plain),
        );
        let string_descriptor = LocalDescriptorImport::swift_probe(
            string_schema,
            LocalValueLayout::new(16, 8, 16),
            LocalDescriptorImportKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::Thunk {
                    len: LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.utf8.count"),
                    element: LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.utf8.element"),
                    write: Some(LocalThunk::new(
                        LocalBackend::SwiftProbe,
                        "Swift.String.init.utf8",
                    )),
                },
            )),
        );
        let leaf_descriptor = LocalDescriptorImport::swift_probe(
            leaf_schema.clone(),
            LocalValueLayout::new(8, 4, 8),
            LocalDescriptorImportKind::Struct {
                fields: vec![
                    LocalFieldImport {
                        name: "count".to_owned(),
                        access: LocalAccess::Direct { offset: 0 },
                        descriptor: i32_descriptor.clone(),
                    },
                    LocalFieldImport {
                        name: "flag".to_owned(),
                        access: LocalAccess::Direct { offset: 4 },
                        descriptor: bool_descriptor,
                    },
                ],
            },
        );
        let nested_descriptor = LocalDescriptorImport::swift_probe(
            nested_schema,
            LocalValueLayout::new(40, 8, 40),
            LocalDescriptorImportKind::Struct {
                fields: vec![
                    LocalFieldImport {
                        name: "title".to_owned(),
                        access: LocalAccess::Direct { offset: 0 },
                        descriptor: string_descriptor.clone(),
                    },
                    LocalFieldImport {
                        name: "leaf".to_owned(),
                        access: LocalAccess::Direct { offset: 16 },
                        descriptor: leaf_descriptor.clone(),
                    },
                    LocalFieldImport {
                        name: "values".to_owned(),
                        access: LocalAccess::Direct { offset: 24 },
                        descriptor: LocalDescriptorImport::swift_probe(
                            TypeRef::concrete(TypeId(0x5E_AE_00_04)),
                            LocalValueLayout::new(8, 8, 8),
                            LocalDescriptorImportKind::Sequence {
                                element: Box::new(u8_descriptor),
                                storage: LocalSequenceStorage::Thunk {
                                    len: LocalThunk::new(
                                        LocalBackend::SwiftProbe,
                                        "Swift.Array.count",
                                    ),
                                    element: LocalThunk::new(
                                        LocalBackend::SwiftProbe,
                                        "Swift.Array.element",
                                    ),
                                    write: Some(LocalThunk::new(
                                        LocalBackend::SwiftProbe,
                                        "Swift.Array.init.elements",
                                    )),
                                },
                            },
                        ),
                    },
                ],
            },
        );
        let enum_descriptor = LocalDescriptorImport::swift_probe(
            enum_schema,
            LocalValueLayout::new(24, 8, 24),
            LocalDescriptorImportKind::Enum {
                tag: LocalAccess::Thunk(LocalThunk::new(
                    LocalBackend::SwiftProbe,
                    "ProbeEnum.discriminant",
                )),
                variants: vec![
                    LocalVariantImport {
                        name: "empty".to_owned(),
                        index: 0,
                        access: LocalAccess::Thunk(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "ProbeEnum.project.empty",
                        )),
                        project_into: None,
                        drop_projected: None,
                        construct: Some(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "ProbeEnum.init.empty",
                        )),
                        payload: None,
                    },
                    LocalVariantImport {
                        name: "titled".to_owned(),
                        index: 1,
                        access: LocalAccess::Thunk(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "ProbeEnum.project.titled",
                        )),
                        project_into: None,
                        drop_projected: None,
                        construct: Some(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "ProbeEnum.init.titled.utf8",
                        )),
                        payload: Some(string_descriptor),
                    },
                    LocalVariantImport {
                        name: "nested".to_owned(),
                        index: 2,
                        access: LocalAccess::Thunk(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "ProbeEnum.project.nested",
                        )),
                        project_into: None,
                        drop_projected: None,
                        construct: Some(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "ProbeEnum.init.nested",
                        )),
                        payload: Some(leaf_descriptor),
                    },
                ],
            },
        );

        let nested = LocalTypeDescriptor::from_import(nested_descriptor).unwrap();
        let enum_descriptor = LocalTypeDescriptor::from_import(enum_descriptor).unwrap();

        assert_eq!(nested.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Struct { fields } = nested.kind else {
            panic!("expected imported Swift struct descriptor");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["title", "leaf", "values"]
        );
        assert!(matches!(
            fields[0].descriptor.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::Thunk { .. }
            ))
        ));
        assert!(matches!(
            fields[2].descriptor.kind,
            LocalTypeKind::Sequence {
                storage: LocalSequenceStorage::Thunk { .. },
                ..
            }
        ));

        let LocalTypeKind::Enum { tag, variants } = enum_descriptor.kind else {
            panic!("expected imported Swift enum descriptor");
        };
        assert!(matches!(tag, LocalAccess::Thunk(_)));
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            ["empty", "titled", "nested"]
        );
        assert!(variants[0].payload.is_none());
        assert!(matches!(
            variants[1].payload.as_deref().map(|payload| &payload.kind),
            Some(LocalTypeKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::Thunk { .. }
            )))
        ));
        assert!(matches!(
            variants[2].payload.as_deref().map(|payload| &payload.kind),
            Some(LocalTypeKind::Struct { .. })
        ));
    }

    // r[verify binette.local-access.swift-probes+2]
    // r[verify binette.local-access.descriptor+2]
    #[test]
    fn swift_probe_export_lowers_string_as_scalar_descriptor() {
        let export = LocalDescriptorExport {
            schema_name: "string".to_owned(),
            backend: "swift-probe".to_owned(),
            layout: LocalLayoutExport {
                size: 16,
                alignment: 8,
                stride: 16,
            },
            kind: LocalKindExport {
                tag: "string".to_owned(),
                fields: None,
                variants: None,
                element: None,
                some: None,
                storage: Some(LocalStorageExport {
                    tag: "thunk".to_owned(),
                    count: Some("Swift.String.utf8.count".to_owned()),
                    element: Some("Swift.String.utf8.element".to_owned()),
                    write: Some("Swift.String.init.utf8".to_owned()),
                    ..LocalStorageExport::default()
                }),
                reason: None,
            },
        };

        let descriptor =
            LocalTypeDescriptor::from_export(export, |schema_name| match schema_name {
                "string" => Some(LocalSchemaRef::Type(TypeRef::concrete(primitive_type_id(
                    Primitive::String,
                )))),
                _ => None,
            })
            .unwrap();

        assert_eq!(descriptor.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Scalar(LocalScalarAccess::String(LocalSequenceStorage::Thunk {
            len,
            element,
            write: Some(write),
        })) = descriptor.kind
        else {
            panic!("expected imported Swift string scalar descriptor");
        };
        assert_eq!(
            len,
            LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.utf8.count")
        );
        assert_eq!(
            element,
            LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.utf8.element")
        );
        assert_eq!(
            write,
            LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.init.utf8")
        );
    }

    // r[verify binette.local-access.swift-probes+2]
    // r[verify binette.local-access.descriptor+2]
    #[test]
    fn swift_probe_json_handoff_lowers_to_runtime_descriptor_tree() {
        let exports = local_descriptor_exports_from_json(include_str!(
            "../tests/fixtures/swift-probe-descriptors.json"
        ))
        .unwrap();
        assert_eq!(exports.len(), 12);
        let descriptors = exports
            .into_iter()
            .map(|export| {
                let name = export.schema_name.clone();
                LocalTypeDescriptor::from_export(export, resolve_swift_handoff_schema)
                    .map(|descriptor| (name, descriptor))
            })
            .collect::<Result<std::collections::HashMap<_, _>, _>>()
            .unwrap();
        let descriptor = descriptors.get("ProbeNested").unwrap();
        let option_descriptor = descriptors.get("option<string>").unwrap();
        let option_bool_descriptor = descriptors.get("option<bool>").unwrap();
        let option_u16_descriptor = descriptors.get("option<u16>").unwrap();
        let enum_descriptor = descriptors.get("ProbeEnum").unwrap();

        assert_eq!(descriptor.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Struct { fields } = &descriptor.kind else {
            panic!("expected Swift handoff root struct");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["title", "leaf", "values"]
        );
        assert_eq!(fields[0].access, LocalAccess::Direct { offset: 0 });
        assert!(matches!(
            fields[0].descriptor.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::Thunk { .. }
            ))
        ));
        assert!(matches!(
            fields[1].descriptor.kind,
            LocalTypeKind::Struct { .. }
        ));
        assert!(matches!(
            fields[2].descriptor.kind,
            LocalTypeKind::Sequence {
                storage: LocalSequenceStorage::Thunk { .. },
                ..
            }
        ));

        assert_eq!(option_descriptor.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Option {
            some,
            representation:
                LocalOptionRepresentation::Thunk {
                    is_some,
                    some: some_thunk,
                    write_none: Some(write_none),
                    write_some_bytes: Some(write_some_bytes),
                },
        } = &option_descriptor.kind
        else {
            panic!("expected Swift handoff option descriptor");
        };
        assert!(matches!(
            some.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::Thunk { .. }
            ))
        ));
        assert_eq!(
            *is_some,
            LocalThunk::new(LocalBackend::SwiftProbe, "Swift.Optional.isSome")
        );
        assert_eq!(
            *some_thunk,
            LocalThunk::new(LocalBackend::SwiftProbe, "Swift.Optional.some")
        );
        assert_eq!(
            *write_none,
            LocalThunk::new(LocalBackend::SwiftProbe, "Swift.Optional.init.none")
        );
        assert_eq!(
            *write_some_bytes,
            LocalThunk::new(
                LocalBackend::SwiftProbe,
                "Swift.Optional<String>.init.some.utf8"
            )
        );

        assert_eq!(option_bool_descriptor.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Option {
            some,
            representation:
                LocalOptionRepresentation::Niche {
                    tag,
                    tag_width,
                    none_value,
                    some: some_access,
                    ..
                },
        } = &option_bool_descriptor.kind
        else {
            panic!("expected Swift niche-tag option descriptor");
        };
        assert!(matches!(
            some.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::Plain)
        ));
        assert_eq!(*tag, LocalAccess::Direct { offset: 0 });
        assert_eq!(*tag_width, 1);
        assert_eq!(*none_value, 2);
        assert_eq!(*some_access, LocalAccess::Direct { offset: 0 });

        assert_eq!(option_u16_descriptor.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Option {
            some,
            representation:
                LocalOptionRepresentation::Tag {
                    tag,
                    tag_width,
                    none_value,
                    some_value,
                    some: some_access,
                },
        } = &option_u16_descriptor.kind
        else {
            panic!("expected Swift direct-tag option descriptor");
        };
        assert!(matches!(
            some.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::Plain)
        ));
        assert_eq!(*tag, LocalAccess::Direct { offset: 2 });
        assert_eq!(*tag_width, 1);
        assert_eq!(*none_value, 1);
        assert_eq!(*some_value, 0);
        assert_eq!(*some_access, LocalAccess::Direct { offset: 0 });

        assert_eq!(enum_descriptor.backend, LocalBackend::SwiftProbe);
        let LocalTypeKind::Enum { tag, variants } = &enum_descriptor.kind else {
            panic!("expected Swift handoff enum descriptor");
        };
        assert_eq!(
            *tag,
            LocalAccess::Thunk(LocalThunk::new(
                LocalBackend::SwiftProbe,
                "ProbeEnum.discriminant"
            ))
        );
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            ["empty", "titled", "nested"]
        );
        assert!(variants[0].payload.is_none());
        assert!(matches!(
            variants[1].payload.as_deref().map(|payload| &payload.kind),
            Some(LocalTypeKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::Thunk { .. }
            )))
        ));
        assert!(matches!(
            variants[2].payload.as_deref().map(|payload| &payload.kind),
            Some(LocalTypeKind::Struct { .. })
        ));
    }

    // r[verify binette.local-access.swift-probes+2]
    #[test]
    fn imported_descriptor_rejects_cross_backend_thunks() {
        let import = LocalDescriptorImport::swift_probe(
            TypeRef::concrete(primitive_type_id(Primitive::String)),
            LocalValueLayout::new(16, 8, 16),
            LocalDescriptorImportKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::Thunk {
                    len: LocalThunk::new(LocalBackend::RustFacet, "Facet.List.len"),
                    element: LocalThunk::new(LocalBackend::SwiftProbe, "Swift.String.utf8.element"),
                    write: None,
                },
            )),
        );

        let err = LocalTypeDescriptor::from_import(import).unwrap_err();
        assert!(matches!(
            err,
            LocalDescriptorImportError::BackendMismatch {
                expected: LocalBackend::SwiftProbe,
                actual: LocalBackend::RustFacet,
                ..
            }
        ));
    }

    fn resolve_swift_handoff_schema(schema_name: &str) -> Option<LocalSchemaRef> {
        let type_ref = match schema_name {
            "bool" => TypeRef::concrete(primitive_type_id(Primitive::Bool)),
            "u8" => TypeRef::concrete(primitive_type_id(Primitive::U8)),
            "u16" => TypeRef::concrete(primitive_type_id(Primitive::U16)),
            "i32" => TypeRef::concrete(primitive_type_id(Primitive::I32)),
            "i64" => TypeRef::concrete(primitive_type_id(Primitive::I64)),
            "u32" => TypeRef::concrete(primitive_type_id(Primitive::U32)),
            "string" => TypeRef::concrete(primitive_type_id(Primitive::String)),
            "ProbeLeaf" => TypeRef::concrete(TypeId(0x5E_AE_00_01)),
            "ProbeNested" => TypeRef::concrete(TypeId(0x5E_AE_00_02)),
            "ProbeEnum" => TypeRef::concrete(TypeId(0x5E_AE_00_03)),
            "array<i64>" => TypeRef::concrete(TypeId(0x5E_AE_00_04)),
            "option<string>" => TypeRef::concrete(TypeId(0x5E_AE_00_05)),
            "option<u16>" => TypeRef::concrete(TypeId(0x5E_AE_00_06)),
            "option<bool>" => TypeRef::concrete(TypeId(0x5E_AE_00_07)),
            _ => return None,
        };
        Some(LocalSchemaRef::Type(type_ref))
    }

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.descriptor+2]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_facet_descriptor_lowers_nested_struct_fields_and_arrays() {
        let descriptor = rust_facet_descriptor_for_shape(DescriptorOuter::SHAPE).unwrap();
        assert_eq!(descriptor.backend, LocalBackend::RustFacet);

        let LocalTypeKind::Struct { fields } = descriptor.kind else {
            panic!("expected struct descriptor");
        };

        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["id", "inner", "values", "title"]
        );
        assert_eq!(
            fields[0].access,
            LocalAccess::Direct {
                offset: std::mem::offset_of!(DescriptorOuter, id)
            }
        );
        assert_eq!(
            fields[1].access,
            LocalAccess::Direct {
                offset: std::mem::offset_of!(DescriptorOuter, inner)
            }
        );

        let LocalTypeKind::Struct {
            fields: inner_fields,
        } = &fields[1].descriptor.kind
        else {
            panic!("expected nested struct descriptor");
        };
        assert_eq!(
            inner_fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["code", "enabled"]
        );

        let LocalTypeKind::Sequence {
            element,
            storage:
                LocalSequenceStorage::InlineFixed {
                    element_count,
                    element_stride,
                    ..
                },
        } = &fields[2].descriptor.kind
        else {
            panic!("expected inline fixed array descriptor");
        };
        assert_eq!(*element_count, 3);
        assert_eq!(*element_stride, size_of::<u16>());
        assert!(matches!(
            element.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::Plain)
        ));

        assert!(matches!(
            fields[3].descriptor.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::String(
                LocalSequenceStorage::DirectContiguous { .. }
            ))
        ));
    }

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.descriptor+2]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_facet_descriptor_lowers_vec_and_enum_payloads() {
        let vec_descriptor = rust_facet_descriptor_for_shape(<Vec<u16>>::SHAPE).unwrap();
        let LocalTypeKind::Sequence {
            element,
            storage: LocalSequenceStorage::DirectContiguous { element_stride, .. },
        } = vec_descriptor.kind
        else {
            panic!("expected direct vec descriptor");
        };
        assert_eq!(element_stride, size_of::<u16>());
        assert!(matches!(
            element.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::Plain)
        ));

        let enum_descriptor = rust_facet_descriptor_for_shape(DescriptorEvent::SHAPE).unwrap();
        let LocalTypeKind::Enum { tag, variants } = enum_descriptor.kind else {
            panic!("expected enum descriptor");
        };
        assert_eq!(tag, LocalAccess::Direct { offset: 0 });
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            ["Empty", "Count", "Named"]
        );
        assert_eq!(variants[0].access, LocalAccess::Direct { offset: 0 });
        assert!(variants[0].payload.is_none());
        assert!(matches!(variants[1].access, LocalAccess::Direct { .. }));
        assert!(matches!(
            variants[1].payload.as_deref().map(|payload| &payload.kind),
            Some(LocalTypeKind::Scalar(LocalScalarAccess::Plain))
        ));
        assert!(matches!(
            variants[2].payload.as_deref().map(|payload| &payload.kind),
            Some(LocalTypeKind::Struct { .. })
        ));
        assert!(matches!(
            variants[2].payload.as_deref().map(|payload| &payload.schema),
            Some(LocalSchemaRef::Position { path, .. }) if path == "variant.Named"
        ));
    }
}
