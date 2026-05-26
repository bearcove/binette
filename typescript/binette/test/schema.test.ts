import assert from "node:assert/strict";
import test from "node:test";

import {
  BinetteError,
  concreteTypeRef,
  decodeSchema,
  decodeSchemaBundle,
  encodeSchema,
  encodeSchemaBundle,
  encodeSelfDescribed,
  primitiveForTypeId,
  primitiveTypeId,
  schemaBundleFromValue,
  schemaBundleToValue,
  schemaFromValue,
  schemaToValue,
  schemaTypeId,
  typeRefFromValue,
  typeRefToValue,
  typeVar,
  type Primitive,
  type Schema,
  type SchemaBundle,
  type SchemaKind,
  type TypeRef,
  type Value,
} from "../src/index.js";

function primitiveRef(primitive: Primitive): TypeRef {
  return concreteTypeRef(primitiveTypeId(primitive));
}

function schema(kind: SchemaKind, typeParams: string[] = []): Schema {
  return {
    id: schemaTypeId({ typeParams, kind }),
    typeParams,
    kind,
  };
}

function structFields(value: Value): Array<{ name: string; value: Value }> {
  assert.equal(value.kind, "struct");
  return value.fields;
}

// r[verify binette.schema.encoding.self-describing]
// r[verify binette.schema.format+2]
// r[verify binette.schema.format.kind+2]
// r[verify binette.schema.format.fields+2]
// r[verify binette.schema.format.type-ref+2]
test("schema values round-trip through self-describing bytes", () => {
  const optionU32 = schema({ kind: "option", element: primitiveRef("u32") });
  const account = schema({
    kind: "struct",
    name: "Account",
    fields: [
      { name: "id", typeRef: primitiveRef("u64") },
      { name: "display_name", typeRef: primitiveRef("string") },
      { name: "lucky", typeRef: concreteTypeRef(optionU32.id) },
    ],
  });

  const value = schemaToValue(account);
  assert.deepEqual(
    structFields(value).map((field) => field.name),
    ["id", "type_params", "kind"],
  );

  assert.deepEqual(decodeSchema(encodeSchema(account)), account);
  assert.deepEqual(typeRefFromValue(typeRefToValue(typeVar("T"))), typeVar("T"));
});

// r[verify binette.schema.format.variants+2]
// r[verify binette.schema.format.kind+2]
// r[verify binette.schema.fields]
test("schema format encodes enum variants and payloads", () => {
  const event = schema({
    kind: "enum",
    name: "Event",
    variants: [
      { name: "Started", index: 0, payload: { kind: "unit" } },
      {
        name: "Renamed",
        index: 1,
        payload: { kind: "newtype", typeRef: primitiveRef("string") },
      },
      {
        name: "Moved",
        index: 2,
        payload: {
          kind: "tuple",
          elements: [primitiveRef("u32"), primitiveRef("u32")],
        },
      },
      {
        name: "Failed",
        index: 3,
        payload: {
          kind: "struct",
          fields: [
            { name: "code", typeRef: primitiveRef("u16") },
            { name: "message", typeRef: primitiveRef("string") },
          ],
        },
      },
    ],
  });

  assert.deepEqual(decodeSchema(encodeSchema(event)), event);

  const kind = structFields(schemaToValue(event))[2]?.value;
  assert.equal(kind?.kind, "enum");
  assert.equal(kind.variant, "enum");
  assert.equal(structFields(kind.payload)[1]?.value.kind, "list");
});

// r[verify binette.bundle.format]
// r[verify binette.bundle.model]
// r[verify binette.bundle.attachments]
test("schema bundles round-trip with root and attachments", () => {
  const account = schema({
    kind: "struct",
    name: "Account",
    fields: [
      { name: "id", typeRef: primitiveRef("u64") },
      { name: "display_name", typeRef: primitiveRef("string") },
    ],
  });
  const bundle: SchemaBundle = {
    schemas: [account],
    root: concreteTypeRef(account.id),
    attachments: [
      {
        kind: "channel",
        metadataSchema: primitiveRef("string"),
      },
      {
        kind: "cancel",
        metadataSchema: null,
      },
    ],
  };

  assert.deepEqual(decodeSchemaBundle(encodeSchemaBundle(bundle)), bundle);
  assert.deepEqual(schemaBundleFromValue(schemaBundleToValue(bundle)), bundle);
});

// r[verify binette.schema.external]
// r[verify binette.schema.format.kind+2]
// r[verify binette.type-id.hash.external]
test("external schema metadata is a binette value", () => {
  const external = schema({
    kind: "external",
    externalKind: "channel",
    metadata: {
      kind: "struct",
      fields: [
        { name: "transport", value: { kind: "string", value: "ordered" } },
        { name: "version", value: { kind: "u32", value: 1 } },
      ],
    },
  });

  assert.deepEqual(decodeSchema(encodeSchema(external)), external);
});

// r[verify binette.schema.format+2]
test("schema format rejects extra duplicate and missing fields", () => {
  const valid = schemaToValue(
    schema({ kind: "primitive", primitive: "u8" }),
  );

  assert.throws(() => {
    schemaFromValue({
      kind: "struct",
      fields: [
        ...structFields(valid),
        { name: "extra", value: { kind: "unit" } },
      ],
    });
  }, BinetteError);

  assert.throws(() => {
    const fields = structFields(valid);
    schemaFromValue({
      kind: "struct",
      fields: [...fields, fields[0] as { name: string; value: Value }],
    });
  }, BinetteError);

  assert.throws(() => {
    schemaFromValue({
      kind: "struct",
      fields: structFields(valid).filter((field) => field.name !== "kind"),
    });
  }, BinetteError);
});

// r[verify binette.schema.extension]
test("schema format rejects extension as a compact schema kind", () => {
  const extensionKind: Value = {
    kind: "enum",
    variant: "extension",
    payload: { kind: "unit" },
  };
  const value: Value = {
    kind: "struct",
    fields: [
      { name: "id", value: { kind: "u64", value: 0n } },
      { name: "type_params", value: { kind: "list", elements: [] } },
      { name: "kind", value: extensionKind },
    ],
  };

  assert.throws(() => schemaFromValue(value), BinetteError);
});

// r[verify binette.schema.primitive]
// r[verify binette.type-id.hash.primitives]
test("primitive schema IDs are well-known and reversible", () => {
  const u8 = primitiveTypeId("u8");

  assert.equal(primitiveForTypeId(u8), "u8");
  assert.equal(primitiveForTypeId(0xffff_ffff_ffff_ffffn), null);
  assert.equal(schemaTypeId({ typeParams: [], kind: { kind: "primitive", primitive: "u8" } }), u8);
});

// r[verify binette.schema.model]
// r[verify binette.schema.kinds]
// r[verify binette.schema.name]
// r[verify binette.schema.array]
// r[verify binette.schema.dynamic]
// r[verify binette.schema.tuple]
// r[verify binette.schema.type-ref]
// r[verify binette.type-id]
// r[verify binette.hash.recursive.non-recursive]
// r[verify binette.type-id.hash]
// r[verify binette.type-id.hash.struct]
// r[verify binette.type-id.hash.enum]
// r[verify binette.type-id.hash.container]
// r[verify binette.type-id.hash.tuple]
// r[verify binette.type-id.hash.typeref]
// r[verify binette.type-id.hash.dynamic]
test("schema type IDs change with canonical schema content", () => {
  const named = schema({
    kind: "struct",
    name: "Account",
    fields: [{ name: "id", typeRef: primitiveRef("u64") }],
  });
  const renamed = schema({
    kind: "struct",
    name: "User",
    fields: [{ name: "id", typeRef: primitiveRef("u64") }],
  });
  const reordered = schema({
    kind: "struct",
    name: "Account",
    fields: [
      { name: "name", typeRef: primitiveRef("string") },
      { name: "id", typeRef: primitiveRef("u64") },
    ],
  });
  const wrapper = schema(
    {
      kind: "struct",
      name: "Wrapper",
      fields: [{ name: "value", typeRef: typeVar("T") }],
    },
    ["T"],
  );
  const dynamic = schema({ kind: "dynamic" });
  const tuple = schema({
    kind: "tuple",
    elements: [primitiveRef("u16"), primitiveRef("string")],
  });
  const array = schema({
    kind: "array",
    element: primitiveRef("u16"),
    dimensions: [4n],
  });

  assert.equal(named.id, schemaTypeId(named));
  assert.notEqual(named.id, renamed.id);
  assert.notEqual(named.id, reordered.id);
  assert.notEqual(wrapper.id, dynamic.id);
  assert.notEqual(tuple.id, array.id);
});

test("schema format rejects mismatched declared IDs", () => {
  const valid = schema({ kind: "primitive", primitive: "u8" });
  const value = schemaToValue({ ...valid, id: valid.id + 1n });

  assert.throws(() => schemaFromValue(value), BinetteError);
});

test("schema bytes can be embedded as ordinary self-described values", () => {
  const account = schema({
    kind: "struct",
    name: "Account",
    fields: [{ name: "id", typeRef: primitiveRef("u64") }],
  });

  assert.deepEqual(encodeSchema(account), encodeSelfDescribed(schemaToValue(account)));
});
