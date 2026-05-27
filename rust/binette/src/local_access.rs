use std::mem::{align_of, size_of};

use crate::schema::TypeRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

// r[impl binette.local-access.descriptor]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTypeDescriptor {
    pub schema: TypeRef,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalThunk {
    pub backend: LocalBackend,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSequenceStorage {
    DirectContiguous {
        pointer: LocalAccess,
        length: LocalAccess,
        capacity: Option<LocalAccess>,
        element_stride: usize,
    },
    Thunk {
        len: LocalThunk,
        element: LocalThunk,
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
        schema: TypeRef,
        backend: LocalBackend,
        layout: LocalValueLayout,
        kind: LocalTypeKind,
    ) -> Self {
        Self {
            schema,
            backend,
            layout,
            kind,
        }
    }

    pub fn rust_facet(schema: TypeRef, layout: LocalValueLayout, kind: LocalTypeKind) -> Self {
        Self::new(schema, LocalBackend::RustFacet, layout, kind)
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

#[cfg(not(target_arch = "wasm32"))]
mod rust_layout {
    use super::*;
    use crate::layout::{OptionStringLayout, VecLayout, option_string_layout, string_layout};

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
}

#[cfg(not(target_arch = "wasm32"))]
pub use rust_layout::{rust_option_string_descriptor, rust_string_descriptor, rust_vec_descriptor};
#[cfg(not(target_arch = "wasm32"))]
pub use rust_layout::{rust_option_string_representation, rust_string_storage, rust_vec_storage};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::primitive_type_id;
    use crate::schema::{Primitive, TypeId};

    // r[verify binette.local-access.descriptor]
    #[test]
    fn descriptor_keeps_schema_backend_and_layout_separate() {
        let schema = TypeRef::concrete(primitive_type_id(Primitive::U8));
        let descriptor = LocalTypeDescriptor::rust_facet(
            schema.clone(),
            LocalValueLayout::of::<u8>(),
            LocalTypeKind::Scalar(LocalScalarAccess::Plain),
        );

        assert_eq!(descriptor.schema, schema);
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
}
