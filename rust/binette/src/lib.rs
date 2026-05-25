//! binette is a compact binary value format with schemas, stable type
//! identities, compatibility tooling, and support for long-lived data.

mod compact;
mod decode;
mod encode;
mod error;
mod facet;
mod hash;
mod plan;
mod registry;
mod schema;

pub use compact::{CompactError, CompactReader};
pub use decode::{DecodeError, decode_from_slice, decode_from_slice_with_plan};
pub use encode::{
    EncodeError, WriterPlan, encode_to_vec, encode_to_vec_with_plan, writer_plan_for,
    writer_plan_for_shape,
};
pub use error::SchemaError;
pub use facet::{schema_bundle_for, schema_bundle_for_shape};
pub use hash::{primitive_for_type_id, primitive_type_id, schema_type_id};
pub use plan::{
    EnumPayloadPlan, EnumVariantPlan, PlanError, PlanNode, ReaderPlan, StructFieldPlan,
    reader_plan_for, reader_plan_for_shape,
};
pub use registry::SchemaRegistry;
pub use schema::{
    AttachmentDeclaration, Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef,
    Variant, VariantPayload,
};
