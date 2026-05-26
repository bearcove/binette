import assert from "node:assert/strict";
import test from "node:test";

import {
  BinetteError,
  SchemaRegistry,
  concreteTypeRef,
  primitiveTypeId,
  recursiveSchemaTypeIds,
  schemaTypeId,
  typeVar,
  type Primitive,
  type Schema,
  type SchemaBundle,
  type SchemaKind,
  type TypeId,
  type TypeRef,
  type VariantPayload,
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

function bundle(root: TypeId, schemas: Schema[]): SchemaBundle {
  return {
    schemas,
    root: concreteTypeRef(root),
    attachments: [],
  };
}

function only<T>(values: readonly T[], context: string): T {
  const value = values[0];
  assert.notEqual(value, undefined, context);
  return value as T;
}

// r[verify binette.schema.registry.install]
// r[verify binette.schema.primitive]
// r[verify binette.type-id.hash.primitives]
test("registry treats primitive schemas as built-in", () => {
  const typeId = primitiveTypeId("u8");
  const registry = new SchemaRegistry();

  registry.installBundle({
    schemas: [
      {
        id: typeId,
        typeParams: [],
        kind: { kind: "primitive", primitive: "u8" },
      },
    ],
    root: concreteTypeRef(typeId),
    attachments: [],
  });

  assert.equal(registry.contains(typeId), true);
  assert.equal(registry.get(typeId), undefined);
  assert.equal(registry.size, 0);
  assert.equal(registry.isEmpty, true);
});

// r[verify binette.bundle.registry]
// r[verify binette.schema.registry.install]
test("registry installs bundles after verifying declared ids and root refs", () => {
  const optionU32 = schema({ kind: "option", element: primitiveRef("u32") });
  const account = schema({
    kind: "struct",
    name: "Account",
    fields: [
      { name: "id", typeRef: primitiveRef("u64") },
      { name: "lucky", typeRef: concreteTypeRef(optionU32.id) },
    ],
  });
  const registry = new SchemaRegistry();

  registry.installBundle(bundle(account.id, [account, optionU32]));

  assert.equal(registry.contains(account.id), true);
  assert.equal(registry.contains(optionU32.id), true);
  assert.equal(registry.size, 2);
});

// r[verify binette.bundle.self-contained]
// r[verify binette.schema.registry+2]
test("self-contained validation requires all transitive schemas unless installed", () => {
  const optionU32 = schema({ kind: "option", element: primitiveRef("u32") });
  const account = schema({
    kind: "struct",
    name: "Account",
    fields: [{ name: "lucky", typeRef: concreteTypeRef(optionU32.id) }],
  });
  const missingOption = bundle(account.id, [account]);

  assert.throws(
    () => new SchemaRegistry().validateSelfContainedBundle(missingOption),
    BinetteError,
  );

  const registry = new SchemaRegistry();
  registry.installBundle(bundle(optionU32.id, [optionU32]));
  registry.validateSelfContainedBundle(missingOption);
});

test("registry rejects schema id mismatches and bad type arguments", () => {
  const account = schema({
    kind: "struct",
    name: "Account",
    fields: [{ name: "id", typeRef: primitiveRef("u64") }],
  });
  assert.throws(
    () =>
      new SchemaRegistry().installBundle(
        bundle(account.id + 1n, [{ ...account, id: account.id + 1n }]),
      ),
    BinetteError,
  );

  const wrapper = schema(
    {
      kind: "struct",
      name: "Wrapper",
      fields: [{ name: "value", typeRef: typeVar("T") }],
    },
    ["T"],
  );
  assert.throws(
    () =>
      new SchemaRegistry().installBundle({
        schemas: [wrapper],
        root: concreteTypeRef(wrapper.id),
        attachments: [],
      }),
    BinetteError,
  );
});

// r[verify binette.hash.recursive]
// r[verify binette.schema.registry.recursive]
test("registry installs a self-recursive schema group", () => {
  const provisional = 1n;
  const node = rewriteSchemaTypeIds(
    {
      id: provisional,
      typeParams: [],
      kind: {
        kind: "struct",
        name: "Node",
        fields: [{ name: "next", typeRef: concreteTypeRef(provisional) }],
      },
    },
    [[provisional, only(recursiveSchemaTypeIds([
      {
        id: provisional,
        typeParams: [],
        kind: {
          kind: "struct",
          name: "Node",
          fields: [{ name: "next", typeRef: concreteTypeRef(provisional) }],
        },
      },
    ]), "recursive node id")]],
  );

  const registry = new SchemaRegistry();
  registry.installBundle(bundle(node.id, [node]));

  assert.equal(registry.contains(node.id), true);
  assert.equal(registry.size, 1);
});

// r[verify binette.hash.recursive]
// r[verify binette.schema.registry.recursive]
test("recursive hashing is stable for mutual recursion", () => {
  const provisionalFirst = 1n;
  const provisionalSecond = 2n;
  const first = recursiveSchema("First", "second", provisionalFirst, provisionalSecond);
  const second = recursiveSchema("Second", "first", provisionalSecond, provisionalFirst);

  const forward = recursiveSchemaTypeIds([first, second]);
  const reverse = recursiveSchemaTypeIds([second, first]);
  assert.deepEqual(forward, [reverse[1], reverse[0]]);

  const replacements: Array<[TypeId, TypeId]> = [
    [provisionalFirst, mustTypeId(forward[0])],
    [provisionalSecond, mustTypeId(forward[1])],
  ];
  const finalFirst = rewriteSchemaTypeIds(first, replacements);
  const finalSecond = rewriteSchemaTypeIds(second, replacements);

  const registry = new SchemaRegistry();
  registry.installBundle(bundle(finalFirst.id, [finalSecond, finalFirst]));

  assert.equal(registry.contains(finalFirst.id), true);
  assert.equal(registry.contains(finalSecond.id), true);
  assert.equal(registry.size, 2);
});

function recursiveSchema(
  name: string,
  fieldName: string,
  id: TypeId,
  target: TypeId,
): Schema {
  return {
    id,
    typeParams: [],
    kind: {
      kind: "struct",
      name,
      fields: [{ name: fieldName, typeRef: concreteTypeRef(target) }],
    },
  };
}

function rewriteSchemaTypeIds(
  schema: Schema,
  replacements: ReadonlyArray<readonly [TypeId, TypeId]>,
): Schema {
  return {
    id: replaceTypeId(schema.id, replacements),
    typeParams: [...schema.typeParams],
    kind: rewriteKindTypeIds(schema.kind, replacements),
  };
}

function rewriteKindTypeIds(
  kind: SchemaKind,
  replacements: ReadonlyArray<readonly [TypeId, TypeId]>,
): SchemaKind {
  switch (kind.kind) {
    case "primitive":
    case "dynamic":
      return kind;
    case "struct":
      return {
        kind: "struct",
        name: kind.name,
        fields: kind.fields.map((field) => ({
          name: field.name,
          typeRef: rewriteTypeRefIds(field.typeRef, replacements),
        })),
      };
    case "enum":
      return {
        kind: "enum",
        name: kind.name,
        variants: kind.variants.map((variant) => ({
          name: variant.name,
          index: variant.index,
          payload: rewritePayloadTypeIds(variant.payload, replacements),
        })),
      };
    case "tuple":
      return {
        kind: "tuple",
        elements: kind.elements.map((element) =>
          rewriteTypeRefIds(element, replacements),
        ),
      };
    case "list":
    case "set":
    case "option":
      return {
        kind: kind.kind,
        element: rewriteTypeRefIds(kind.element, replacements),
      };
    case "map":
      return {
        kind: "map",
        key: rewriteTypeRefIds(kind.key, replacements),
        value: rewriteTypeRefIds(kind.value, replacements),
      };
    case "array":
      return {
        kind: "array",
        element: rewriteTypeRefIds(kind.element, replacements),
        dimensions: [...kind.dimensions],
      };
    case "external":
      return kind;
  }
}

function rewritePayloadTypeIds(
  payload: VariantPayload,
  replacements: ReadonlyArray<readonly [TypeId, TypeId]>,
): VariantPayload {
  switch (payload.kind) {
    case "unit":
      return payload;
    case "newtype":
      return {
        kind: "newtype",
        typeRef: rewriteTypeRefIds(payload.typeRef, replacements),
      };
    case "tuple":
      return {
        kind: "tuple",
        elements: payload.elements.map((element) =>
          rewriteTypeRefIds(element, replacements),
        ),
      };
    case "struct":
      return {
        kind: "struct",
        fields: payload.fields.map((field) => ({
          name: field.name,
          typeRef: rewriteTypeRefIds(field.typeRef, replacements),
        })),
      };
  }
}

function rewriteTypeRefIds(
  typeRef: TypeRef,
  replacements: ReadonlyArray<readonly [TypeId, TypeId]>,
): TypeRef {
  switch (typeRef.kind) {
    case "concrete":
      return concreteTypeRef(
        replaceTypeId(typeRef.typeId, replacements),
        typeRef.args.map((arg) => rewriteTypeRefIds(arg, replacements)),
      );
    case "var":
      return typeRef;
  }
}

function replaceTypeId(
  typeId: TypeId,
  replacements: ReadonlyArray<readonly [TypeId, TypeId]>,
): TypeId {
  return replacements.find(([original]) => original === typeId)?.[1] ?? typeId;
}

function mustTypeId(typeId: TypeId | undefined): TypeId {
  assert.notEqual(typeId, undefined);
  return typeId as TypeId;
}
