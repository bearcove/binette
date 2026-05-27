//! binette is a compact binary value format with schemas, stable type
//! identities, compatibility tooling, self-describing values, and support for
//! long-lived data.

mod compact;
mod compatibility;
mod decode;
mod dump;
mod encode;
mod error;
mod facet;
mod hash;
mod layout;
mod plan;
mod registry;
mod schema;
mod schema_format;
mod stencil;
mod value;

pub use compact::{CompactError, CompactReader, ExternalAttachmentSlot};
pub use compatibility::{
    CompatibilityDirection, CompatibilityFailure, CompatibilityFailureReason, CompatibilityReport,
    CompatibilityStatus, compatibility_report,
};
pub use decode::{DecodeError, decode_from_slice, decode_from_slice_with_plan};
pub use dump::{
    DeclarationMetadata, Defaultability, FieldMetadata, ProducerMetadata, SchemaDump,
    SchemaSnapshot, decode_schema_dump_from_slice, decode_schema_snapshot_from_slice,
    encode_schema_dump_to_vec, encode_schema_snapshot_to_vec, schema_dump_from_value,
    schema_dump_to_value, schema_snapshot_from_value, schema_snapshot_to_value,
};
pub use encode::{
    EncodeError, WriterPlan, encode_peek_to_vec_with_plan, encode_peek_with_plan, encode_to_vec,
    encode_to_vec_with_plan, writer_plan_for, writer_plan_for_shape,
};
pub use error::SchemaError;
pub use facet::{schema_bundle_for, schema_bundle_for_shape};
pub use hash::{
    primitive_for_type_id, primitive_type_id, recursive_schema_type_ids, schema_type_id,
};
pub use plan::{
    EnumPayloadPlan, EnumVariantPlan, PlanError, PlanNode, ReaderPlan, StructFieldPlan,
    reader_plan_for, reader_plan_for_bundle, reader_plan_for_bundles, reader_plan_for_shape,
};
pub use registry::SchemaRegistry;
pub use schema::{
    AttachmentDeclaration, Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef,
    Variant, VariantPayload,
};
pub use schema_format::{
    SchemaFormatError, decode_schema_bundle_from_slice, decode_schema_from_slice,
    encode_schema_bundle_to_vec, encode_schema_to_vec, schema_bundle_from_value,
    schema_bundle_to_value, schema_from_value, schema_to_value,
};
pub use stencil::{
    StencilDecoder, StencilEncoder, StencilError, StencilMode, StencilReport,
    decode_from_slice_with_stencil, encode_to_vec_with_stencil, hybrid_stencil_decoder_for,
    hybrid_stencil_decoder_from_plan, hybrid_stencil_encoder_for, hybrid_stencil_encoder_from_plan,
    stencil_decoder_for, stencil_decoder_from_plan, stencil_encoder_for, stencil_encoder_from_plan,
    strict_stencil_decoder_for, strict_stencil_decoder_from_plan, strict_stencil_encoder_for,
    strict_stencil_encoder_from_plan,
};
pub use value::{
    ArrayValue, EnumValue, ExtensionValue, FieldValue, SelfDescribingError, Value,
    decode_dynamic_value_from_slice, decode_self_described_from_slice, encode_dynamic_value_to_vec,
    encode_self_described_to_vec,
};
