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
