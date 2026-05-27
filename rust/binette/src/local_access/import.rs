use super::{
    LocalAccess, LocalBackend, LocalFieldDescriptor, LocalOptionRepresentation, LocalScalarAccess,
    LocalSchemaRef, LocalSequenceStorage, LocalThunk, LocalTypeDescriptor, LocalTypeKind,
    LocalValueLayout, LocalVariantDescriptor,
};
use facet::Facet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDescriptorImport {
    pub schema: LocalSchemaRef,
    pub backend: LocalBackend,
    pub layout: LocalValueLayout,
    pub kind: LocalDescriptorImportKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalDescriptorImportKind {
    Scalar(LocalScalarAccess),
    Struct {
        fields: Vec<LocalFieldImport>,
    },
    Enum {
        tag: LocalAccess,
        variants: Vec<LocalVariantImport>,
    },
    Sequence {
        element: Box<LocalDescriptorImport>,
        storage: LocalSequenceStorage,
    },
    Option {
        some: Box<LocalDescriptorImport>,
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
pub struct LocalFieldImport {
    pub name: String,
    pub access: LocalAccess,
    pub descriptor: LocalDescriptorImport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVariantImport {
    pub name: String,
    pub index: u32,
    pub access: LocalAccess,
    pub project_into: Option<LocalThunk>,
    pub drop_projected: Option<LocalThunk>,
    pub construct: Option<LocalThunk>,
    pub payload: Option<LocalDescriptorImport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct LocalDescriptorExport {
    pub schema_name: String,
    pub backend: String,
    pub layout: LocalLayoutExport,
    pub kind: LocalKindExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
pub struct LocalLayoutExport {
    pub size: usize,
    pub alignment: usize,
    pub stride: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct LocalKindExport {
    pub tag: String,
    pub fields: Option<Vec<LocalFieldExport>>,
    pub variants: Option<Vec<LocalVariantExport>>,
    pub element: Option<Box<LocalDescriptorExport>>,
    pub some: Option<Box<LocalDescriptorExport>>,
    pub storage: Option<LocalStorageExport>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct LocalFieldExport {
    pub name: String,
    pub access: LocalAccessExport,
    pub descriptor: Box<LocalDescriptorExport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct LocalVariantExport {
    pub name: String,
    pub index: u32,
    pub access: LocalAccessExport,
    pub construct: Option<String>,
    pub payload: Option<Box<LocalDescriptorExport>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct LocalAccessExport {
    pub tag: String,
    pub offset: Option<usize>,
    pub thunk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Facet)]
pub struct LocalStorageExport {
    pub tag: String,
    pub pointer_offset: Option<usize>,
    pub count_offset: Option<usize>,
    pub element_stride: Option<usize>,
    pub count: Option<String>,
    pub element: Option<String>,
    pub write: Option<String>,
    pub option_tag_offset: Option<usize>,
    pub option_tag_width: Option<usize>,
    pub none_value: Option<usize>,
    pub some_value: Option<usize>,
    pub some_offset: Option<usize>,
    pub is_some: Option<String>,
    pub some: Option<String>,
    pub write_none: Option<String>,
    pub write_some_bytes: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalDescriptorImportError {
    #[error("invalid local descriptor layout: {reason}")]
    InvalidLayout { reason: &'static str },

    #[error("local descriptor backend mismatch at {path}: expected {expected:?}, got {actual:?}")]
    BackendMismatch {
        path: String,
        expected: LocalBackend,
        actual: LocalBackend,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LocalDescriptorExportError {
    #[error("unknown local descriptor schema name {schema_name:?} at {path}")]
    UnknownSchema { path: String, schema_name: String },

    #[error("unknown local descriptor backend {backend:?} at {path}")]
    UnknownBackend { path: String, backend: String },

    #[error("missing local descriptor export field {field} at {path}")]
    MissingField { path: String, field: &'static str },

    #[error("unknown local descriptor export tag {tag:?} for {field} at {path}")]
    UnknownTag {
        path: String,
        field: &'static str,
        tag: String,
    },

    #[error(transparent)]
    Import(#[from] LocalDescriptorImportError),
}

#[derive(Debug, thiserror::Error)]
pub enum LocalDescriptorHandoffError {
    #[error(transparent)]
    Json(#[from] facet_json::DeserializeError),
}

// r[impl binette.local-access.swift-probes+2]
pub fn local_descriptor_exports_from_json(
    input: &str,
) -> Result<Vec<LocalDescriptorExport>, LocalDescriptorHandoffError> {
    Ok(facet_json::from_str(input)?)
}

impl LocalTypeDescriptor {
    pub fn from_import(import: LocalDescriptorImport) -> Result<Self, LocalDescriptorImportError> {
        import.into_descriptor("$")
    }

    // r[impl binette.local-access.swift-probes+2]
    pub fn from_export<F>(
        export: LocalDescriptorExport,
        resolve_schema: F,
    ) -> Result<Self, LocalDescriptorExportError>
    where
        F: FnMut(&str) -> Option<LocalSchemaRef>,
    {
        let import = LocalDescriptorImport::from_export(export, resolve_schema)?;
        Ok(Self::from_import(import)?)
    }
}

impl LocalDescriptorImport {
    // r[impl binette.local-access.swift-probes+2]
    pub fn swift_probe(
        schema: impl Into<LocalSchemaRef>,
        layout: LocalValueLayout,
        kind: LocalDescriptorImportKind,
    ) -> Self {
        Self {
            schema: schema.into(),
            backend: LocalBackend::SwiftProbe,
            layout,
            kind,
        }
    }

    fn into_descriptor(
        self,
        path: &str,
    ) -> Result<LocalTypeDescriptor, LocalDescriptorImportError> {
        self.layout.validate()?;
        let backend = self.backend;
        let kind = self.kind.into_kind(backend, path)?;
        Ok(LocalTypeDescriptor::new(
            self.schema,
            self.backend,
            self.layout,
            kind,
        ))
    }
}

impl LocalDescriptorImport {
    // r[impl binette.local-access.swift-probes+2]
    pub fn from_export<F>(
        export: LocalDescriptorExport,
        mut resolve_schema: F,
    ) -> Result<Self, LocalDescriptorExportError>
    where
        F: FnMut(&str) -> Option<LocalSchemaRef>,
    {
        export.into_import("$", &mut resolve_schema)
    }
}

impl LocalDescriptorExport {
    fn into_import<F>(
        self,
        path: &str,
        resolve_schema: &mut F,
    ) -> Result<LocalDescriptorImport, LocalDescriptorExportError>
    where
        F: FnMut(&str) -> Option<LocalSchemaRef>,
    {
        let schema = resolve_schema(&self.schema_name).ok_or_else(|| {
            LocalDescriptorExportError::UnknownSchema {
                path: path.to_owned(),
                schema_name: self.schema_name.clone(),
            }
        })?;
        let backend = parse_backend(&self.backend, path)?;
        let kind = self.kind.into_import_kind(backend, path, resolve_schema)?;
        Ok(LocalDescriptorImport {
            schema,
            backend,
            layout: LocalValueLayout::new(
                self.layout.size,
                self.layout.alignment,
                self.layout.stride,
            ),
            kind,
        })
    }
}

impl LocalKindExport {
    fn into_import_kind<F>(
        self,
        backend: LocalBackend,
        path: &str,
        resolve_schema: &mut F,
    ) -> Result<LocalDescriptorImportKind, LocalDescriptorExportError>
    where
        F: FnMut(&str) -> Option<LocalSchemaRef>,
    {
        match self.tag.as_str() {
            "scalar" => Ok(LocalDescriptorImportKind::Scalar(LocalScalarAccess::Plain)),
            "string" => Ok(LocalDescriptorImportKind::Scalar(
                LocalScalarAccess::String(
                    required(self.storage, path, "storage")?
                        .into_sequence_storage(backend, path)?,
                ),
            )),
            "bytes" => Ok(LocalDescriptorImportKind::Scalar(LocalScalarAccess::Bytes(
                required(self.storage, path, "storage")?.into_sequence_storage(backend, path)?,
            ))),
            "struct" => Ok(LocalDescriptorImportKind::Struct {
                fields: self
                    .fields
                    .unwrap_or_default()
                    .into_iter()
                    .map(|field| field.into_import(backend, path, resolve_schema))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            "enum" => Ok(LocalDescriptorImportKind::Enum {
                tag: self
                    .fields
                    .as_deref()
                    .and_then(|fields| fields.iter().find(|field| field.name == "$tag"))
                    .ok_or_else(|| LocalDescriptorExportError::MissingField {
                        path: path.to_owned(),
                        field: "fields.$tag",
                    })?
                    .access
                    .clone()
                    .into_access(backend, path)?,
                variants: self
                    .variants
                    .unwrap_or_default()
                    .into_iter()
                    .map(|variant| variant.into_import(backend, path, resolve_schema))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            "sequence" => Ok(LocalDescriptorImportKind::Sequence {
                element: Box::new(
                    required(self.element, path, "element")?
                        .into_import(&format!("{path}[]"), resolve_schema)?,
                ),
                storage: required(self.storage, path, "storage")?
                    .into_sequence_storage(backend, path)?,
            }),
            "option" => Ok(LocalDescriptorImportKind::Option {
                some: Box::new(
                    required(self.some, path, "some")?
                        .into_import(&format!("{path}.some"), resolve_schema)?,
                ),
                representation: required(self.storage, path, "storage")?
                    .into_option_representation(backend, path)?,
            }),
            "external-attachment" => Ok(LocalDescriptorImportKind::ExternalAttachment {
                kind: required(self.reason, path, "reason")?,
            }),
            "opaque" => Ok(LocalDescriptorImportKind::Opaque {
                reason: required(self.reason, path, "reason")?,
            }),
            _ => Err(LocalDescriptorExportError::UnknownTag {
                path: path.to_owned(),
                field: "kind.tag",
                tag: self.tag,
            }),
        }
    }
}

impl LocalFieldExport {
    fn into_import<F>(
        self,
        backend: LocalBackend,
        parent_path: &str,
        resolve_schema: &mut F,
    ) -> Result<LocalFieldImport, LocalDescriptorExportError>
    where
        F: FnMut(&str) -> Option<LocalSchemaRef>,
    {
        let path = format!("{parent_path}.{}", self.name);
        Ok(LocalFieldImport {
            name: self.name,
            access: self.access.into_access(backend, &path)?,
            descriptor: self.descriptor.into_import(&path, resolve_schema)?,
        })
    }
}

impl LocalVariantExport {
    fn into_import<F>(
        self,
        backend: LocalBackend,
        parent_path: &str,
        resolve_schema: &mut F,
    ) -> Result<LocalVariantImport, LocalDescriptorExportError>
    where
        F: FnMut(&str) -> Option<LocalSchemaRef>,
    {
        let path = format!("{parent_path}::{}", self.name);
        Ok(LocalVariantImport {
            name: self.name,
            index: self.index,
            access: self.access.into_access(backend, &path)?,
            project_into: None,
            drop_projected: None,
            construct: self.construct.map(|name| LocalThunk::new(backend, name)),
            payload: self
                .payload
                .map(|payload| payload.into_import(&path, resolve_schema))
                .transpose()?,
        })
    }
}

impl LocalAccessExport {
    fn into_access(
        self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalAccess, LocalDescriptorExportError> {
        match self.tag.as_str() {
            "direct" => Ok(LocalAccess::Direct {
                offset: required(self.offset, path, "offset")?,
            }),
            "thunk" => Ok(LocalAccess::Thunk(LocalThunk::new(
                backend,
                required(self.thunk, path, "thunk")?,
            ))),
            _ => Err(LocalDescriptorExportError::UnknownTag {
                path: path.to_owned(),
                field: "access.tag",
                tag: self.tag,
            }),
        }
    }
}

impl LocalStorageExport {
    fn into_sequence_storage(
        self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalSequenceStorage, LocalDescriptorExportError> {
        match self.tag.as_str() {
            "direct-contiguous" => Ok(LocalSequenceStorage::DirectContiguous {
                pointer: LocalAccess::Direct {
                    offset: required(self.pointer_offset, path, "pointer_offset")?,
                },
                length: LocalAccess::Direct {
                    offset: required(self.count_offset, path, "count_offset")?,
                },
                capacity: None,
                element_stride: required(self.element_stride, path, "element_stride")?,
            }),
            "thunk" => Ok(LocalSequenceStorage::Thunk {
                len: LocalThunk::new(backend, required(self.count, path, "count")?),
                element: LocalThunk::new(backend, required(self.element, path, "element")?),
                write: self.write.map(|name| LocalThunk::new(backend, name)),
            }),
            _ => Err(LocalDescriptorExportError::UnknownTag {
                path: path.to_owned(),
                field: "storage.tag",
                tag: self.tag,
            }),
        }
    }

    fn into_option_representation(
        self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalOptionRepresentation, LocalDescriptorExportError> {
        match self.tag.as_str() {
            "direct-tag" => Ok(LocalOptionRepresentation::Tag {
                tag: LocalAccess::Direct {
                    offset: required(self.option_tag_offset, path, "option_tag_offset")?,
                },
                tag_width: self.option_tag_width.unwrap_or(1),
                none_value: required(self.none_value, path, "none_value")?,
                some_value: required(self.some_value, path, "some_value")?,
                some: LocalAccess::Direct {
                    offset: required(self.some_offset, path, "some_offset")?,
                },
            }),
            "niche-tag" => Ok(LocalOptionRepresentation::Niche {
                tag: LocalAccess::Direct {
                    offset: required(self.option_tag_offset, path, "option_tag_offset")?,
                },
                tag_width: self.option_tag_width.unwrap_or(1),
                none_value: required(self.none_value, path, "none_value")?,
                none_bytes: None,
                some: LocalAccess::Direct {
                    offset: required(self.some_offset, path, "some_offset")?,
                },
            }),
            "thunk" => Ok(LocalOptionRepresentation::Thunk {
                is_some: LocalThunk::new(backend, required(self.is_some, path, "is_some")?),
                some: LocalThunk::new(backend, required(self.some, path, "some")?),
                write_none: self.write_none.map(|name| LocalThunk::new(backend, name)),
                write_some_bytes: self
                    .write_some_bytes
                    .map(|name| LocalThunk::new(backend, name)),
            }),
            _ => Err(LocalDescriptorExportError::UnknownTag {
                path: path.to_owned(),
                field: "storage.tag",
                tag: self.tag,
            }),
        }
    }
}

fn parse_backend(backend: &str, path: &str) -> Result<LocalBackend, LocalDescriptorExportError> {
    match backend {
        "swift-probe" => Ok(LocalBackend::SwiftProbe),
        "rust-facet" => Ok(LocalBackend::RustFacet),
        _ => Err(LocalDescriptorExportError::UnknownBackend {
            path: path.to_owned(),
            backend: backend.to_owned(),
        }),
    }
}

fn required<T>(
    value: Option<T>,
    path: &str,
    field: &'static str,
) -> Result<T, LocalDescriptorExportError> {
    value.ok_or_else(|| LocalDescriptorExportError::MissingField {
        path: path.to_owned(),
        field,
    })
}

impl LocalDescriptorImportKind {
    fn into_kind(
        self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<LocalTypeKind, LocalDescriptorImportError> {
        match self {
            Self::Scalar(access) => {
                access.validate_backend(backend, path)?;
                Ok(LocalTypeKind::Scalar(access))
            }
            Self::Struct { fields } => {
                let fields = fields
                    .into_iter()
                    .map(|field| field.into_descriptor(backend, path))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(LocalTypeKind::Struct { fields })
            }
            Self::Enum { tag, variants } => {
                tag.validate_backend(backend, path)?;
                let variants = variants
                    .into_iter()
                    .map(|variant| variant.into_descriptor(backend, path))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(LocalTypeKind::Enum { tag, variants })
            }
            Self::Sequence { element, storage } => {
                storage.validate_backend(backend, path)?;
                Ok(LocalTypeKind::Sequence {
                    element: Box::new(element.into_descriptor(&format!("{path}[]"))?),
                    storage,
                })
            }
            Self::Option {
                some,
                representation,
            } => {
                representation.validate_backend(backend, path)?;
                Ok(LocalTypeKind::Option {
                    some: Box::new(some.into_descriptor(&format!("{path}.some"))?),
                    representation,
                })
            }
            Self::ExternalAttachment { kind } => Ok(LocalTypeKind::ExternalAttachment { kind }),
            Self::Opaque { reason } => Ok(LocalTypeKind::Opaque { reason }),
        }
    }
}

impl LocalFieldImport {
    fn into_descriptor(
        self,
        backend: LocalBackend,
        parent_path: &str,
    ) -> Result<LocalFieldDescriptor, LocalDescriptorImportError> {
        let path = format!("{parent_path}.{}", self.name);
        self.access.validate_backend(backend, &path)?;
        Ok(LocalFieldDescriptor {
            name: self.name,
            access: self.access,
            descriptor: Box::new(self.descriptor.into_descriptor(&path)?),
        })
    }
}

impl LocalVariantImport {
    fn into_descriptor(
        self,
        backend: LocalBackend,
        parent_path: &str,
    ) -> Result<LocalVariantDescriptor, LocalDescriptorImportError> {
        let path = format!("{parent_path}::{}", self.name);
        self.access.validate_backend(backend, &path)?;
        if let Some(project_into) = &self.project_into {
            project_into.validate_backend(backend, &path)?;
        }
        if let Some(drop_projected) = &self.drop_projected {
            drop_projected.validate_backend(backend, &path)?;
        }
        if let Some(construct) = &self.construct {
            construct.validate_backend(backend, &path)?;
        }
        Ok(LocalVariantDescriptor {
            name: self.name,
            index: self.index,
            access: self.access,
            project_into: self.project_into,
            drop_projected: self.drop_projected,
            construct: self.construct,
            payload: self
                .payload
                .map(|payload| payload.into_descriptor(&path).map(Box::new))
                .transpose()?,
        })
    }
}

impl LocalValueLayout {
    fn validate(&self) -> Result<(), LocalDescriptorImportError> {
        if self.align == 0 || !self.align.is_power_of_two() {
            return Err(LocalDescriptorImportError::InvalidLayout {
                reason: "alignment must be a non-zero power of two",
            });
        }
        if self.stride < self.size {
            return Err(LocalDescriptorImportError::InvalidLayout {
                reason: "stride must be at least size",
            });
        }
        Ok(())
    }
}

impl LocalAccess {
    fn validate_backend(
        &self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<(), LocalDescriptorImportError> {
        match self {
            Self::Direct { .. } => Ok(()),
            Self::Thunk(thunk) => thunk.validate_backend(backend, path),
        }
    }
}

impl LocalThunk {
    fn validate_backend(
        &self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<(), LocalDescriptorImportError> {
        if self.backend == backend {
            Ok(())
        } else {
            Err(LocalDescriptorImportError::BackendMismatch {
                path: path.to_owned(),
                expected: backend,
                actual: self.backend,
            })
        }
    }
}

impl LocalScalarAccess {
    fn validate_backend(
        &self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<(), LocalDescriptorImportError> {
        match self {
            Self::Plain => Ok(()),
            Self::String(storage) | Self::Bytes(storage) => storage.validate_backend(backend, path),
        }
    }
}

impl LocalSequenceStorage {
    fn validate_backend(
        &self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<(), LocalDescriptorImportError> {
        match self {
            Self::InlineFixed { .. } => Ok(()),
            Self::DirectContiguous {
                pointer,
                length,
                capacity,
                ..
            } => {
                pointer.validate_backend(backend, path)?;
                length.validate_backend(backend, path)?;
                if let Some(capacity) = capacity {
                    capacity.validate_backend(backend, path)?;
                }
                Ok(())
            }
            Self::Thunk {
                len,
                element,
                write,
            } => {
                len.validate_backend(backend, path)?;
                element.validate_backend(backend, path)?;
                if let Some(write) = write {
                    write.validate_backend(backend, path)?;
                }
                Ok(())
            }
        }
    }
}

impl LocalOptionRepresentation {
    fn validate_backend(
        &self,
        backend: LocalBackend,
        path: &str,
    ) -> Result<(), LocalDescriptorImportError> {
        match self {
            Self::Tag { tag, some, .. } | Self::Niche { tag, some, .. } => {
                tag.validate_backend(backend, path)?;
                some.validate_backend(backend, &format!("{path}.some"))
            }
            Self::Thunk {
                is_some,
                some,
                write_none,
                write_some_bytes,
            } => {
                is_some.validate_backend(backend, path)?;
                some.validate_backend(backend, path)?;
                if let Some(write_none) = write_none {
                    write_none.validate_backend(backend, path)?;
                }
                if let Some(write_some_bytes) = write_some_bytes {
                    write_some_bytes.validate_backend(backend, path)?;
                }
                Ok(())
            }
        }
    }
}
