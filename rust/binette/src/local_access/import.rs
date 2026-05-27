use super::{
    LocalAccess, LocalBackend, LocalFieldDescriptor, LocalOptionRepresentation, LocalScalarAccess,
    LocalSchemaRef, LocalSequenceStorage, LocalThunk, LocalTypeDescriptor, LocalTypeKind,
    LocalValueLayout, LocalVariantDescriptor,
};

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
    pub construct: Option<LocalThunk>,
    pub payload: Option<LocalDescriptorImport>,
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

impl LocalTypeDescriptor {
    pub fn from_import(import: LocalDescriptorImport) -> Result<Self, LocalDescriptorImportError> {
        import.into_descriptor("$")
    }
}

impl LocalDescriptorImport {
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
        if let Some(construct) = &self.construct {
            construct.validate_backend(backend, &path)?;
        }
        Ok(LocalVariantDescriptor {
            name: self.name,
            index: self.index,
            access: self.access,
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
            Self::Tag { tag, .. } => tag.validate_backend(backend, path),
            Self::NicheString {
                string, none_tag, ..
            } => {
                string.validate_backend(backend, path)?;
                none_tag.validate_backend(backend, path)
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
