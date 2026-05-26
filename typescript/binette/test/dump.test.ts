import assert from "node:assert/strict";
import test from "node:test";

import {
  compatibilityReport,
  concreteTypeRef,
  decodeSchemaDump,
  decodeSchemaSnapshot,
  encodeSchemaDump,
  encodeSchemaSnapshot,
  primitiveTypeId,
  schemaDumpFromValue,
  schemaDumpToValue,
  schemaSnapshotFromValue,
  schemaSnapshotToValue,
  schemaTypeId,
  type Primitive,
  type Schema,
  type SchemaBundle,
  type SchemaDump,
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

function accountSchema(fields: Array<{ name: string; typeRef: TypeRef }>): Schema {
  return schema({ kind: "struct", name: "Account", fields });
}

// r[verify binette.bundle.dump]
// r[verify binette.compat.defaultability-metadata]
test("schema dumps round-trip metadata without affecting type ids", () => {
  const optionString = schema({ kind: "option", element: primitiveRef("string") });
  const account = accountSchema([
    { name: "id", typeRef: primitiveRef("u64") },
    { name: "name", typeRef: primitiveRef("string") },
    { name: "nickname", typeRef: concreteTypeRef(optionString.id) },
  ]);
  const rootTypeId = schemaTypeId(account);
  const dump: SchemaDump = {
    bundle: bundle(account, [account, optionString]),
    metadata: {
      declarations: [
        {
          typeId: account.id,
          sourceName: "Account",
          documentation: "account record",
          sourceLocation: "test/dump.test.ts",
          fields: [
            {
              name: "id",
              defaultability: { kind: "none" },
              documentation: null,
              sourceLocation: null,
            },
            {
              name: "name",
              defaultability: { kind: "opaque" },
              documentation: "display name",
              sourceLocation: null,
            },
            {
              name: "nickname",
              defaultability: {
                kind: "literal",
                value: { kind: "option", value: null },
              },
              documentation: null,
              sourceLocation: "Account.nickname",
            },
          ],
        },
      ],
    },
  };

  const decoded = decodeSchemaDump(encodeSchemaDump(dump));

  assert.deepEqual(decoded, dump);
  assert.deepEqual(schemaDumpFromValue(schemaDumpToValue(decoded)), decoded);
  assert.equal(schemaTypeId(decoded.bundle.schemas[0] as Schema), rootTypeId);
});

// r[verify binette.bundle.snapshot]
test("schema snapshots round-trip bundle roots", () => {
  const account = accountSchema([{ name: "id", typeRef: primitiveRef("u64") }]);
  const message = schema({
    kind: "struct",
    name: "Message",
    fields: [{ name: "body", typeRef: primitiveRef("string") }],
  });
  const snapshot = {
    dumps: [
      { bundle: bundle(account), metadata: { declarations: [] } },
      { bundle: bundle(message), metadata: { declarations: [] } },
    ],
  };

  const decoded = decodeSchemaSnapshot(encodeSchemaSnapshot(snapshot));

  assert.deepEqual(decoded, snapshot);
  assert.deepEqual(schemaSnapshotFromValue(schemaSnapshotToValue(decoded)), decoded);
  assert.deepEqual(decoded.dumps.map((dump) => dump.bundle.root), [
    concreteTypeRef(account.id),
    concreteTypeRef(message.id),
  ]);
});

// r[verify binette.compat.report]
test("compatibility report classifies schema snapshot direction", () => {
  const old = bundle(accountSchema([{ name: "id", typeRef: primitiveRef("u64") }]));
  const addedRequired = bundle(
    accountSchema([
      { name: "id", typeRef: primitiveRef("u64") },
      { name: "name", typeRef: primitiveRef("string") },
    ]),
  );
  const reordered = bundle(
    accountSchema([
      { name: "name", typeRef: primitiveRef("string") },
      { name: "id", typeRef: primitiveRef("u64") },
    ]),
  );
  const changed = bundle(accountSchema([{ name: "id", typeRef: primitiveRef("string") }]));

  const forward = compatibilityReport(old, addedRequired);
  assert.equal(forward.status, "forward");
  assert.equal(forward.failures.length, 1);
  assert.equal(forward.failures[0]?.direction, "backward");
  assert.equal(forward.failures[0]?.path, "$");
  assert.equal(forward.failures[0]?.reason.kind, "missingReaderField");

  assert.equal(compatibilityReport(addedRequired, old).status, "backward");
  assert.deepEqual(compatibilityReport(addedRequired, reordered), {
    status: "bidirectional",
    failures: [],
  });

  const incompatible = compatibilityReport(old, changed);
  assert.equal(incompatible.status, "incompatible");
  assert.equal(incompatible.failures.length, 2);
  assert.equal(incompatible.failures[0]?.path, "$.id");
  assert.equal(incompatible.failures[1]?.path, "$.id");
  assert.equal(incompatible.failures[0]?.reason.kind, "typeMismatch");
  assert.equal(incompatible.failures[1]?.reason.kind, "typeMismatch");
});
