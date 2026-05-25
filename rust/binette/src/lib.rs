//! binette is a compact binary value format with schemas, stable type
//! identities, compatibility tooling, and support for long-lived data.

mod error;
mod facet;
mod hash;
mod registry;
mod schema;

pub use error::SchemaError;
pub use facet::{schema_bundle_for, schema_bundle_for_shape};
pub use hash::{primitive_for_type_id, primitive_type_id, schema_type_id};
pub use registry::SchemaRegistry;
pub use schema::{
    AttachmentDeclaration, Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef,
    Variant, VariantPayload,
};
