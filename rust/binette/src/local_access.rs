use std::ffi::c_void;
use std::mem::{align_of, size_of};

use crate::hash::{primitive_for_type_id, primitive_type_id, schema_type_id};
use crate::schema::{
    Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef, Variant, VariantPayload,
};
use crate::schema_format::type_ref_to_value;
use crate::value::Value;

mod c_abi;
mod import;
pub use c_abi::{
    BINETTE_LOCAL_ACCESS_DIRECT, BINETTE_LOCAL_ACCESS_THUNK, BINETTE_LOCAL_BACKEND_RUST_FACET,
    BINETTE_LOCAL_BACKEND_SWIFT, BINETTE_LOCAL_EXTERNAL_METADATA_STRING,
    BINETTE_LOCAL_EXTERNAL_METADATA_STRUCT, BINETTE_LOCAL_EXTERNAL_METADATA_TYPE_REF,
    BINETTE_LOCAL_EXTERNAL_METADATA_UNIT, BINETTE_LOCAL_KIND_ENUM,
    BINETTE_LOCAL_KIND_EXTERNAL_ATTACHMENT, BINETTE_LOCAL_KIND_OPAQUE, BINETTE_LOCAL_KIND_OPTION,
    BINETTE_LOCAL_KIND_SCALAR, BINETTE_LOCAL_KIND_SEQUENCE, BINETTE_LOCAL_KIND_STRUCT,
    BINETTE_LOCAL_KIND_TUPLE, BINETTE_LOCAL_OPTION_DIRECT_TAG, BINETTE_LOCAL_OPTION_NICHE,
    BINETTE_LOCAL_OPTION_THUNK, BINETTE_LOCAL_SCALAR_BYTES, BINETTE_LOCAL_SCALAR_PLAIN,
    BINETTE_LOCAL_SCALAR_STRING, BINETTE_LOCAL_SCHEMA_REF_POSITION, BINETTE_LOCAL_SCHEMA_REF_TYPE,
    BINETTE_LOCAL_SEQUENCE_DIRECT_CONTIGUOUS, BINETTE_LOCAL_SEQUENCE_INLINE_FIXED,
    BINETTE_LOCAL_SEQUENCE_THUNK, BINETTE_LOCAL_VARIANT_PAYLOAD_NEWTYPE,
    BINETTE_LOCAL_VARIANT_PAYLOAD_STRUCT, BINETTE_LOCAL_VARIANT_PAYLOAD_TUPLE,
    BINETTE_LOCAL_VARIANT_PAYLOAD_UNIT, BinetteLocalAccessTag, BinetteLocalBackendAbi,
    BinetteLocalDescriptorAbi, BinetteLocalEnumAbi, BinetteLocalEnumTagAccessAbi,
    BinetteLocalEnumTagThunk, BinetteLocalEnumTagThunkAbi, BinetteLocalExternalAbi,
    BinetteLocalExternalMetadataAbi, BinetteLocalExternalMetadataFieldAbi,
    BinetteLocalExternalMetadataTag, BinetteLocalExternalMetadataValueAbi,
    BinetteLocalExternalMetadataValueTag, BinetteLocalFieldAbi, BinetteLocalKindAbi,
    BinetteLocalKindTag, BinetteLocalLayoutAbi, BinetteLocalOptionAbi,
    BinetteLocalOptionIsSomeThunk, BinetteLocalOptionRepresentationAbi,
    BinetteLocalOptionRepresentationTag, BinetteLocalOptionSomeThunk, BinetteLocalOptionThunksAbi,
    BinetteLocalOptionWriteNoneThunk, BinetteLocalOptionWriteSomeBytesThunk, BinetteLocalScalarAbi,
    BinetteLocalScalarTag, BinetteLocalSchemaRefAbi, BinetteLocalSchemaRefTag,
    BinetteLocalSequenceAbi, BinetteLocalSequenceElementDropProjectedThunk,
    BinetteLocalSequenceElementProjectIntoThunk, BinetteLocalSequenceElementPtrThunk,
    BinetteLocalSequenceLenThunk, BinetteLocalSequenceStorageAbi, BinetteLocalSequenceStorageTag,
    BinetteLocalSequenceThunksAbi, BinetteLocalSequenceU8Thunk,
    BinetteLocalSequenceWriteBytesThunk, BinetteLocalSequenceWriteFixedElementsThunk,
    BinetteLocalStrAbi, BinetteLocalStructAbi, BinetteLocalVariantAbi,
    BinetteLocalVariantConstructAbi, BinetteLocalVariantConstructThunk, BinetteLocalVariantDropAbi,
    BinetteLocalVariantDropProjectedThunk, BinetteLocalVariantPayloadKindTag,
    BinetteLocalVariantProjectAccessAbi, BinetteLocalVariantProjectIntoAbi,
    BinetteLocalVariantProjectIntoThunk, BinetteLocalVariantProjectThunk,
    BinetteLocalVariantProjectThunkAbi, LocalDescriptorAbiError, LocalDescriptorAbiImport,
};
pub use import::{
    LocalAccessExport, LocalDescriptorExport, LocalDescriptorExportError, LocalDescriptorImport,
    LocalDescriptorImportError, LocalDescriptorImportKind, LocalFieldExport, LocalFieldImport,
    LocalKindExport, LocalLayoutExport, LocalStorageExport, LocalVariantExport, LocalVariantImport,
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

#[derive(Debug, thiserror::Error)]
pub enum LocalSchemaBundleError {
    #[error("local descriptor schema reference cannot be used as a concrete type")]
    InvalidSchemaRef,

    #[error("local descriptor plain scalar does not name a primitive type")]
    InvalidPlainScalar,

    #[error("local opaque descriptors do not have a synthetic schema")]
    Opaque,

    #[error("failed to compute local descriptor schema id")]
    TypeId,
}

// r[impl binette.local-access.descriptor+2]
pub fn synthetic_schema_bundle_for_local_descriptor(
    descriptor: &mut LocalTypeDescriptor,
) -> Result<SchemaBundle, LocalSchemaBundleError> {
    let mut schemas = Vec::new();
    let root = canonicalize_descriptor_schema(descriptor, &mut schemas)?;
    Ok(SchemaBundle {
        schemas,
        root,
        attachments: Vec::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalTypeKind {
    Scalar(LocalScalarAccess),
    Struct {
        fields: Vec<LocalFieldDescriptor>,
    },
    Tuple {
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
        metadata: LocalExternalMetadata,
    },
    Opaque {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalExternalMetadata {
    Unit,
    Struct(Vec<LocalExternalMetadataField>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalExternalMetadataField {
    pub name: String,
    pub value: LocalExternalMetadataValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalExternalMetadataValue {
    String(String),
    TypeRef(Box<LocalTypeDescriptor>),
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
    pub payload_kind: LocalVariantPayloadKind,
    pub payload: Option<Box<LocalTypeDescriptor>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalVariantPayloadKind {
    Unit,
    Newtype,
    Tuple,
    Struct,
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
pub type LocalSequenceElementProjectIntoThunk = unsafe extern "C" fn(
    value: *const u8,
    index: usize,
    out: *mut u8,
    out_len: usize,
    context: *mut c_void,
) -> bool;
pub type LocalSequenceElementDropProjectedThunk =
    unsafe extern "C" fn(value: *mut u8, context: *mut c_void);
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
pub struct LocalSequenceElementProjectIntoEncodeThunks {
    pub len: LocalSequenceLenThunk,
    pub element_project_into: LocalSequenceElementProjectIntoThunk,
    pub element_drop_projected: Option<LocalSequenceElementDropProjectedThunk>,
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
pub struct LocalSequenceElementProjectIntoThunkBinding {
    pub len: LocalThunk,
    pub element: LocalThunk,
    pub thunks: LocalSequenceElementProjectIntoEncodeThunks,
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
    sequence_element_project_into: Vec<LocalSequenceElementProjectIntoThunkBinding>,
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

    pub fn with_sequence_element_project_into(
        mut self,
        len: LocalThunk,
        element: LocalThunk,
        thunks: LocalSequenceElementProjectIntoEncodeThunks,
    ) -> Self {
        self.sequence_element_project_into
            .push(LocalSequenceElementProjectIntoThunkBinding {
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

    pub fn sequence_element_project_into(
        &self,
        len: &LocalThunk,
        element: &LocalThunk,
    ) -> Option<LocalSequenceElementProjectIntoEncodeThunks> {
        self.sequence_element_project_into
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
                    Ok(LocalTypeKind::Tuple { fields })
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
                                payload_kind: if payload.is_some() {
                                    LocalVariantPayloadKind::Newtype
                                } else {
                                    LocalVariantPayloadKind::Unit
                                },
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
                SchemaKind::External { kind, .. } => Ok(LocalTypeKind::ExternalAttachment {
                    kind: kind.clone(),
                    metadata: LocalExternalMetadata::Unit,
                }),
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
            LocalTypeKind::Struct { fields } | LocalTypeKind::Tuple { fields } => {
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

fn canonicalize_descriptor_schema(
    descriptor: &mut LocalTypeDescriptor,
    schemas: &mut Vec<Schema>,
) -> Result<TypeRef, LocalSchemaBundleError> {
    let type_ref = match &mut descriptor.kind {
        LocalTypeKind::Scalar(LocalScalarAccess::Plain) => {
            let type_ref = descriptor_type_ref(descriptor)?;
            let TypeRef::Concrete { type_id, args } = &type_ref else {
                return Err(LocalSchemaBundleError::InvalidSchemaRef);
            };
            if !args.is_empty() || primitive_for_type_id(*type_id).is_none() {
                return Err(LocalSchemaBundleError::InvalidPlainScalar);
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
                .collect::<Result<Vec<_>, LocalSchemaBundleError>>()?;
            push_synthetic_schema(
                schemas,
                SchemaKind::Struct {
                    name: "local.struct".to_owned(),
                    fields,
                },
            )?
        }
        LocalTypeKind::Tuple { fields } => {
            let elements = fields
                .iter_mut()
                .map(|field| canonicalize_descriptor_schema(&mut field.descriptor, schemas))
                .collect::<Result<Vec<_>, LocalSchemaBundleError>>()?;
            push_synthetic_schema(schemas, SchemaKind::Tuple { elements })?
        }
        LocalTypeKind::Enum { variants, .. } => {
            let variants = variants
                .iter_mut()
                .map(|variant| {
                    let payload = match (&variant.payload_kind, &mut variant.payload) {
                        (LocalVariantPayloadKind::Unit, None) => VariantPayload::Unit,
                        (LocalVariantPayloadKind::Newtype, Some(payload)) => {
                            VariantPayload::Newtype {
                                type_ref: canonicalize_descriptor_schema(payload, schemas)?,
                            }
                        }
                        (LocalVariantPayloadKind::Tuple, Some(payload)) => {
                            canonicalize_tuple_variant_payload_descriptor(payload, schemas)?
                        }
                        (LocalVariantPayloadKind::Struct, Some(payload)) => {
                            canonicalize_struct_variant_payload_descriptor(payload, schemas)?
                        }
                        _ => return Err(LocalSchemaBundleError::InvalidSchemaRef),
                    };
                    Ok(Variant {
                        name: variant.name.clone(),
                        index: variant.index,
                        payload,
                    })
                })
                .collect::<Result<Vec<_>, LocalSchemaBundleError>>()?;
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
        LocalTypeKind::ExternalAttachment { kind, metadata } => {
            let metadata = canonicalize_external_metadata(metadata, schemas)?;
            push_synthetic_schema(
                schemas,
                SchemaKind::External {
                    kind: kind.clone(),
                    metadata,
                },
            )?
        }
        LocalTypeKind::Opaque { .. } => return Err(LocalSchemaBundleError::Opaque),
    };
    descriptor.schema = LocalSchemaRef::Type(type_ref.clone());
    Ok(type_ref)
}

fn canonicalize_tuple_variant_payload_descriptor(
    payload: &mut LocalTypeDescriptor,
    schemas: &mut Vec<Schema>,
) -> Result<VariantPayload, LocalSchemaBundleError> {
    let LocalTypeKind::Tuple { fields } = &mut payload.kind else {
        return Err(LocalSchemaBundleError::InvalidSchemaRef);
    };
    let elements = fields
        .iter_mut()
        .map(|field| canonicalize_descriptor_schema(&mut field.descriptor, schemas))
        .collect::<Result<Vec<_>, LocalSchemaBundleError>>()?;
    Ok(VariantPayload::Tuple { elements })
}

fn canonicalize_struct_variant_payload_descriptor(
    payload: &mut LocalTypeDescriptor,
    schemas: &mut Vec<Schema>,
) -> Result<VariantPayload, LocalSchemaBundleError> {
    let LocalTypeKind::Struct { fields } = &mut payload.kind else {
        return Err(LocalSchemaBundleError::InvalidSchemaRef);
    };
    let fields = fields
        .iter_mut()
        .map(|field| {
            Ok(Field {
                name: field.name.clone(),
                type_ref: canonicalize_descriptor_schema(&mut field.descriptor, schemas)?,
                required: true,
            })
        })
        .collect::<Result<Vec<_>, LocalSchemaBundleError>>()?;
    Ok(VariantPayload::Struct { fields })
}

fn canonicalize_external_metadata(
    metadata: &mut LocalExternalMetadata,
    schemas: &mut Vec<Schema>,
) -> Result<Value, LocalSchemaBundleError> {
    match metadata {
        LocalExternalMetadata::Unit => Ok(Value::Unit),
        LocalExternalMetadata::Struct(fields) => Ok(Value::Struct(
            fields
                .iter_mut()
                .map(|field| {
                    Ok(crate::value::FieldValue {
                        name: field.name.clone(),
                        value: canonicalize_external_metadata_value(&mut field.value, schemas)?,
                    })
                })
                .collect::<Result<Vec<_>, LocalSchemaBundleError>>()?,
        )),
    }
}

fn canonicalize_external_metadata_value(
    value: &mut LocalExternalMetadataValue,
    schemas: &mut Vec<Schema>,
) -> Result<Value, LocalSchemaBundleError> {
    match value {
        LocalExternalMetadataValue::String(value) => Ok(Value::String(value.clone())),
        LocalExternalMetadataValue::TypeRef(descriptor) => {
            let type_ref = canonicalize_descriptor_schema(descriptor, schemas)?;
            type_ref_to_value(&type_ref).map_err(|_| LocalSchemaBundleError::TypeId)
        }
    }
}

fn descriptor_type_ref(
    descriptor: &LocalTypeDescriptor,
) -> Result<TypeRef, LocalSchemaBundleError> {
    match &descriptor.schema {
        LocalSchemaRef::Type(type_ref) => Ok(type_ref.clone()),
        LocalSchemaRef::Position { .. } => Err(LocalSchemaBundleError::InvalidSchemaRef),
    }
}

fn push_synthetic_schema(
    schemas: &mut Vec<Schema>,
    kind: SchemaKind,
) -> Result<TypeRef, LocalSchemaBundleError> {
    let schema = schema_with_canonical_id(kind)?;
    let type_ref = TypeRef::concrete(schema.id);
    if !schemas.iter().any(|existing| existing.id == schema.id) {
        schemas.push(schema);
    }
    Ok(type_ref)
}

fn schema_with_canonical_id(kind: SchemaKind) -> Result<Schema, LocalSchemaBundleError> {
    let mut schema = Schema {
        id: TypeId(0),
        type_params: Vec::new(),
        kind,
    };
    schema.id = schema_type_id(&schema).map_err(|_| LocalSchemaBundleError::TypeId)?;
    Ok(schema)
}

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

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.descriptor+2]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rust_facet_tuple_descriptor_keeps_tuple_schema_kind() {
        type Args = (u16, String);

        let mut descriptor =
            rust_facet_descriptor_for::<Args>().expect("tuple descriptor should be representable");

        let LocalTypeKind::Tuple { fields } = &descriptor.kind else {
            panic!("expected tuple descriptor, got {descriptor:?}");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["0", "1"]
        );

        let bundle = synthetic_schema_bundle_for_local_descriptor(&mut descriptor)
            .expect("tuple descriptor should synthesize a schema bundle");
        let TypeRef::Concrete {
            type_id: root_id, ..
        } = bundle.root
        else {
            panic!("expected concrete root");
        };
        let root_schema = bundle
            .schemas
            .iter()
            .find(|schema| schema.id == root_id)
            .expect("root tuple schema should be present");
        let SchemaKind::Tuple { elements } = &root_schema.kind else {
            panic!("expected tuple schema, got {root_schema:?}");
        };
        assert_eq!(elements.len(), 2);
        assert_eq!(
            elements[0],
            TypeRef::concrete(primitive_type_id(Primitive::U16))
        );
        assert_eq!(
            elements[1],
            TypeRef::concrete(primitive_type_id(Primitive::String))
        );
    }

    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.value-model.external-form]
    #[test]
    fn external_attachment_descriptor_metadata_synthesizes_schema_value() {
        let mut descriptor = LocalTypeDescriptor::rust_facet(
            TypeRef::concrete(TypeId(0xB1_0000_0000_0001)),
            LocalValueLayout::new(0, 1, 1),
            LocalTypeKind::ExternalAttachment {
                kind: "vox.channel".to_owned(),
                metadata: LocalExternalMetadata::Struct(vec![
                    LocalExternalMetadataField {
                        name: "direction".to_owned(),
                        value: LocalExternalMetadataValue::String("tx".to_owned()),
                    },
                    LocalExternalMetadataField {
                        name: "element".to_owned(),
                        value: LocalExternalMetadataValue::TypeRef(Box::new(
                            LocalTypeDescriptor::rust_facet(
                                TypeRef::concrete(primitive_type_id(Primitive::U32)),
                                LocalValueLayout::of::<u32>(),
                                LocalTypeKind::Scalar(LocalScalarAccess::Plain),
                            ),
                        )),
                    },
                ]),
            },
        );

        let bundle = synthetic_schema_bundle_for_local_descriptor(&mut descriptor)
            .expect("external descriptor metadata should synthesize");
        let root_id = match bundle.root {
            TypeRef::Concrete { type_id, args } => {
                assert!(args.is_empty());
                type_id
            }
            TypeRef::Var { .. } => panic!("expected concrete root"),
        };
        let root_schema = bundle
            .schemas
            .iter()
            .find(|schema| schema.id == root_id)
            .expect("root external schema should be present");
        let SchemaKind::External { kind, metadata } = &root_schema.kind else {
            panic!("expected external schema, got {root_schema:?}");
        };
        assert_eq!(kind, "vox.channel");
        let Value::Struct(fields) = metadata else {
            panic!("expected external metadata struct, got {metadata:?}");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "direction");
        assert_eq!(fields[0].value, Value::String("tx".to_owned()));
        assert_eq!(fields[1].name, "element");
        assert_eq!(
            crate::schema_format::type_ref_from_value(&fields[1].value).unwrap(),
            TypeRef::concrete(primitive_type_id(Primitive::U32))
        );
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
                        payload_kind: LocalVariantPayloadKind::Unit,
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
                        payload_kind: LocalVariantPayloadKind::Newtype,
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
                        payload_kind: LocalVariantPayloadKind::Newtype,
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

    // r[verify binette.local-access.descriptor+2]
    #[test]
    fn synthetic_schema_bundle_accepts_sequence_of_unit_enum_descriptor() {
        let color_schema = TypeRef::concrete(TypeId(0x1B57_CC77_42FA_BEB0));
        let color = LocalDescriptorImport::swift_probe(
            color_schema,
            LocalValueLayout::new(1, 1, 1),
            LocalDescriptorImportKind::Enum {
                tag: LocalAccess::Thunk(LocalThunk::new(LocalBackend::SwiftProbe, "Color.tag")),
                variants: vec![
                    LocalVariantImport {
                        name: "Red".to_owned(),
                        index: 0,
                        access: LocalAccess::Thunk(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "Color.red",
                        )),
                        project_into: None,
                        drop_projected: None,
                        construct: None,
                        payload_kind: LocalVariantPayloadKind::Unit,
                        payload: None,
                    },
                    LocalVariantImport {
                        name: "Green".to_owned(),
                        index: 1,
                        access: LocalAccess::Thunk(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "Color.green",
                        )),
                        project_into: None,
                        drop_projected: None,
                        construct: None,
                        payload_kind: LocalVariantPayloadKind::Unit,
                        payload: None,
                    },
                ],
            },
        );
        let mut descriptor = LocalTypeDescriptor::from_import(LocalDescriptorImport::swift_probe(
            TypeRef::concrete(TypeId(0xCC0D_CE2D_2934_25BF)),
            LocalValueLayout::new(24, 8, 24),
            LocalDescriptorImportKind::Sequence {
                element: Box::new(color),
                storage: LocalSequenceStorage::Thunk {
                    len: LocalThunk::new(LocalBackend::SwiftProbe, "Array.len"),
                    element: LocalThunk::new(LocalBackend::SwiftProbe, "Array.element"),
                    write: None,
                },
            },
        ))
        .unwrap();

        let bundle = synthetic_schema_bundle_for_local_descriptor(&mut descriptor).unwrap();
        crate::schema_format::encode_schema_bundle_to_vec(&bundle).unwrap();
    }

    // r[verify binette.local-access.descriptor+2]
    #[test]
    fn synthetic_schema_bundle_accepts_empty_tuple_descriptor() {
        let mut descriptor = LocalTypeDescriptor::from_import(LocalDescriptorImport::swift_probe(
            TypeRef::concrete(TypeId(0x5AE7_E5BB_CAAD_E4A0)),
            LocalValueLayout::new(0, 1, 1),
            LocalDescriptorImportKind::Tuple { fields: Vec::new() },
        ))
        .unwrap();

        let bundle = synthetic_schema_bundle_for_local_descriptor(&mut descriptor).unwrap();
        crate::schema_format::encode_schema_bundle_to_vec(&bundle).unwrap();
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
