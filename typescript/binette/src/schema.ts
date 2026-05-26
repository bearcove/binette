import { blake3 } from "@noble/hashes/blake3.js";

import {
  BinetteError,
  decodeSelfDescribed,
  encodeSelfDescribed,
  type Value,
} from "./value.js";

const PRIMITIVES = [
  "bool",
  "u8",
  "u16",
  "u32",
  "u64",
  "u128",
  "i8",
  "i16",
  "i32",
  "i64",
  "i128",
  "f32",
  "f64",
  "char",
  "string",
  "unit",
  "never",
  "bytes",
  "payload",
] as const;

export type Primitive = (typeof PRIMITIVES)[number];

// r[impl binette.type-id]
export type TypeId = bigint;

// r[impl binette.schema.type-ref]
// r[impl binette.type-id.hash.typeref]
export type TypeRef =
  | { kind: "concrete"; typeId: TypeId; args: TypeRef[] }
  | { kind: "var"; name: string };

// r[impl binette.schema.fields]
export type Field = {
  name: string;
  typeRef: TypeRef;
};

export type Variant = {
  name: string;
  index: number;
  payload: VariantPayload;
};

export type VariantPayload =
  | { kind: "unit" }
  | { kind: "newtype"; typeRef: TypeRef }
  | { kind: "tuple"; elements: TypeRef[] }
  | { kind: "struct"; fields: Field[] };

// r[impl binette.schema.kinds]
// r[impl binette.schema.array]
// r[impl binette.schema.dynamic]
// r[impl binette.schema.external]
// r[impl binette.schema.extension]
// r[impl binette.schema.name]
// r[impl binette.schema.tuple]
export type SchemaKind =
  | { kind: "primitive"; primitive: Primitive }
  | { kind: "struct"; name: string; fields: Field[] }
  | { kind: "enum"; name: string; variants: Variant[] }
  | { kind: "tuple"; elements: TypeRef[] }
  | { kind: "list"; element: TypeRef }
  | { kind: "set"; element: TypeRef }
  | { kind: "map"; key: TypeRef; value: TypeRef }
  | { kind: "array"; element: TypeRef; dimensions: bigint[] }
  | { kind: "option"; element: TypeRef }
  | { kind: "dynamic" }
  | { kind: "external"; externalKind: string; metadata: Value };

// r[impl binette.schema.model]
export type Schema = {
  id: TypeId;
  typeParams: string[];
  kind: SchemaKind;
};

// r[impl binette.bundle.model]
export type SchemaBundle = {
  schemas: Schema[];
  root: TypeRef;
  attachments: AttachmentDeclaration[];
};

// r[impl binette.bundle.attachments]
export type AttachmentDeclaration = {
  kind: string;
  metadataSchema: TypeRef | null;
};

export function concreteTypeRef(typeId: TypeId, args: TypeRef[] = []): TypeRef {
  return { kind: "concrete", typeId, args };
}

export function typeVar(name: string): TypeRef {
  return { kind: "var", name };
}

// r[impl binette.schema.primitive]
export function isPrimitive(value: string): value is Primitive {
  return (PRIMITIVES as readonly string[]).includes(value);
}

// r[impl binette.type-id.hash.primitives]
export function primitiveTypeId(primitive: Primitive): TypeId {
  const writer = new HashWriter();
  writer.string(primitive);
  return hashTypeId(writer.finish());
}

export function primitiveForTypeId(typeId: TypeId): Primitive | null {
  for (const primitive of PRIMITIVES) {
    if (primitiveTypeId(primitive) === typeId) {
      return primitive;
    }
  }
  return null;
}

// r[impl binette.type-id.hash]
// r[impl binette.type-id.hash.struct]
// r[impl binette.type-id.hash.enum]
// r[impl binette.type-id.hash.container]
// r[impl binette.type-id.hash.tuple]
// r[impl binette.type-id.hash.dynamic]
// r[impl binette.type-id.hash.external]
// r[impl binette.type-id.context-free]
// r[impl binette.hash.recursive.non-recursive]
export function schemaTypeId(schema: Pick<Schema, "typeParams" | "kind">): TypeId {
  return hashTypeId(typeIdHashBytes(schema.kind, schema.typeParams, null));
}

// r[impl binette.hash.recursive]
export function recursiveSchemaTypeIds(schemas: readonly Schema[]): TypeId[] {
  const map = recursiveTypeIdMap(schemas);
  return schemas.map((schema) => {
    const typeId = map.get(schema.id);
    if (typeId === undefined) {
      throw new BinetteError(
        `recursive type-id map is missing schema ${schema.id}`,
      );
    }
    return typeId;
  });
}

export function recursiveTypeIdMap(
  schemas: readonly Schema[],
): Map<TypeId, TypeId> {
  const groupIds = new Set<TypeId>(schemas.map((schema) => schema.id));
  const entries: Array<{
    originalIds: TypeId[];
    preliminary: TypeId;
    bytes: Uint8Array;
  }> = [];

  for (const schema of schemas) {
    const bytes = typeIdHashBytes(schema.kind, schema.typeParams, groupIds);
    const existing = entries.find((entry) => bytesEqual(entry.bytes, bytes));
    if (existing !== undefined) {
      existing.originalIds.push(schema.id);
    } else {
      entries.push({
        originalIds: [schema.id],
        preliminary: hashTypeId(bytes),
        bytes,
      });
    }
  }

  entries.sort((left, right) => {
    if (left.preliminary < right.preliminary) {
      return -1;
    }
    if (left.preliminary > right.preliminary) {
      return 1;
    }
    return compareBytes(left.bytes, right.bytes);
  });

  const groupHashInput = new Uint8Array(entries.length * 8);
  entries.forEach((entry, index) => {
    groupHashInput.set(typeIdToLeBytes(entry.preliminary), index * 8);
  });
  const groupHash = hashTypeId(groupHashInput);

  const result = new Map<TypeId, TypeId>();
  entries.forEach((entry, index) => {
    const input = new Uint8Array(16);
    input.set(typeIdToLeBytes(groupHash), 0);
    input.set(typeIdToLeBytes(BigInt(index)), 8);
    const finalId = hashTypeId(input);
    for (const originalId of entry.originalIds) {
      result.set(originalId, finalId);
    }
  });
  return result;
}

// r[impl binette.schema.encoding.self-describing]
// r[impl binette.schema.format+2]
export function schemaToValue(schema: Schema): Value {
  return {
    kind: "struct",
    fields: [
      { name: "id", value: { kind: "u64", value: schema.id } },
      {
        name: "type_params",
        value: {
          kind: "list",
          elements: schema.typeParams.map((value) => ({ kind: "string", value })),
        },
      },
      { name: "kind", value: schemaKindToValue(schema.kind) },
    ],
  };
}

// r[impl binette.schema.encoding.self-describing]
// r[impl binette.schema.format+2]
export function schemaFromValue(value: Value): Schema {
  const fields = expectStruct(value, "schema", ["id", "type_params", "kind"]);
  const schema: Schema = {
    id: expectU64(fields[0], "schema.id"),
    typeParams: expectStringList(fields[1], "schema.type_params"),
    kind: schemaKindFromValue(fields[2]),
  };

  const computed = schemaTypeId(schema);
  if (computed !== schema.id) {
    throw new BinetteError(
      `schema declared id ${schema.id} but canonical content hashes to ${computed}`,
    );
  }

  return schema;
}

export function encodeSchema(schema: Schema): Uint8Array {
  return encodeSelfDescribed(schemaToValue(schema));
}

export function decodeSchema(bytes: Uint8Array): Schema {
  return schemaFromValue(decodeSelfDescribed(bytes));
}

// r[impl binette.bundle.format]
export function schemaBundleToValue(bundle: SchemaBundle): Value {
  return {
    kind: "struct",
    fields: [
      {
        name: "schemas",
        value: {
          kind: "list",
          elements: bundle.schemas.map(schemaToValue),
        },
      },
      { name: "root", value: typeRefToValue(bundle.root) },
      {
        name: "attachments",
        value: {
          kind: "list",
          elements: bundle.attachments.map(attachmentToValue),
        },
      },
    ],
  };
}

// r[impl binette.bundle.format]
export function schemaBundleFromValue(value: Value): SchemaBundle {
  const fields = expectStruct(value, "schema bundle", [
    "schemas",
    "root",
    "attachments",
  ]);
  return {
    schemas: expectList(fields[0], "schema bundle.schemas").map(schemaFromValue),
    root: typeRefFromValue(fields[1]),
    attachments: expectList(fields[2], "schema bundle.attachments").map(
      attachmentFromValue,
    ),
  };
}

export function encodeSchemaBundle(bundle: SchemaBundle): Uint8Array {
  return encodeSelfDescribed(schemaBundleToValue(bundle));
}

export function decodeSchemaBundle(bytes: Uint8Array): SchemaBundle {
  return schemaBundleFromValue(decodeSelfDescribed(bytes));
}

// r[impl binette.schema.format.type-ref+2]
export function typeRefToValue(typeRef: TypeRef): Value {
  switch (typeRef.kind) {
    case "concrete":
      return enumValue("concrete", {
        kind: "struct",
        fields: [
          { name: "type_id", value: { kind: "u64", value: typeRef.typeId } },
          {
            name: "args",
            value: { kind: "list", elements: typeRef.args.map(typeRefToValue) },
          },
        ],
      });
    case "var":
      return enumValue("var", { kind: "string", value: typeRef.name });
  }
}

// r[impl binette.schema.format.type-ref+2]
export function typeRefFromValue(value: Value): TypeRef {
  const enum_ = expectEnum(value, "type reference");
  switch (enum_.variant) {
    case "concrete": {
      const fields = expectStruct(enum_.payload, "type reference concrete", [
        "type_id",
        "args",
      ]);
      return concreteTypeRef(
        expectU64(fields[0], "type reference concrete.type_id"),
        expectList(fields[1], "type reference concrete.args").map(typeRefFromValue),
      );
    }
    case "var":
      return typeVar(expectString(enum_.payload, "type reference var"));
    default:
      throw new BinetteError(`unknown type reference variant ${enum_.variant}`);
  }
}

function schemaKindToValue(kind: SchemaKind): Value {
  switch (kind.kind) {
    case "primitive":
      return enumValue("primitive", { kind: "string", value: kind.primitive });
    case "struct":
      return enumValue("struct", {
        kind: "struct",
        fields: [
          { name: "name", value: { kind: "string", value: kind.name } },
          { name: "fields", value: fieldsToValue(kind.fields) },
        ],
      });
    case "enum":
      return enumValue("enum", {
        kind: "struct",
        fields: [
          { name: "name", value: { kind: "string", value: kind.name } },
          {
            name: "variants",
            value: { kind: "list", elements: kind.variants.map(variantToValue) },
          },
        ],
      });
    case "tuple":
      if (kind.elements.length === 0) {
        throw new BinetteError("schema kind tuple list must not be empty");
      }
      return enumValue("tuple", {
        kind: "list",
        elements: kind.elements.map(typeRefToValue),
      });
    case "list":
    case "set":
    case "option":
      return elementSchemaKind(kind.kind, kind.element);
    case "map":
      return enumValue("map", {
        kind: "struct",
        fields: [
          { name: "key", value: typeRefToValue(kind.key) },
          { name: "value", value: typeRefToValue(kind.value) },
        ],
      });
    case "array":
      if (kind.dimensions.length === 0) {
        throw new BinetteError("schema kind array.dimensions list must not be empty");
      }
      return enumValue("array", {
        kind: "struct",
        fields: [
          { name: "element", value: typeRefToValue(kind.element) },
          {
            name: "dimensions",
            value: {
              kind: "list",
              elements: kind.dimensions.map((value) => ({ kind: "u64", value })),
            },
          },
        ],
      });
    case "dynamic":
      return enumValue("dynamic", { kind: "unit" });
    case "external":
      return enumValue("external", {
        kind: "struct",
        fields: [
          { name: "kind", value: { kind: "string", value: kind.externalKind } },
          { name: "metadata", value: { kind: "dynamic", value: kind.metadata } },
        ],
      });
  }
}

// r[impl binette.schema.format.kind+2]
function schemaKindFromValue(value: Value): SchemaKind {
  const enum_ = expectEnum(value, "schema kind");
  switch (enum_.variant) {
    case "primitive": {
      const primitive = expectString(enum_.payload, "schema kind primitive");
      if (!isPrimitive(primitive)) {
        throw new BinetteError(`unknown primitive tag ${primitive}`);
      }
      return { kind: "primitive", primitive };
    }
    case "struct": {
      const fields = expectStruct(enum_.payload, "schema kind struct", [
        "name",
        "fields",
      ]);
      return {
        kind: "struct",
        name: expectString(fields[0], "schema kind struct.name"),
        fields: fieldsFromValue(fields[1]),
      };
    }
    case "enum": {
      const fields = expectStruct(enum_.payload, "schema kind enum", [
        "name",
        "variants",
      ]);
      return {
        kind: "enum",
        name: expectString(fields[0], "schema kind enum.name"),
        variants: expectList(fields[1], "schema kind enum.variants").map(
          variantFromValue,
        ),
      };
    }
    case "tuple": {
      const elements = expectList(enum_.payload, "schema kind tuple").map(
        typeRefFromValue,
      );
      if (elements.length === 0) {
        throw new BinetteError("schema kind tuple list must not be empty");
      }
      return { kind: "tuple", elements };
    }
    case "list":
      return { kind: "list", element: elementSchemaKindFromValue(enum_.payload) };
    case "set":
      return { kind: "set", element: elementSchemaKindFromValue(enum_.payload) };
    case "option":
      return { kind: "option", element: elementSchemaKindFromValue(enum_.payload) };
    case "map": {
      const fields = expectStruct(enum_.payload, "schema kind map", [
        "key",
        "value",
      ]);
      return {
        kind: "map",
        key: typeRefFromValue(fields[0]),
        value: typeRefFromValue(fields[1]),
      };
    }
    case "array": {
      const fields = expectStruct(enum_.payload, "schema kind array", [
        "element",
        "dimensions",
      ]);
      const dimensions = expectList(fields[1], "schema kind array.dimensions").map(
        (value) => expectU64(value, "schema kind array.dimension"),
      );
      if (dimensions.length === 0) {
        throw new BinetteError("schema kind array.dimensions list must not be empty");
      }
      return {
        kind: "array",
        element: typeRefFromValue(fields[0]),
        dimensions,
      };
    }
    case "dynamic":
      expectUnit(enum_.payload, "schema kind dynamic");
      return { kind: "dynamic" };
    case "external": {
      const fields = expectStruct(enum_.payload, "schema kind external", [
        "kind",
        "metadata",
      ]);
      return {
        kind: "external",
        externalKind: expectString(fields[0], "schema kind external.kind"),
        metadata: expectDynamic(fields[1], "schema kind external.metadata"),
      };
    }
    default:
      throw new BinetteError(`unknown schema kind variant ${enum_.variant}`);
  }
}

// r[impl binette.schema.format.fields+2]
function fieldsToValue(fields: Field[]): Value {
  return { kind: "list", elements: fields.map(fieldToValue) };
}

// r[impl binette.schema.format.fields+2]
function fieldsFromValue(value: Value): Field[] {
  return expectList(value, "field descriptors").map(fieldFromValue);
}

function fieldToValue(field: Field): Value {
  return {
    kind: "struct",
    fields: [
      { name: "name", value: { kind: "string", value: field.name } },
      { name: "type_ref", value: typeRefToValue(field.typeRef) },
    ],
  };
}

function fieldFromValue(value: Value): Field {
  const fields = expectStruct(value, "field descriptor", ["name", "type_ref"]);
  return {
    name: expectString(fields[0], "field descriptor.name"),
    typeRef: typeRefFromValue(fields[1]),
  };
}

// r[impl binette.schema.format.variants+2]
function variantToValue(variant: Variant): Value {
  return {
    kind: "struct",
    fields: [
      { name: "name", value: { kind: "string", value: variant.name } },
      { name: "index", value: { kind: "u32", value: variant.index } },
      { name: "payload", value: variantPayloadToValue(variant.payload) },
    ],
  };
}

// r[impl binette.schema.format.variants+2]
function variantFromValue(value: Value): Variant {
  const fields = expectStruct(value, "variant descriptor", [
    "name",
    "index",
    "payload",
  ]);
  return {
    name: expectString(fields[0], "variant descriptor.name"),
    index: expectU32(fields[1], "variant descriptor.index"),
    payload: variantPayloadFromValue(fields[2]),
  };
}

function variantPayloadToValue(payload: VariantPayload): Value {
  switch (payload.kind) {
    case "unit":
      return enumValue("unit", { kind: "unit" });
    case "newtype":
      return enumValue("newtype", typeRefToValue(payload.typeRef));
    case "tuple":
      return enumValue("tuple", {
        kind: "list",
        elements: payload.elements.map(typeRefToValue),
      });
    case "struct":
      return enumValue("struct", fieldsToValue(payload.fields));
  }
}

function variantPayloadFromValue(value: Value): VariantPayload {
  const enum_ = expectEnum(value, "variant payload");
  switch (enum_.variant) {
    case "unit":
      expectUnit(enum_.payload, "variant payload unit");
      return { kind: "unit" };
    case "newtype":
      return { kind: "newtype", typeRef: typeRefFromValue(enum_.payload) };
    case "tuple":
      return {
        kind: "tuple",
        elements: expectList(enum_.payload, "variant payload tuple").map(typeRefFromValue),
      };
    case "struct":
      return { kind: "struct", fields: fieldsFromValue(enum_.payload) };
    default:
      throw new BinetteError(`unknown variant payload variant ${enum_.variant}`);
  }
}

function attachmentToValue(attachment: AttachmentDeclaration): Value {
  return {
    kind: "struct",
    fields: [
      { name: "kind", value: { kind: "string", value: attachment.kind } },
      {
        name: "metadata_schema",
        value:
          attachment.metadataSchema === null
            ? { kind: "option", value: null }
            : { kind: "option", value: typeRefToValue(attachment.metadataSchema) },
      },
    ],
  };
}

function attachmentFromValue(value: Value): AttachmentDeclaration {
  const fields = expectStruct(value, "attachment declaration", [
    "kind",
    "metadata_schema",
  ]);
  const metadata = fields[1];
  if (metadata.kind !== "option") {
    throw new BinetteError(
      `expected option for attachment declaration.metadata_schema, got ${valueKind(metadata)}`,
    );
  }
  return {
    kind: expectString(fields[0], "attachment declaration.kind"),
    metadataSchema:
      metadata.value === null ? null : typeRefFromValue(metadata.value),
  };
}

function typeIdHashBytes(
  kind: SchemaKind,
  typeParams: string[],
  sentinelGroup: Set<TypeId> | null,
): Uint8Array {
  const writer = new HashWriter(sentinelGroup);
  writer.kind(kind, typeParams);
  return writer.finish();
}

function hashTypeId(bytes: Uint8Array): TypeId {
  const hash = blake3(bytes);
  let value = 0n;
  for (let index = 0; index < 8; index += 1) {
    const byte = hash[index];
    if (byte === undefined) {
      throw new BinetteError("BLAKE3 digest shorter than 8 bytes");
    }
    value |= BigInt(byte) << BigInt(index * 8);
  }
  return value;
}

function typeIdToLeBytes(typeId: TypeId): Uint8Array {
  if (typeId < 0n || typeId >= 1n << 64n) {
    throw new BinetteError("u64 out of range");
  }
  const bytes = new Uint8Array(8);
  for (let index = 0; index < 8; index += 1) {
    bytes[index] = Number((typeId >> BigInt(index * 8)) & 0xffn);
  }
  return bytes;
}

function bytesEqual(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }
  return true;
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const leftByte = left[index];
    const rightByte = right[index];
    if (leftByte === undefined || rightByte === undefined) {
      throw new BinetteError("byte comparison index out of bounds");
    }
    if (leftByte < rightByte) {
      return -1;
    }
    if (leftByte > rightByte) {
      return 1;
    }
  }
  return left.length - right.length;
}

class HashWriter {
  private readonly chunks: number[] = [];

  constructor(private readonly sentinelGroup: Set<TypeId> | null = null) {}

  finish(): Uint8Array {
    return Uint8Array.from(this.chunks);
  }

  kind(kind: SchemaKind, typeParams: string[]): void {
    switch (kind.kind) {
      case "primitive":
        this.string(kind.primitive);
        break;
      case "struct":
        this.string("struct");
        this.string(kind.name);
        this.typeParams(typeParams);
        this.len(kind.fields.length);
        for (const field of kind.fields) {
          this.string(field.name);
          this.typeRef(field.typeRef);
        }
        break;
      case "enum":
        this.string("enum");
        this.string(kind.name);
        this.typeParams(typeParams);
        this.len(kind.variants.length);
        for (const variant of kind.variants) {
          this.string(variant.name);
          this.u32(variant.index);
          this.variantPayload(variant.payload);
        }
        break;
      case "tuple":
        this.string("tuple");
        this.len(kind.elements.length);
        for (const element of kind.elements) {
          this.typeRef(element);
        }
        break;
      case "list":
        this.string("list");
        this.typeRef(kind.element);
        break;
      case "set":
        this.string("set");
        this.typeRef(kind.element);
        break;
      case "map":
        this.string("map");
        this.typeRef(kind.key);
        this.typeRef(kind.value);
        break;
      case "array":
        this.string("array");
        this.typeRef(kind.element);
        this.len(kind.dimensions.length);
        for (const dimension of kind.dimensions) {
          this.u64(dimension);
        }
        break;
      case "option":
        this.string("option");
        this.typeRef(kind.element);
        break;
      case "dynamic":
        this.string("dynamic");
        break;
      case "external":
        this.string("external");
        this.string(kind.externalKind);
        this.raw(encodeSelfDescribed(kind.metadata));
        break;
    }
  }

  string(value: string): void {
    const bytes = new TextEncoder().encode(value);
    this.len(bytes.length);
    this.raw(bytes);
  }

  typeParams(typeParams: string[]): void {
    this.len(typeParams.length);
    for (const typeParam of typeParams) {
      this.string(typeParam);
    }
  }

  typeRef(typeRef: TypeRef): void {
    switch (typeRef.kind) {
      case "concrete":
        this.string("concrete");
        if (this.sentinelGroup?.has(typeRef.typeId) === true) {
          this.u64(0n);
        } else {
          this.u64(typeRef.typeId);
        }
        if (typeRef.args.length > 0) {
          this.string("args");
          this.len(typeRef.args.length);
          for (const arg of typeRef.args) {
            this.typeRef(arg);
          }
        }
        break;
      case "var":
        this.string("var");
        this.string(typeRef.name);
        break;
    }
  }

  variantPayload(payload: VariantPayload): void {
    switch (payload.kind) {
      case "unit":
        this.string("unit");
        break;
      case "newtype":
        this.string("newtype");
        this.typeRef(payload.typeRef);
        break;
      case "tuple":
        this.string("tuple");
        this.len(payload.elements.length);
        for (const element of payload.elements) {
          this.typeRef(element);
        }
        break;
      case "struct":
        this.string("struct");
        this.len(payload.fields.length);
        for (const field of payload.fields) {
          this.string(field.name);
          this.typeRef(field.typeRef);
        }
        break;
    }
  }

  len(value: number): void {
    this.u32(value);
  }

  u32(value: number): void {
    if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff) {
      throw new BinetteError("u32 out of range");
    }
    this.chunks.push(value & 0xff);
    this.chunks.push((value >> 8) & 0xff);
    this.chunks.push((value >> 16) & 0xff);
    this.chunks.push((value >> 24) & 0xff);
  }

  u64(value: bigint): void {
    if (value < 0n || value >= 1n << 64n) {
      throw new BinetteError("u64 out of range");
    }
    for (let index = 0; index < 8; index += 1) {
      this.chunks.push(Number((value >> BigInt(index * 8)) & 0xffn));
    }
  }

  raw(bytes: Uint8Array): void {
    for (const byte of bytes) {
      this.chunks.push(byte);
    }
  }
}

function elementSchemaKind(variant: "list" | "set" | "option", element: TypeRef): Value {
  return enumValue(variant, {
    kind: "struct",
    fields: [{ name: "element", value: typeRefToValue(element) }],
  });
}

function elementSchemaKindFromValue(value: Value): TypeRef {
  const fields = expectStruct(value, "element schema kind", ["element"]);
  return typeRefFromValue(fields[0]);
}

function enumValue(variant: string, payload: Value): Value {
  return { kind: "enum", variant, payload };
}

type StructValues<Fields extends readonly string[]> = {
  [Index in keyof Fields]: Value;
};

function expectStruct<const Fields extends readonly string[]>(
  value: Value,
  context: string,
  expectedFields: Fields,
): StructValues<Fields> {
  if (value.kind !== "struct") {
    throw new BinetteError(
      `expected struct for ${context}, got ${valueKind(value)}`,
    );
  }

  const result: Array<Value | undefined> = new Array(expectedFields.length);
  for (const field of value.fields) {
    const index = expectedFields.indexOf(field.name);
    if (index === -1) {
      throw new BinetteError(`unexpected field ${field.name} in ${context}`);
    }
    if (result[index] !== undefined) {
      throw new BinetteError(`duplicate field ${field.name} in ${context}`);
    }
    result[index] = field.value;
  }

  const values: Value[] = [];
  for (let index = 0; index < result.length; index += 1) {
    const field = result[index];
    if (field === undefined) {
      throw new BinetteError(`missing field ${expectedFields[index]} in ${context}`);
    }
    values.push(field);
  }
  return values as StructValues<Fields>;
}

function expectEnum(value: Value, context: string): Extract<Value, { kind: "enum" }> {
  if (value.kind !== "enum") {
    throw new BinetteError(`expected enum for ${context}, got ${valueKind(value)}`);
  }
  return value;
}

function expectList(value: Value, context: string): Value[] {
  if (value.kind !== "list") {
    throw new BinetteError(`expected list for ${context}, got ${valueKind(value)}`);
  }
  return value.elements;
}

function expectString(value: Value, context: string): string {
  if (value.kind !== "string") {
    throw new BinetteError(`expected string for ${context}, got ${valueKind(value)}`);
  }
  return value.value;
}

function expectStringList(value: Value, context: string): string[] {
  return expectList(value, context).map((item) => expectString(item, context));
}

function expectU64(value: Value, context: string): bigint {
  if (value.kind !== "u64") {
    throw new BinetteError(`expected u64 for ${context}, got ${valueKind(value)}`);
  }
  return value.value;
}

function expectU32(value: Value, context: string): number {
  if (value.kind !== "u32") {
    throw new BinetteError(`expected u32 for ${context}, got ${valueKind(value)}`);
  }
  return value.value;
}

function expectUnit(value: Value, context: string): void {
  if (value.kind !== "unit") {
    throw new BinetteError(`expected unit for ${context}, got ${valueKind(value)}`);
  }
}

function expectDynamic(value: Value, context: string): Value {
  if (value.kind !== "dynamic") {
    throw new BinetteError(
      `expected dynamic value for ${context}, got ${valueKind(value)}`,
    );
  }
  return value.value;
}

function valueKind(value: Value): string {
  return value.kind;
}
