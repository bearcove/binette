import assert from "node:assert/strict";
import test from "node:test";

import {
  BinetteError,
  SchemaRegistry,
  concreteTypeRef,
  decodeCompact,
  encodeCompact,
  externalAttachmentSlots,
  primitiveTypeId,
  schemaTypeId,
  skipCompact,
  type Primitive,
  type Schema,
  type SchemaKind,
  type TypeRef,
  type Value,
} from "../src/index.js";

function bytes(...values: number[]): Uint8Array {
  return Uint8Array.from(values);
}

function primitiveRef(primitive: Primitive): TypeRef {
  return concreteTypeRef(primitiveTypeId(primitive));
}

function schema(kind: SchemaKind): Schema {
  return {
    id: schemaTypeId({ typeParams: [], kind }),
    typeParams: [],
    kind,
  };
}

function registryFor(root: Schema, schemas: Schema[]): SchemaRegistry {
  const registry = new SchemaRegistry();
  registry.installBundle({
    schemas,
    root: concreteTypeRef(root.id),
    attachments: [],
  });
  return registry;
}

// r[verify binette.mode.compact]
// r[verify binette.aggregate.struct.compact]
test("compact structs encode fields directly in schema order", () => {
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
  const registry = registryFor(account, [account, optionU32]);
  const value: Value = {
    kind: "struct",
    fields: [
      { name: "display_name", value: { kind: "string", value: "A" } },
      { name: "lucky", value: { kind: "option", value: { kind: "u32", value: 9 } } },
      { name: "id", value: { kind: "u64", value: 0x0102030405060708n } },
    ],
  };
  const encoded = encodeCompact(value, concreteTypeRef(account.id), registry);

  assert.deepEqual(
    encoded,
    bytes(8, 7, 6, 5, 4, 3, 2, 1, 1, 0, 0, 0, 65, 1, 9, 0, 0, 0),
  );
  assert.deepEqual(decodeCompact(encoded, concreteTypeRef(account.id), registry), {
    kind: "struct",
    fields: [
      { name: "id", value: { kind: "u64", value: 0x0102030405060708n } },
      { name: "display_name", value: { kind: "string", value: "A" } },
      { name: "lucky", value: { kind: "option", value: { kind: "u32", value: 9 } } },
    ],
  });
});

// r[verify binette.aggregate.enum.compact]
test("compact enums encode variant index followed by payload bytes", () => {
  const event = schema({
    kind: "enum",
    name: "Event",
    variants: [
      { name: "Started", index: 0, payload: { kind: "unit" } },
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
  const registry = registryFor(event, [event]);
  const value: Value = {
    kind: "enum",
    variant: "Failed",
    payload: {
      kind: "struct",
      fields: [
        { name: "message", value: { kind: "string", value: "no" } },
        { name: "code", value: { kind: "u16", value: 0x1234 } },
      ],
    },
  };

  const encoded = encodeCompact(value, concreteTypeRef(event.id), registry);

  assert.deepEqual(encoded, bytes(3, 0, 0, 0, 0x34, 0x12, 2, 0, 0, 0, 0x6e, 0x6f));
  assert.deepEqual(decodeCompact(encoded, concreteTypeRef(event.id), registry), {
    kind: "enum",
    variant: "Failed",
    payload: {
      kind: "struct",
      fields: [
        { name: "code", value: { kind: "u16", value: 0x1234 } },
        { name: "message", value: { kind: "string", value: "no" } },
      ],
    },
  });
});

test("compact sets sort and validate canonical element bytes", () => {
  const setU16 = schema({ kind: "set", element: primitiveRef("u16") });
  const registry = registryFor(setU16, [setU16]);
  const value: Value = {
    kind: "set",
    elements: [
      { kind: "u16", value: 1 },
      { kind: "u16", value: 256 },
    ],
  };

  const encoded = encodeCompact(value, concreteTypeRef(setU16.id), registry);

  assert.deepEqual(encoded, bytes(2, 0, 0, 0, 0, 1, 1, 0));
  assert.deepEqual(decodeCompact(encoded, concreteTypeRef(setU16.id), registry), {
    kind: "set",
    elements: [
      { kind: "u16", value: 256 },
      { kind: "u16", value: 1 },
    ],
  });
  assert.throws(
    () =>
      decodeCompact(
        bytes(2, 0, 0, 0, 1, 0, 0, 1),
        concreteTypeRef(setU16.id),
        registry,
      ),
    BinetteError,
  );
});

// r[verify binette.aggregate.schema-driven-skip]
// r[verify binette.aggregate.external-attachment]
test("compact skip walks schema and records external attachment slots", () => {
  const channel = schema({
    kind: "external",
    externalKind: "channel",
    metadata: { kind: "unit" },
  });
  const maybeChannel = schema({ kind: "option", element: concreteTypeRef(channel.id) });
  const message = schema({
    kind: "struct",
    name: "Message",
    fields: [
      { name: "id", typeRef: primitiveRef("u8") },
      { name: "stream", typeRef: concreteTypeRef(channel.id) },
      { name: "maybe", typeRef: concreteTypeRef(maybeChannel.id) },
    ],
  });
  const registry = registryFor(message, [message, channel, maybeChannel]);
  const encoded = encodeCompact(
    {
      kind: "struct",
      fields: [
        { name: "id", value: { kind: "u8", value: 7 } },
        { name: "stream", value: { kind: "externalAttachment" } },
        {
          name: "maybe",
          value: { kind: "option", value: { kind: "externalAttachment" } },
        },
      ],
    },
    concreteTypeRef(message.id),
    registry,
  );

  assert.deepEqual(encoded, bytes(7, 1));
  assert.equal(skipCompact(encoded, concreteTypeRef(message.id), registry), 2);
  assert.deepEqual(
    externalAttachmentSlots(encoded, concreteTypeRef(message.id), registry),
    [
      { bytePosition: 1, kind: "channel", metadata: { kind: "unit" } },
      { bytePosition: 2, kind: "channel", metadata: { kind: "unit" } },
    ],
  );
});

test("compact dynamic fields contain exactly one self-described value", () => {
  const dynamic = schema({ kind: "dynamic" });
  const registry = registryFor(dynamic, [dynamic]);
  const value: Value = {
    kind: "dynamic",
    value: { kind: "string", value: "x" },
  };

  const encoded = encodeCompact(value, concreteTypeRef(dynamic.id), registry);

  assert.deepEqual(encoded, bytes(0x0f, 1, 0, 0, 0, 0x78));
  assert.deepEqual(decodeCompact(encoded, concreteTypeRef(dynamic.id), registry), value);
});

// r[verify binette.scalar.never]
test("compact never values are rejected", () => {
  assert.throws(
    () => encodeCompact({ kind: "unit" }, primitiveRef("never"), new SchemaRegistry()),
    BinetteError,
  );
  assert.throws(
    () => decodeCompact(bytes(), primitiveRef("never"), new SchemaRegistry()),
    BinetteError,
  );
});
