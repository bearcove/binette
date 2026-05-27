use std::ffi::c_void;
use std::slice;
use std::str;

use super::{
    LocalAccess, LocalBackend, LocalDescriptorImport, LocalDescriptorImportKind,
    LocalEnumTagThunks, LocalOptionRepresentation, LocalOptionSequenceDecodeThunks,
    LocalScalarAccess, LocalSequenceDecodeThunks, LocalSequenceElementPtrEncodeThunks,
    LocalSequenceEncodeThunks, LocalSequenceFixedDecodeThunks, LocalSequenceStorage, LocalThunk,
    LocalThunkBindings, LocalTypeDescriptor, LocalValueLayout, LocalVariantConstructThunks,
    LocalVariantProjectThunks,
};
use crate::schema::{TypeId, TypeRef};

#[derive(Debug)]
pub struct LocalDescriptorAbiImport {
    pub descriptor: LocalTypeDescriptor,
    pub thunks: LocalThunkBindings,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalDescriptorAbiError {
    #[error("null local descriptor ABI pointer at {path}")]
    NullDescriptor { path: String },

    #[error("null local descriptor ABI pointer for {field} at {path}")]
    NullPointer { path: String, field: &'static str },

    #[error("invalid local descriptor ABI tag {tag} for {field} at {path}")]
    InvalidTag {
        path: String,
        field: &'static str,
        tag: u32,
    },

    #[error("invalid local descriptor ABI UTF-8 for {field} at {path}")]
    InvalidUtf8 { path: String, field: &'static str },

    #[error("missing local descriptor ABI thunk {field} at {path}")]
    MissingThunk { path: String, field: &'static str },

    #[error(transparent)]
    Import(#[from] super::LocalDescriptorImportError),
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalStrAbi {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalLayoutAbi {
    pub size: usize,
    pub align: usize,
    pub stride: usize,
}

pub type BinetteLocalBackendAbi = u32;
pub const BINETTE_LOCAL_BACKEND_RUST_FACET: BinetteLocalBackendAbi = 1;
pub const BINETTE_LOCAL_BACKEND_SWIFT: BinetteLocalBackendAbi = 2;

pub type BinetteLocalSchemaRefTag = u32;
pub const BINETTE_LOCAL_SCHEMA_REF_TYPE: BinetteLocalSchemaRefTag = 1;
pub const BINETTE_LOCAL_SCHEMA_REF_POSITION: BinetteLocalSchemaRefTag = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalSchemaRefAbi {
    pub tag: BinetteLocalSchemaRefTag,
    pub type_id: u64,
    pub owner_type_id: u64,
    pub path: BinetteLocalStrAbi,
}

pub type BinetteLocalKindTag = u32;
pub const BINETTE_LOCAL_KIND_SCALAR: BinetteLocalKindTag = 1;
pub const BINETTE_LOCAL_KIND_STRUCT: BinetteLocalKindTag = 2;
pub const BINETTE_LOCAL_KIND_ENUM: BinetteLocalKindTag = 3;
pub const BINETTE_LOCAL_KIND_SEQUENCE: BinetteLocalKindTag = 4;
pub const BINETTE_LOCAL_KIND_OPTION: BinetteLocalKindTag = 5;
pub const BINETTE_LOCAL_KIND_EXTERNAL_ATTACHMENT: BinetteLocalKindTag = 6;
pub const BINETTE_LOCAL_KIND_OPAQUE: BinetteLocalKindTag = 7;

pub type BinetteLocalScalarTag = u32;
pub const BINETTE_LOCAL_SCALAR_PLAIN: BinetteLocalScalarTag = 1;
pub const BINETTE_LOCAL_SCALAR_STRING: BinetteLocalScalarTag = 2;
pub const BINETTE_LOCAL_SCALAR_BYTES: BinetteLocalScalarTag = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalDescriptorAbi {
    pub schema: BinetteLocalSchemaRefAbi,
    pub backend: BinetteLocalBackendAbi,
    pub layout: BinetteLocalLayoutAbi,
    pub kind: BinetteLocalKindAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalKindAbi {
    pub tag: BinetteLocalKindTag,
    pub scalar: BinetteLocalScalarAbi,
    pub structure: BinetteLocalStructAbi,
    pub enumeration: BinetteLocalEnumAbi,
    pub sequence: BinetteLocalSequenceAbi,
    pub option: BinetteLocalOptionAbi,
    pub text: BinetteLocalStrAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalScalarAbi {
    pub tag: BinetteLocalScalarTag,
    pub storage: BinetteLocalSequenceStorageAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalStructAbi {
    pub fields: *const BinetteLocalFieldAbi,
    pub field_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalFieldAbi {
    pub name: BinetteLocalStrAbi,
    pub offset: usize,
    pub descriptor: *const BinetteLocalDescriptorAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalEnumAbi {
    pub tag: BinetteLocalEnumTagAccessAbi,
    pub variants: *const BinetteLocalVariantAbi,
    pub variant_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalVariantAbi {
    pub name: BinetteLocalStrAbi,
    pub index: u32,
    pub project: BinetteLocalVariantProjectAccessAbi,
    pub construct: BinetteLocalVariantConstructAbi,
    pub payload: *const BinetteLocalDescriptorAbi,
}

pub type BinetteLocalAccessTag = u32;
pub const BINETTE_LOCAL_ACCESS_DIRECT: BinetteLocalAccessTag = 1;
pub const BINETTE_LOCAL_ACCESS_THUNK: BinetteLocalAccessTag = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalEnumTagAccessAbi {
    pub tag: BinetteLocalAccessTag,
    pub direct_offset: usize,
    pub thunk: BinetteLocalEnumTagThunkAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalVariantProjectAccessAbi {
    pub tag: BinetteLocalAccessTag,
    pub direct_offset: usize,
    pub thunk: BinetteLocalVariantProjectThunkAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalEnumTagThunkAbi {
    pub call: Option<super::LocalEnumTagThunk>,
    pub context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalVariantProjectThunkAbi {
    pub call: Option<super::LocalVariantProjectThunk>,
    pub context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalVariantConstructAbi {
    pub call: Option<super::LocalVariantConstructThunk>,
    pub context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalSequenceAbi {
    pub element: *const BinetteLocalDescriptorAbi,
    pub storage: BinetteLocalSequenceStorageAbi,
}

pub type BinetteLocalSequenceStorageTag = u32;
pub const BINETTE_LOCAL_SEQUENCE_INLINE_FIXED: BinetteLocalSequenceStorageTag = 1;
pub const BINETTE_LOCAL_SEQUENCE_DIRECT_CONTIGUOUS: BinetteLocalSequenceStorageTag = 2;
pub const BINETTE_LOCAL_SEQUENCE_THUNK: BinetteLocalSequenceStorageTag = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalSequenceStorageAbi {
    pub tag: BinetteLocalSequenceStorageTag,
    pub offset: usize,
    pub element_count: usize,
    pub pointer_offset: usize,
    pub length_offset: usize,
    pub has_capacity: u8,
    pub capacity_offset: usize,
    pub element_stride: usize,
    pub thunks: BinetteLocalSequenceThunksAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalSequenceThunksAbi {
    pub len: Option<super::LocalSequenceLenThunk>,
    pub element_u8: Option<super::LocalSequenceU8Thunk>,
    pub element_ptr: Option<super::LocalSequenceElementPtrThunk>,
    pub write_bytes: Option<super::LocalSequenceWriteBytesThunk>,
    pub write_fixed_elements: Option<super::LocalSequenceWriteFixedElementsThunk>,
    pub context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalOptionAbi {
    pub some: *const BinetteLocalDescriptorAbi,
    pub representation: BinetteLocalOptionRepresentationAbi,
}

pub type BinetteLocalOptionRepresentationTag = u32;
pub const BINETTE_LOCAL_OPTION_DIRECT_TAG: BinetteLocalOptionRepresentationTag = 1;
pub const BINETTE_LOCAL_OPTION_NICHE: BinetteLocalOptionRepresentationTag = 2;
pub const BINETTE_LOCAL_OPTION_THUNK: BinetteLocalOptionRepresentationTag = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalOptionRepresentationAbi {
    pub tag: BinetteLocalOptionRepresentationTag,
    pub tag_offset: usize,
    pub tag_width: usize,
    pub none_value: usize,
    pub some_value: usize,
    pub some_offset: usize,
    pub none_bytes: *const u8,
    pub none_bytes_len: usize,
    pub thunks: BinetteLocalOptionThunksAbi,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BinetteLocalOptionThunksAbi {
    pub is_some: Option<super::LocalOptionIsSomeThunk>,
    pub some: Option<super::LocalOptionSomeThunk>,
    pub write_none: Option<super::LocalOptionWriteNoneThunk>,
    pub write_some_bytes: Option<super::LocalOptionWriteSomeBytesThunk>,
    pub context: *mut c_void,
}

impl BinetteLocalStrAbi {
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null(),
            len: 0,
        }
    }
}

impl LocalTypeDescriptor {
    /// Import a descriptor tree described with plain C ABI structs.
    ///
    /// # Safety
    ///
    /// `descriptor` and every pointer reachable from it must be valid for reads
    /// for the duration of this call. The returned descriptor and thunk bindings
    /// own their Rust-side metadata and do not borrow descriptor memory.
    pub unsafe fn from_abi(
        descriptor: *const BinetteLocalDescriptorAbi,
    ) -> Result<LocalDescriptorAbiImport, LocalDescriptorAbiError> {
        let mut importer = AbiImporter::new();
        let import = unsafe { importer.import_descriptor_ptr(descriptor, "$") }?;
        let descriptor = LocalTypeDescriptor::from_import(import)?;
        Ok(LocalDescriptorAbiImport {
            descriptor,
            thunks: importer.thunks,
        })
    }
}

struct AbiImporter {
    thunks: LocalThunkBindings,
}

impl AbiImporter {
    fn new() -> Self {
        Self {
            thunks: LocalThunkBindings::new(),
        }
    }

    unsafe fn import_descriptor_ptr(
        &mut self,
        descriptor: *const BinetteLocalDescriptorAbi,
        path: &str,
    ) -> Result<LocalDescriptorImport, LocalDescriptorAbiError> {
        let descriptor = unsafe { descriptor.as_ref() }.ok_or_else(|| {
            LocalDescriptorAbiError::NullDescriptor {
                path: path.to_owned(),
            }
        })?;
        unsafe { self.import_descriptor(descriptor, path) }
    }

    unsafe fn import_descriptor(
        &mut self,
        descriptor: &BinetteLocalDescriptorAbi,
        path: &str,
    ) -> Result<LocalDescriptorImport, LocalDescriptorAbiError> {
        let backend = import_backend(descriptor.backend, path)?;
        let schema = unsafe { import_schema_ref(descriptor.schema, path) }?;
        let layout = LocalValueLayout::new(
            descriptor.layout.size,
            descriptor.layout.align,
            descriptor.layout.stride,
        );
        let kind = unsafe { self.import_kind(descriptor.kind, backend, path) }?;
        Ok(LocalDescriptorImport {
            schema,
            backend,
            layout,
            kind,
        })
    }

    unsafe fn import_kind(
        &mut self,
        kind: BinetteLocalKindAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalDescriptorImportKind, LocalDescriptorAbiError> {
        match kind.tag {
            BINETTE_LOCAL_KIND_SCALAR => unsafe { self.import_scalar(kind.scalar, backend, path) },
            BINETTE_LOCAL_KIND_STRUCT => unsafe { self.import_struct(kind.structure, path) },
            BINETTE_LOCAL_KIND_ENUM => unsafe { self.import_enum(kind.enumeration, backend, path) },
            BINETTE_LOCAL_KIND_SEQUENCE => unsafe {
                self.import_sequence(kind.sequence, backend, path)
            },
            BINETTE_LOCAL_KIND_OPTION => unsafe { self.import_option(kind.option, backend, path) },
            BINETTE_LOCAL_KIND_EXTERNAL_ATTACHMENT => {
                Ok(LocalDescriptorImportKind::ExternalAttachment {
                    kind: unsafe { read_str(kind.text, path, "kind") }?,
                })
            }
            BINETTE_LOCAL_KIND_OPAQUE => Ok(LocalDescriptorImportKind::Opaque {
                reason: unsafe { read_str(kind.text, path, "reason") }?,
            }),
            tag => Err(LocalDescriptorAbiError::InvalidTag {
                path: path.to_owned(),
                field: "kind.tag",
                tag,
            }),
        }
    }

    unsafe fn import_scalar(
        &mut self,
        scalar: BinetteLocalScalarAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalDescriptorImportKind, LocalDescriptorAbiError> {
        match scalar.tag {
            BINETTE_LOCAL_SCALAR_PLAIN => {
                Ok(LocalDescriptorImportKind::Scalar(LocalScalarAccess::Plain))
            }
            BINETTE_LOCAL_SCALAR_STRING => {
                let storage = self.import_sequence_storage(scalar.storage, backend, path)?;
                Ok(LocalDescriptorImportKind::Scalar(
                    LocalScalarAccess::String(storage),
                ))
            }
            BINETTE_LOCAL_SCALAR_BYTES => {
                let storage = self.import_sequence_storage(scalar.storage, backend, path)?;
                Ok(LocalDescriptorImportKind::Scalar(LocalScalarAccess::Bytes(
                    storage,
                )))
            }
            tag => Err(LocalDescriptorAbiError::InvalidTag {
                path: path.to_owned(),
                field: "scalar.tag",
                tag,
            }),
        }
    }

    unsafe fn import_struct(
        &mut self,
        structure: BinetteLocalStructAbi,
        path: &str,
    ) -> Result<LocalDescriptorImportKind, LocalDescriptorAbiError> {
        let fields =
            unsafe { read_slice(structure.fields, structure.field_count, path, "fields") }?;
        let mut imports = Vec::with_capacity(fields.len());
        for field in fields {
            let name = unsafe { read_str(field.name, path, "field.name") }?;
            let field_path = format!("{path}.{name}");
            let descriptor = unsafe { self.import_descriptor_ptr(field.descriptor, &field_path) }?;
            imports.push(super::LocalFieldImport {
                name,
                access: LocalAccess::Direct {
                    offset: field.offset,
                },
                descriptor,
            });
        }
        Ok(LocalDescriptorImportKind::Struct { fields: imports })
    }

    unsafe fn import_enum(
        &mut self,
        enumeration: BinetteLocalEnumAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalDescriptorImportKind, LocalDescriptorAbiError> {
        let tag = self.import_enum_tag(enumeration.tag, backend, path)?;
        let variants = unsafe {
            read_slice(
                enumeration.variants,
                enumeration.variant_count,
                path,
                "variants",
            )
        }?;
        let mut imports = Vec::with_capacity(variants.len());
        for variant in variants {
            let name = unsafe { read_str(variant.name, path, "variant.name") }?;
            let variant_path = format!("{path}.{name}");
            let access = self.import_variant_project(variant.project, backend, &variant_path)?;
            let construct =
                self.import_variant_construct(variant.construct, backend, &variant_path);
            let payload = if variant.payload.is_null() {
                None
            } else {
                Some(unsafe { self.import_descriptor_ptr(variant.payload, &variant_path) }?)
            };
            imports.push(super::LocalVariantImport {
                name,
                index: variant.index,
                access,
                construct,
                payload,
            });
        }
        Ok(LocalDescriptorImportKind::Enum {
            tag,
            variants: imports,
        })
    }

    unsafe fn import_sequence(
        &mut self,
        sequence: BinetteLocalSequenceAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalDescriptorImportKind, LocalDescriptorAbiError> {
        let element =
            unsafe { self.import_descriptor_ptr(sequence.element, &format!("{path}[]"))? };
        let storage = self.import_sequence_storage(sequence.storage, backend, path)?;
        Ok(LocalDescriptorImportKind::Sequence {
            element: Box::new(element),
            storage,
        })
    }

    unsafe fn import_option(
        &mut self,
        option: BinetteLocalOptionAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalDescriptorImportKind, LocalDescriptorAbiError> {
        let some = unsafe { self.import_descriptor_ptr(option.some, &format!("{path}.some"))? };
        let representation =
            unsafe { self.import_option_representation(option.representation, backend, path) }?;
        Ok(LocalDescriptorImportKind::Option {
            some: Box::new(some),
            representation,
        })
    }

    fn import_enum_tag(
        &mut self,
        access: BinetteLocalEnumTagAccessAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalAccess, LocalDescriptorAbiError> {
        match access.tag {
            BINETTE_LOCAL_ACCESS_DIRECT => Ok(LocalAccess::Direct {
                offset: access.direct_offset,
            }),
            BINETTE_LOCAL_ACCESS_THUNK => {
                let call =
                    access
                        .thunk
                        .call
                        .ok_or_else(|| LocalDescriptorAbiError::MissingThunk {
                            path: path.to_owned(),
                            field: "enum.tag",
                        })?;
                let thunk = LocalThunk::new(backend, format!("{path}.$tag"));
                self.thunks = std::mem::take(&mut self.thunks).with_enum_tag(
                    thunk.clone(),
                    LocalEnumTagThunks {
                        tag: call,
                        context: access.thunk.context as usize,
                    },
                );
                Ok(LocalAccess::Thunk(thunk))
            }
            tag => Err(LocalDescriptorAbiError::InvalidTag {
                path: path.to_owned(),
                field: "enum.tag.access",
                tag,
            }),
        }
    }

    fn import_variant_project(
        &mut self,
        access: BinetteLocalVariantProjectAccessAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalAccess, LocalDescriptorAbiError> {
        match access.tag {
            BINETTE_LOCAL_ACCESS_DIRECT => Ok(LocalAccess::Direct {
                offset: access.direct_offset,
            }),
            BINETTE_LOCAL_ACCESS_THUNK => {
                let call =
                    access
                        .thunk
                        .call
                        .ok_or_else(|| LocalDescriptorAbiError::MissingThunk {
                            path: path.to_owned(),
                            field: "variant.project",
                        })?;
                let thunk = LocalThunk::new(backend, format!("{path}.$project"));
                self.thunks = std::mem::take(&mut self.thunks).with_variant_project(
                    thunk.clone(),
                    LocalVariantProjectThunks {
                        project: call,
                        context: access.thunk.context as usize,
                    },
                );
                Ok(LocalAccess::Thunk(thunk))
            }
            tag => Err(LocalDescriptorAbiError::InvalidTag {
                path: path.to_owned(),
                field: "variant.project.access",
                tag,
            }),
        }
    }

    fn import_variant_construct(
        &mut self,
        construct: BinetteLocalVariantConstructAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Option<LocalThunk> {
        let call = construct.call?;
        let thunk = LocalThunk::new(backend, format!("{path}.$construct"));
        self.thunks = std::mem::take(&mut self.thunks).with_variant_construct(
            thunk.clone(),
            LocalVariantConstructThunks {
                construct: call,
                context: construct.context as usize,
            },
        );
        Some(thunk)
    }

    fn import_sequence_storage(
        &mut self,
        storage: BinetteLocalSequenceStorageAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalSequenceStorage, LocalDescriptorAbiError> {
        match storage.tag {
            BINETTE_LOCAL_SEQUENCE_INLINE_FIXED => Ok(LocalSequenceStorage::InlineFixed {
                offset: storage.offset,
                element_count: storage.element_count,
                element_stride: storage.element_stride,
            }),
            BINETTE_LOCAL_SEQUENCE_DIRECT_CONTIGUOUS => {
                Ok(LocalSequenceStorage::DirectContiguous {
                    pointer: LocalAccess::Direct {
                        offset: storage.pointer_offset,
                    },
                    length: LocalAccess::Direct {
                        offset: storage.length_offset,
                    },
                    capacity: (storage.has_capacity != 0).then_some(LocalAccess::Direct {
                        offset: storage.capacity_offset,
                    }),
                    element_stride: storage.element_stride,
                })
            }
            BINETTE_LOCAL_SEQUENCE_THUNK => {
                let len =
                    storage
                        .thunks
                        .len
                        .ok_or_else(|| LocalDescriptorAbiError::MissingThunk {
                            path: path.to_owned(),
                            field: "sequence.len",
                        })?;
                let len_thunk = LocalThunk::new(backend, format!("{path}.$len"));
                let element_thunk = LocalThunk::new(backend, format!("{path}.$element"));
                if let Some(element_u8) = storage.thunks.element_u8 {
                    self.thunks = std::mem::take(&mut self.thunks).with_sequence_u8(
                        len_thunk.clone(),
                        element_thunk.clone(),
                        LocalSequenceEncodeThunks {
                            len,
                            element_u8,
                            context: storage.thunks.context as usize,
                        },
                    );
                }
                if let Some(element_ptr) = storage.thunks.element_ptr {
                    self.thunks = std::mem::take(&mut self.thunks).with_sequence_element_ptr(
                        len_thunk.clone(),
                        element_thunk.clone(),
                        LocalSequenceElementPtrEncodeThunks {
                            len,
                            element_ptr,
                            context: storage.thunks.context as usize,
                        },
                    );
                }
                let write = if storage.thunks.write_bytes.is_some()
                    || storage.thunks.write_fixed_elements.is_some()
                {
                    Some(LocalThunk::new(backend, format!("{path}.$write")))
                } else {
                    None
                };
                if let (Some(write_thunk), Some(write_bytes)) =
                    (write.as_ref(), storage.thunks.write_bytes)
                {
                    self.thunks = std::mem::take(&mut self.thunks).with_sequence_decode(
                        write_thunk.clone(),
                        LocalSequenceDecodeThunks {
                            write_bytes,
                            context: storage.thunks.context as usize,
                        },
                    );
                }
                if let (Some(write_thunk), Some(write_elements)) =
                    (write.as_ref(), storage.thunks.write_fixed_elements)
                {
                    self.thunks = std::mem::take(&mut self.thunks).with_sequence_fixed_decode(
                        write_thunk.clone(),
                        LocalSequenceFixedDecodeThunks {
                            write_elements,
                            context: storage.thunks.context as usize,
                        },
                    );
                }
                Ok(LocalSequenceStorage::Thunk {
                    len: len_thunk,
                    element: element_thunk,
                    write,
                })
            }
            tag => Err(LocalDescriptorAbiError::InvalidTag {
                path: path.to_owned(),
                field: "sequence.storage.tag",
                tag,
            }),
        }
    }

    unsafe fn import_option_representation(
        &mut self,
        option: BinetteLocalOptionRepresentationAbi,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalOptionRepresentation, LocalDescriptorAbiError> {
        match option.tag {
            BINETTE_LOCAL_OPTION_DIRECT_TAG => Ok(LocalOptionRepresentation::Tag {
                tag: LocalAccess::Direct {
                    offset: option.tag_offset,
                },
                tag_width: option.tag_width,
                none_value: option.none_value,
                some_value: option.some_value,
                some: LocalAccess::Direct {
                    offset: option.some_offset,
                },
            }),
            BINETTE_LOCAL_OPTION_NICHE => {
                let none_bytes = unsafe {
                    read_optional_bytes(
                        option.none_bytes,
                        option.none_bytes_len,
                        path,
                        "none_bytes",
                    )
                }?;
                Ok(LocalOptionRepresentation::Niche {
                    tag: LocalAccess::Direct {
                        offset: option.tag_offset,
                    },
                    tag_width: option.tag_width,
                    none_value: option.none_value,
                    none_bytes,
                    some: LocalAccess::Direct {
                        offset: option.some_offset,
                    },
                })
            }
            BINETTE_LOCAL_OPTION_THUNK => {
                let is_some =
                    option
                        .thunks
                        .is_some
                        .ok_or_else(|| LocalDescriptorAbiError::MissingThunk {
                            path: path.to_owned(),
                            field: "option.is_some",
                        })?;
                let some =
                    option
                        .thunks
                        .some
                        .ok_or_else(|| LocalDescriptorAbiError::MissingThunk {
                            path: path.to_owned(),
                            field: "option.some",
                        })?;
                let is_some_thunk = LocalThunk::new(backend, format!("{path}.$is_some"));
                let some_thunk = LocalThunk::new(backend, format!("{path}.$some"));
                self.thunks = std::mem::take(&mut self.thunks).with_option(
                    is_some_thunk.clone(),
                    some_thunk.clone(),
                    super::LocalOptionEncodeThunks {
                        is_some,
                        some,
                        context: option.thunks.context as usize,
                    },
                );
                let write_none = option
                    .thunks
                    .write_none
                    .map(|_| LocalThunk::new(backend, format!("{path}.$write_none")));
                let write_some_bytes = option
                    .thunks
                    .write_some_bytes
                    .map(|_| LocalThunk::new(backend, format!("{path}.$write_some_bytes")));
                if let (
                    Some(write_none_thunk),
                    Some(write_some_bytes_thunk),
                    Some(write_none),
                    Some(write_some_bytes),
                ) = (
                    write_none.as_ref(),
                    write_some_bytes.as_ref(),
                    option.thunks.write_none,
                    option.thunks.write_some_bytes,
                ) {
                    self.thunks = std::mem::take(&mut self.thunks).with_option_sequence_decode(
                        write_none_thunk.clone(),
                        write_some_bytes_thunk.clone(),
                        LocalOptionSequenceDecodeThunks {
                            write_none,
                            write_some_bytes,
                            context: option.thunks.context as usize,
                        },
                    );
                }
                Ok(LocalOptionRepresentation::Thunk {
                    is_some: is_some_thunk,
                    some: some_thunk,
                    write_none,
                    write_some_bytes,
                })
            }
            tag => Err(LocalDescriptorAbiError::InvalidTag {
                path: path.to_owned(),
                field: "option.representation.tag",
                tag,
            }),
        }
    }
}

unsafe fn import_schema_ref(
    schema: BinetteLocalSchemaRefAbi,
    path: &str,
) -> Result<super::LocalSchemaRef, LocalDescriptorAbiError> {
    match schema.tag {
        BINETTE_LOCAL_SCHEMA_REF_TYPE => Ok(super::LocalSchemaRef::Type(TypeRef::concrete(
            TypeId(schema.type_id),
        ))),
        BINETTE_LOCAL_SCHEMA_REF_POSITION => Ok(super::LocalSchemaRef::Position {
            owner: TypeRef::concrete(TypeId(schema.owner_type_id)),
            path: unsafe { read_str(schema.path, path, "schema.path") }?,
        }),
        tag => Err(LocalDescriptorAbiError::InvalidTag {
            path: path.to_owned(),
            field: "schema.tag",
            tag,
        }),
    }
}

fn import_backend(
    backend: BinetteLocalBackendAbi,
    path: &str,
) -> Result<LocalBackend, LocalDescriptorAbiError> {
    match backend {
        BINETTE_LOCAL_BACKEND_RUST_FACET => Ok(LocalBackend::RustFacet),
        BINETTE_LOCAL_BACKEND_SWIFT => Ok(LocalBackend::SwiftProbe),
        tag => Err(LocalDescriptorAbiError::InvalidTag {
            path: path.to_owned(),
            field: "backend",
            tag,
        }),
    }
}

unsafe fn read_str(
    value: BinetteLocalStrAbi,
    path: &str,
    field: &'static str,
) -> Result<String, LocalDescriptorAbiError> {
    if value.len == 0 {
        return Ok(String::new());
    }
    let bytes = unsafe { read_slice(value.ptr, value.len, path, field) }?;
    let text = str::from_utf8(bytes).map_err(|_| LocalDescriptorAbiError::InvalidUtf8 {
        path: path.to_owned(),
        field,
    })?;
    Ok(text.to_owned())
}

unsafe fn read_optional_bytes(
    ptr: *const u8,
    len: usize,
    path: &str,
    field: &'static str,
) -> Result<Option<Vec<u8>>, LocalDescriptorAbiError> {
    if len == 0 && ptr.is_null() {
        return Ok(None);
    }
    let bytes = unsafe { read_slice(ptr, len, path, field) }?;
    Ok(Some(bytes.to_vec()))
}

unsafe fn read_slice<'a, T>(
    ptr: *const T,
    len: usize,
    path: &str,
    field: &'static str,
) -> Result<&'a [T], LocalDescriptorAbiError> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(LocalDescriptorAbiError::NullPointer {
            path: path.to_owned(),
            field,
        });
    }
    Ok(unsafe { slice::from_raw_parts(ptr, len) })
}
