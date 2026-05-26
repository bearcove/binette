import assert from "node:assert/strict";
import test from "node:test";

import {
  PlanError,
  concreteTypeRef,
  primitiveTypeId,
  readerPlanForBundles,
  schemaTypeId,
  type Primitive,
  type Schema,
  type SchemaBundle,
  type SchemaKind,
  type TypeRef,
} from "../src/index.js";

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

function bundle(root: Schema, schemas: Schema[] = [root]): SchemaBundle {
  return {
    schemas,
    root: concreteTypeRef(root.id),
    attachments: [],
  };
}

// r[verify binette.compat.plan]
test("same bundle roots plan structurally", () => {
  const account = schema({
    kind: "struct",
    name: "Account",
    fields: [
      { name: "id", typeRef: primitiveRef("u64") },
      { name: "name", typeRef: primitiveRef("string") },
    ],
  });

  const plan = readerPlanForBundles(bundle(account), bundle(account));

  assert.equal(plan.root.kind, "struct");
  assert.deepEqual(plan.root.fields.map((field) => field.kind), ["read", "read"]);
  assert.deepEqual(plan.root.fields[0], {
    kind: "read",
    writerIndex: 0,
    readerIndex: 0,
    name: "id",
    plan: { kind: "primitive", primitive: "u64" },
  });
  assert.deepEqual(plan.root.fields[1], {
    kind: "read",
    writerIndex: 1,
    readerIndex: 1,
    name: "name",
    plan: { kind: "primitive", primitive: "string" },
  });
});

// r[verify binette.compat.plan]
test("bundle roots plan without a local reader shape", () => {
  const writer = accountSchema([
    { name: "id", typeRef: primitiveRef("u64") },
    { name: "name", typeRef: primitiveRef("string") },
  ]);
  const reader = accountSchema([
    { name: "name", typeRef: primitiveRef("string") },
    { name: "id", typeRef: primitiveRef("u64") },
  ]);

  const plan = readerPlanForBundles(bundle(writer), bundle(reader));

  assert.equal(plan.root.kind, "struct");
});

// r[verify binette.compat.field-matching]
test("struct fields are planned by name, not position", () => {
  const writer = accountSchema([
    { name: "id", typeRef: primitiveRef("u64") },
    { name: "name", typeRef: primitiveRef("string") },
  ]);
  const reader = accountSchema([
    { name: "name", typeRef: primitiveRef("string") },
    { name: "id", typeRef: primitiveRef("u64") },
  ]);

  const plan = readerPlanForBundles(bundle(writer), bundle(reader));

  assert.equal(plan.root.kind, "struct");
  assert.deepEqual(plan.root.fields.map((field) => field.kind), ["read", "read"]);
  assert.deepEqual(plan.root.fields[0], {
    kind: "read",
    writerIndex: 0,
    readerIndex: 1,
    name: "id",
    plan: { kind: "primitive", primitive: "u64" },
  });
  assert.deepEqual(plan.root.fields[1], {
    kind: "read",
    writerIndex: 1,
    readerIndex: 0,
    name: "name",
    plan: { kind: "primitive", primitive: "string" },
  });
});

// r[verify binette.compat.skip-unknown]
test("writer-only struct fields become skip steps", () => {
  const writer = accountSchema([
    { name: "id", typeRef: primitiveRef("u64") },
    { name: "name", typeRef: primitiveRef("string") },
    { name: "nickname", typeRef: primitiveRef("string") },
  ]);
  const reader = accountSchema([
    { name: "id", typeRef: primitiveRef("u64") },
    { name: "name", typeRef: primitiveRef("string") },
  ]);

  const plan = readerPlanForBundles(bundle(writer), bundle(reader));

  assert.equal(plan.root.kind, "struct");
  assert.deepEqual(plan.root.fields[2], {
    kind: "skip",
    writerIndex: 2,
    name: "nickname",
    writerType: primitiveRef("string"),
  });
});

// r[verify binette.compat.fill-defaults]
test("reader-only struct fields fail without a default provider", () => {
  const writer = accountSchema([{ name: "id", typeRef: primitiveRef("u64") }]);
  const reader = accountSchema([
    { name: "id", typeRef: primitiveRef("u64") },
    { name: "name", typeRef: primitiveRef("string") },
  ]);

  assert.throws(
    () => readerPlanForBundles(bundle(writer), bundle(reader)),
    (error) =>
      error instanceof PlanError &&
      error.kind === "missingReaderField" &&
      error.path === "$",
  );
});

// r[verify binette.compat.type-compat]
// r[verify binette.compat.type-compat.basic]
test("incompatible field types fail before payload decode", () => {
  const writer = accountSchema([{ name: "id", typeRef: primitiveRef("u64") }]);
  const reader = accountSchema([{ name: "id", typeRef: primitiveRef("string") }]);

  assert.throws(
    () => readerPlanForBundles(bundle(writer), bundle(reader)),
    (error) =>
      error instanceof PlanError &&
      error.kind === "typeMismatch" &&
      error.path === "$.id",
  );
});

// r[verify binette.compat.tuple]
test("tuple arity mismatch fails before payload decode", () => {
  const writer = schema({
    kind: "tuple",
    elements: [primitiveRef("u16"), primitiveRef("u16")],
  });
  const reader = schema({
    kind: "tuple",
    elements: [primitiveRef("u16"), primitiveRef("u16"), primitiveRef("u16")],
  });

  assert.throws(
    () => readerPlanForBundles(bundle(writer), bundle(reader)),
    (error) =>
      error instanceof PlanError &&
      error.kind === "unsupported" &&
      error.path === "$",
  );
});

// r[verify binette.compat.enum]
// r[verify binette.compat.enum.payload]
test("enum variants are planned by name, not index", () => {
  const writer = eventSchema([
    { name: "Started", index: 0, payload: { kind: "unit" } },
    {
      name: "Moved",
      index: 1,
      payload: {
        kind: "tuple",
        elements: [primitiveRef("u32"), primitiveRef("u32")],
      },
    },
  ]);
  const reader = eventSchema([
    {
      name: "Moved",
      index: 0,
      payload: {
        kind: "tuple",
        elements: [primitiveRef("u32"), primitiveRef("u32")],
      },
    },
    { name: "Started", index: 1, payload: { kind: "unit" } },
  ]);

  const plan = readerPlanForBundles(bundle(writer), bundle(reader));

  assert.equal(plan.root.kind, "enum");
  assert.deepEqual(plan.root.variants[0], {
    kind: "read",
    writerIndex: 0,
    readerIndex: 1,
    name: "Started",
    payload: { kind: "unit" },
  });
  assert.equal(plan.root.variants[1]?.kind, "read");
  assert.equal(plan.root.variants[1]?.name, "Moved");
});

// r[verify binette.compat.enum.missing-variant]
// r[verify binette.compat.enum.unknown-variant]
test("writer-only enum variants become runtime reject steps", () => {
  const writer = eventSchema([
    { name: "Started", index: 0, payload: { kind: "unit" } },
    {
      name: "Failed",
      index: 1,
      payload: {
        kind: "struct",
        fields: [{ name: "code", typeRef: primitiveRef("u16") }],
      },
    },
  ]);
  const reader = eventSchema([
    { name: "Started", index: 0, payload: { kind: "unit" } },
  ]);

  const plan = readerPlanForBundles(bundle(writer), bundle(reader));

  assert.equal(plan.root.kind, "enum");
  assert.deepEqual(plan.root.variants[1], {
    kind: "reject",
    writerIndex: 1,
    name: "Failed",
  });
});

// r[verify binette.compat.enum.payload]
test("enum payload mismatch fails before payload decode", () => {
  const writer = eventSchema([
    {
      name: "Moved",
      index: 0,
      payload: { kind: "newtype", typeRef: primitiveRef("u32") },
    },
  ]);
  const reader = eventSchema([
    {
      name: "Moved",
      index: 0,
      payload: { kind: "newtype", typeRef: primitiveRef("string") },
    },
  ]);

  assert.throws(
    () => readerPlanForBundles(bundle(writer), bundle(reader)),
    (error) =>
      error instanceof PlanError &&
      error.kind === "typeMismatch" &&
      error.path === "$.Moved",
  );
});

function accountSchema(fields: Array<{ name: string; typeRef: TypeRef }>): Schema {
  return schema({ kind: "struct", name: "Account", fields });
}

function eventSchema(
  variants: Array<Extract<SchemaKind, { kind: "enum" }>["variants"][number]>,
): Schema {
  return schema({ kind: "enum", name: "Event", variants });
}
