import {
  BinetteError,
  decodeSelfDescribed,
  encodeSelfDescribed,
  type Value,
} from "./value.js";

import {
  decodeSchemaBundle,
  schemaBundleFromValue,
  schemaBundleToValue,
  type SchemaBundle,
  type TypeId,
} from "./schema.js";

// r[impl binette.bundle.dump]
export type SchemaDump = {
  bundle: SchemaBundle;
  metadata: ProducerMetadata;
};

export type ProducerMetadata = {
  declarations: DeclarationMetadata[];
};

export type DeclarationMetadata = {
  typeId: TypeId;
  sourceName: string | null;
  documentation: string | null;
  sourceLocation: string | null;
  fields: FieldMetadata[];
};

export type FieldMetadata = {
  name: string;
  defaultability: Defaultability;
  documentation: string | null;
  sourceLocation: string | null;
};

// r[impl binette.compat.defaultability-metadata]
export type Defaultability =
  | { kind: "none" }
  | { kind: "opaque" }
  | { kind: "literal"; value: Value };

// r[impl binette.bundle.snapshot]
export type SchemaSnapshot = {
  dumps: SchemaDump[];
};

// r[impl binette.bundle.dump]
export function schemaDumpToValue(dump: SchemaDump): Value {
  return {
    kind: "struct",
    fields: [
      { name: "bundle", value: schemaBundleToValue(dump.bundle) },
      { name: "metadata", value: producerMetadataToValue(dump.metadata) },
    ],
  };
}

// r[impl binette.bundle.dump]
export function schemaDumpFromValue(value: Value): SchemaDump {
  const fields = expectStruct(value, "schema dump", ["bundle", "metadata"]);
  return {
    bundle: schemaBundleFromValue(fields[0]),
    metadata: producerMetadataFromValue(fields[1]),
  };
}

export function encodeSchemaDump(dump: SchemaDump): Uint8Array {
  return encodeSelfDescribed(schemaDumpToValue(dump));
}

export function decodeSchemaDump(bytes: Uint8Array): SchemaDump {
  return schemaDumpFromValue(decodeSelfDescribed(bytes));
}

// r[impl binette.bundle.snapshot]
export function schemaSnapshotToValue(snapshot: SchemaSnapshot): Value {
  return {
    kind: "struct",
    fields: [
      {
        name: "dumps",
        value: {
          kind: "list",
          elements: snapshot.dumps.map(schemaDumpToValue),
        },
      },
    ],
  };
}

// r[impl binette.bundle.snapshot]
export function schemaSnapshotFromValue(value: Value): SchemaSnapshot {
  const fields = expectStruct(value, "schema snapshot", ["dumps"]);
  return {
    dumps: expectList(fields[0], "schema snapshot.dumps").map(schemaDumpFromValue),
  };
}

export function encodeSchemaSnapshot(snapshot: SchemaSnapshot): Uint8Array {
  return encodeSelfDescribed(schemaSnapshotToValue(snapshot));
}

export function decodeSchemaSnapshot(bytes: Uint8Array): SchemaSnapshot {
  return schemaSnapshotFromValue(decodeSelfDescribed(bytes));
}

function producerMetadataToValue(metadata: ProducerMetadata): Value {
  return {
    kind: "struct",
    fields: [
      {
        name: "declarations",
        value: {
          kind: "list",
          elements: metadata.declarations.map(declarationMetadataToValue),
        },
      },
    ],
  };
}

function producerMetadataFromValue(value: Value): ProducerMetadata {
  const fields = expectStruct(value, "producer metadata", ["declarations"]);
  return {
    declarations: expectList(fields[0], "producer metadata.declarations").map(
      declarationMetadataFromValue,
    ),
  };
}

function declarationMetadataToValue(metadata: DeclarationMetadata): Value {
  return {
    kind: "struct",
    fields: [
      { name: "type_id", value: { kind: "u64", value: metadata.typeId } },
      { name: "source_name", value: optionalStringToValue(metadata.sourceName) },
      { name: "documentation", value: optionalStringToValue(metadata.documentation) },
      {
        name: "source_location",
        value: optionalStringToValue(metadata.sourceLocation),
      },
      {
        name: "fields",
        value: {
          kind: "list",
          elements: metadata.fields.map(fieldMetadataToValue),
        },
      },
    ],
  };
}

function declarationMetadataFromValue(value: Value): DeclarationMetadata {
  const fields = expectStruct(value, "declaration metadata", [
    "type_id",
    "source_name",
    "documentation",
    "source_location",
    "fields",
  ]);
  return {
    typeId: expectU64(fields[0], "declaration metadata.type_id"),
    sourceName: expectOptionalString(fields[1], "declaration metadata.source_name"),
    documentation: expectOptionalString(fields[2], "declaration metadata.documentation"),
    sourceLocation: expectOptionalString(fields[3], "declaration metadata.source_location"),
    fields: expectList(fields[4], "declaration metadata.fields").map(
      fieldMetadataFromValue,
    ),
  };
}

function fieldMetadataToValue(metadata: FieldMetadata): Value {
  return {
    kind: "struct",
    fields: [
      { name: "name", value: { kind: "string", value: metadata.name } },
      {
        name: "defaultability",
        value: defaultabilityToValue(metadata.defaultability),
      },
      { name: "documentation", value: optionalStringToValue(metadata.documentation) },
      {
        name: "source_location",
        value: optionalStringToValue(metadata.sourceLocation),
      },
    ],
  };
}

function fieldMetadataFromValue(value: Value): FieldMetadata {
  const fields = expectStruct(value, "field metadata", [
    "name",
    "defaultability",
    "documentation",
    "source_location",
  ]);
  return {
    name: expectString(fields[0], "field metadata.name"),
    defaultability: defaultabilityFromValue(fields[1]),
    documentation: expectOptionalString(fields[2], "field metadata.documentation"),
    sourceLocation: expectOptionalString(fields[3], "field metadata.source_location"),
  };
}

function defaultabilityToValue(defaultability: Defaultability): Value {
  switch (defaultability.kind) {
    case "none":
      return { kind: "enum", variant: "none", payload: { kind: "unit" } };
    case "opaque":
      return { kind: "enum", variant: "opaque", payload: { kind: "unit" } };
    case "literal":
      return {
        kind: "enum",
        variant: "literal",
        payload: { kind: "dynamic", value: defaultability.value },
      };
  }
}

function defaultabilityFromValue(value: Value): Defaultability {
  if (value.kind !== "enum") {
    throw new BinetteError(`expected enum for defaultability, got ${value.kind}`);
  }
  switch (value.variant) {
    case "none":
      expectUnit(value.payload, "defaultability none");
      return { kind: "none" };
    case "opaque":
      expectUnit(value.payload, "defaultability opaque");
      return { kind: "opaque" };
    case "literal":
      if (value.payload.kind !== "dynamic") {
        throw new BinetteError(
          `expected dynamic value for defaultability literal, got ${value.payload.kind}`,
        );
      }
      return { kind: "literal", value: value.payload.value };
    default:
      throw new BinetteError(`unknown defaultability variant ${value.variant}`);
  }
}

function optionalStringToValue(value: string | null): Value {
  return value === null
    ? { kind: "option", value: null }
    : { kind: "option", value: { kind: "string", value } };
}

function expectOptionalString(value: Value, context: string): string | null {
  if (value.kind !== "option") {
    throw new BinetteError(`expected option for ${context}, got ${value.kind}`);
  }
  if (value.value === null) {
    return null;
  }
  return expectString(value.value, context);
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
    throw new BinetteError(`expected struct for ${context}, got ${value.kind}`);
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

function expectList(value: Value, context: string): Value[] {
  if (value.kind !== "list") {
    throw new BinetteError(`expected list for ${context}, got ${value.kind}`);
  }
  return value.elements;
}

function expectString(value: Value, context: string): string {
  if (value.kind !== "string") {
    throw new BinetteError(`expected string for ${context}, got ${value.kind}`);
  }
  return value.value;
}

function expectU64(value: Value, context: string): TypeId {
  if (value.kind !== "u64") {
    throw new BinetteError(`expected u64 for ${context}, got ${value.kind}`);
  }
  return value.value;
}

function expectUnit(value: Value, context: string): void {
  if (value.kind !== "unit") {
    throw new BinetteError(`expected unit for ${context}, got ${value.kind}`);
  }
}
