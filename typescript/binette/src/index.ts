export {
  BinetteError,
  decodeDynamicValue,
  decodeSelfDescribedPrefix,
  decodeSelfDescribed,
  encodeDynamicValue,
  encodeSelfDescribed,
} from "./value.js";

export {
  CompactReader,
  decodeCompact,
  encodeCompact,
  externalAttachmentSlots,
  skipCompact,
} from "./compact.js";

export {
  decodeBool,
  decodeBytes,
  decodeF32,
  decodeF64,
  decodeI8,
  decodeI16,
  decodeI32,
  decodeI64,
  decodeI128,
  decodeString,
  decodeU8,
  decodeU16,
  decodeU32,
  decodeU64,
  decodeU128,
  encodeBool,
  encodeBytes,
  encodeF32,
  encodeF64,
  encodeI8,
  encodeI16,
  encodeI32,
  encodeI64,
  encodeI128,
  encodeString,
  encodeU8,
  encodeU16,
  encodeU32,
  encodeU64,
  encodeU128,
} from "./primitives.js";

export {
  PlanError,
  readerPlanForBundle,
  readerPlanForBundles,
} from "./plan.js";

export { compatibilityReport } from "./compatibility.js";

export {
  decodeSchemaDump,
  decodeSchemaSnapshot,
  encodeSchemaDump,
  encodeSchemaSnapshot,
  schemaDumpFromValue,
  schemaDumpToValue,
  schemaSnapshotFromValue,
  schemaSnapshotToValue,
} from "./dump.js";

export {
  decodeSchema,
  decodeSchemaBundle,
  encodeSchema,
  encodeSchemaBundle,
  concreteTypeRef,
  isPrimitive,
  primitiveForTypeId,
  primitiveTypeId,
  recursiveSchemaTypeIds,
  recursiveTypeIdMap,
  schemaBundleFromValue,
  schemaBundleToValue,
  schemaFromValue,
  schemaToValue,
  schemaTypeId,
  typeRefFromValue,
  typeRefToValue,
  typeVar,
} from "./schema.js";

export { SchemaRegistry } from "./registry.js";

export type { Value } from "./value.js";

export type { ExternalAttachmentSlot } from "./compact.js";

export type { DecodeResult } from "./primitives.js";

export type {
  EnumPayloadPlan,
  EnumVariantPlan,
  PlanErrorKind,
  PlanNode,
  ReaderPlan,
  StructFieldPlan,
} from "./plan.js";

export type {
  CompatibilityDirection,
  CompatibilityFailure,
  CompatibilityFailureReason,
  CompatibilityReport,
  CompatibilityStatus,
} from "./compatibility.js";

export type {
  DeclarationMetadata,
  Defaultability,
  FieldMetadata,
  ProducerMetadata,
  SchemaDump,
  SchemaSnapshot,
} from "./dump.js";

export type {
  AttachmentDeclaration,
  Field,
  Primitive,
  Schema,
  SchemaBundle,
  SchemaKind,
  TypeId,
  TypeRef,
  Variant,
  VariantPayload,
} from "./schema.js";
