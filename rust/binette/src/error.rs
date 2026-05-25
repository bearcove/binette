use facet_core::ScalarType;
use thiserror::Error;

use crate::schema::{Primitive, TypeId};

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("unsupported scalar type {scalar:?} for {type_name}")]
    UnsupportedScalar {
        scalar: ScalarType,
        type_name: &'static str,
    },

    #[error("unsupported shape {type_name}: {reason}")]
    UnsupportedShape {
        type_name: &'static str,
        reason: &'static str,
    },

    #[error("recursive schema extraction is not implemented yet for {type_name}")]
    RecursiveSchemaUnsupported { type_name: &'static str },

    #[error("external attachment metadata hashing is not implemented yet")]
    ExternalMetadataHashUnsupported,

    #[error("schema declared id {declared:?} but canonical content hashes to {computed:?}")]
    SchemaIdMismatch { declared: TypeId, computed: TypeId },

    #[error("schema id {type_id:?} is reserved for primitive {primitive:?}")]
    SchemaIdReservedForPrimitive {
        type_id: TypeId,
        primitive: Primitive,
    },

    #[error("schema id {type_id:?} is already installed with different content")]
    DuplicateSchemaId { type_id: TypeId },

    #[error("unknown concrete type id {type_id:?}")]
    UnknownTypeId { type_id: TypeId },

    #[error("unknown type parameter {name}")]
    UnknownTypeParameter { name: String },

    #[error(
        "type id {type_id:?} expects {expected} type arguments but reference supplied {actual}"
    )]
    TypeArgumentArity {
        type_id: TypeId,
        expected: usize,
        actual: usize,
    },

    #[error("schema has duplicate type parameter {name}")]
    DuplicateTypeParameter { name: String },

    #[error("schema declaration name must not be empty")]
    EmptySchemaName,

    #[error("array schema rank must be at least one")]
    InvalidArrayRank,

    #[error("tuple schema arity must be at least one")]
    InvalidTupleArity,

    #[error("recursive registry verification is not implemented yet for {type_id:?}")]
    RecursiveRegistryUnsupported { type_id: TypeId },
}
