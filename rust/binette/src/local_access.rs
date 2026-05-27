use std::ffi::c_void;
use std::mem::{align_of, size_of};

use crate::schema::TypeRef;

mod import;
pub use import::{
    LocalDescriptorImport, LocalDescriptorImportError, LocalDescriptorImportKind, LocalFieldImport,
    LocalVariantImport,
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

// r[impl binette.local-access.descriptor]
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
pub type LocalSequenceWriteBytesThunk =
    unsafe extern "C" fn(value: *mut u8, ptr: *const u8, len: usize, context: *mut c_void) -> bool;

#[derive(Debug, Clone, Copy)]
pub struct LocalSequenceEncodeThunks {
    pub len: LocalSequenceLenThunk,
    pub element_u8: LocalSequenceU8Thunk,
    pub context: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalSequenceDecodeThunks {
    pub write_bytes: LocalSequenceWriteBytesThunk,
    pub context: usize,
}

#[derive(Debug, Clone)]
pub struct LocalSequenceThunkBinding {
    pub len: LocalThunk,
    pub element: LocalThunk,
    pub thunks: LocalSequenceEncodeThunks,
}

#[derive(Debug, Clone)]
pub struct LocalSequenceDecodeThunkBinding {
    pub write: LocalThunk,
    pub thunks: LocalSequenceDecodeThunks,
}

#[derive(Debug, Default, Clone)]
pub struct LocalThunkBindings {
    sequence_u8: Vec<LocalSequenceThunkBinding>,
    sequence_decode: Vec<LocalSequenceDecodeThunkBinding>,
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
        none_value: usize,
    },
    NicheString {
        string: LocalSequenceStorage,
        none_tag: LocalAccess,
        none_value: usize,
    },
    Thunk {
        is_some: LocalThunk,
        some: LocalThunk,
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
}

#[cfg(not(target_arch = "wasm32"))]
mod rust_layout {
    use std::collections::HashMap;

    use super::*;
    use facet_core::{Def, Shape, StructKind, Type, UserType};

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
    // r[impl binette.local-access.descriptor]
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
                            }
                        })
                    } else {
                        LocalOptionRepresentation::Thunk {
                            is_some: LocalThunk::new(
                                LocalBackend::RustFacet,
                                "Facet.Option.is_some",
                            ),
                            some: LocalThunk::new(LocalBackend::RustFacet, "Facet.Option.some"),
                        }
                    };
                    Ok(LocalTypeKind::Option {
                        some: Box::new(element),
                        representation,
                    })
                }
                SchemaKind::Enum { variants, .. } => {
                    let shape_enum = enum_type_for_shape(shape)?;
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
                            Ok(LocalVariantDescriptor {
                                name: schema_variant.name.clone(),
                                index: schema_variant.index,
                                access: LocalAccess::Thunk(LocalThunk::new(
                                    LocalBackend::RustFacet,
                                    format!("Facet.Enum.{}", schema_variant.name),
                                )),
                                payload,
                            })
                        })
                        .collect::<Result<Vec<_>, LocalAccessError>>()?;
                    Ok(LocalTypeKind::Enum {
                        tag: LocalAccess::Thunk(LocalThunk::new(
                            LocalBackend::RustFacet,
                            "Facet.Enum.discriminant",
                        )),
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

    fn option_string_representation_from_layout(
        layout: OptionStringLayout,
    ) -> LocalOptionRepresentation {
        LocalOptionRepresentation::NicheString {
            string: sequence_storage_from_vec_layout(layout.some_string, 1),
            none_tag: LocalAccess::Direct {
                offset: layout.none_tag_offset,
            },
            none_value: layout.none_tag_value,
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
pub use rust_layout::{LocalAccessError, rust_facet_descriptor_for_shape};
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

    // r[verify binette.local-access.descriptor]
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
                LocalOptionRepresentation::NicheString {
                    string, none_tag, ..
                },
        } = descriptor.kind
        else {
            panic!("expected niche string option storage");
        };

        assert!(matches!(
            some.kind,
            LocalTypeKind::Scalar(LocalScalarAccess::String(_))
        ));
        assert!(matches!(
            string,
            LocalSequenceStorage::DirectContiguous { .. }
        ));
        assert!(matches!(none_tag, LocalAccess::Direct { .. }));
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

    // r[verify binette.local-access.swift-probes]
    // r[verify binette.local-access.descriptor]
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
                                    write: None,
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
                        payload: None,
                    },
                    LocalVariantImport {
                        name: "titled".to_owned(),
                        index: 1,
                        access: LocalAccess::Thunk(LocalThunk::new(
                            LocalBackend::SwiftProbe,
                            "ProbeEnum.project.titled",
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

    // r[verify binette.local-access.swift-probes]
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
    // r[verify binette.local-access.descriptor]
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
    // r[verify binette.local-access.descriptor]
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
        let LocalTypeKind::Enum { variants, .. } = enum_descriptor.kind else {
            panic!("expected enum descriptor");
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            ["Empty", "Count", "Named"]
        );
        assert!(variants[0].payload.is_none());
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
