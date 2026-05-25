import assert from "node:assert/strict";
import test from "node:test";

import {
  BinetteError,
  decodeDynamicValue,
  decodeSelfDescribed,
  encodeDynamicValue,
  encodeSelfDescribed,
  type Value,
} from "../src/index.js";

function bytes(...values: number[]): Uint8Array {
  return Uint8Array.from(values);
}

function roundTrip(value: Value): Value {
  return decodeSelfDescribed(encodeSelfDescribed(value));
}

// r[verify binette.forms]
// r[verify binette.mode.self-describing]
// r[verify binette.tags]
// r[verify binette.tags.scalar-payload]
// r[verify binette.endianness]
// r[verify binette.length.canonical-width]
// r[verify binette.length.u32]
// r[verify binette.scalar.bool]
// r[verify binette.scalar.bytes]
// r[verify binette.scalar.char]
// r[verify binette.scalar.float]
// r[verify binette.scalar.signed]
// r[verify binette.scalar.string]
// r[verify binette.scalar.unit]
// r[verify binette.scalar.unsigned]
// r[verify binette.value-kind.preserved]
// r[verify binette.value-model]
test("self-describing scalars use fixed tags and payload bytes", () => {
  assert.deepEqual(
    encodeSelfDescribed({ kind: "string", value: "binette" }),
    bytes(0x0f, 7, 0, 0, 0, 0x62, 0x69, 0x6e, 0x65, 0x74, 0x74, 0x65),
  );

  const cases: Array<{ value: Value; encoded: Uint8Array }> = [
    { value: { kind: "unit" }, encoded: bytes(0x00) },
    { value: { kind: "bool", value: true }, encoded: bytes(0x01, 0x01) },
    { value: { kind: "u8", value: 7 }, encoded: bytes(0x02, 0x07) },
    { value: { kind: "u16", value: 0x1234 }, encoded: bytes(0x03, 0x34, 0x12) },
    {
      value: { kind: "u32", value: 0x12345678 },
      encoded: bytes(0x04, 0x78, 0x56, 0x34, 0x12),
    },
    {
      value: { kind: "u64", value: 0x0102030405060708n },
      encoded: bytes(0x05, 8, 7, 6, 5, 4, 3, 2, 1),
    },
    {
      value: { kind: "u128", value: 0x0102030405060708090a0b0c0d0e0f10n },
      encoded: bytes(
        0x06,
        0x10,
        0x0f,
        0x0e,
        0x0d,
        0x0c,
        0x0b,
        0x0a,
        0x09,
        0x08,
        0x07,
        0x06,
        0x05,
        0x04,
        0x03,
        0x02,
        0x01,
      ),
    },
    { value: { kind: "i8", value: -2 }, encoded: bytes(0x07, 0xfe) },
    { value: { kind: "i16", value: -2 }, encoded: bytes(0x08, 0xfe, 0xff) },
    {
      value: { kind: "i32", value: -2 },
      encoded: bytes(0x09, 0xfe, 0xff, 0xff, 0xff),
    },
    {
      value: { kind: "i64", value: -2n },
      encoded: bytes(0x0a, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff),
    },
    {
      value: { kind: "i128", value: -2n },
      encoded: bytes(
        0x0b,
        0xfe,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
        0xff,
      ),
    },
    {
      value: { kind: "f32", value: 1, bits: 0x3f800000 },
      encoded: bytes(0x0c, 0, 0, 0x80, 0x3f),
    },
    {
      value: { kind: "f64", value: 1, bits: 0x3ff0000000000000n },
      encoded: bytes(0x0d, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f),
    },
    {
      value: { kind: "char", value: "\u{00e9}" },
      encoded: bytes(0x0e, 0xe9, 0, 0, 0),
    },
    {
      value: { kind: "bytes", value: bytes(1, 2, 3) },
      encoded: bytes(0x10, 3, 0, 0, 0, 1, 2, 3),
    },
    {
      value: { kind: "payload", value: bytes(4, 5) },
      encoded: bytes(0x11, 2, 0, 0, 0, 4, 5),
    },
  ];

  for (const { value, encoded } of cases) {
    assert.deepEqual(encodeSelfDescribed(value), encoded);
    assert.deepEqual(decodeSelfDescribed(encoded), value);
  }

  assert.throws(() => decodeSelfDescribed(bytes(0x01, 0x02)), BinetteError);
});

// r[verify binette.aggregate.struct.self-describing]
// r[verify binette.aggregate.enum.self-describing]
// r[verify binette.tags.aggregate-payload]
test("self-describing structs and enums round-trip names and payloads", () => {
  const value: Value = {
    kind: "struct",
    fields: [
      { name: "id", value: { kind: "u64", value: 42n } },
      {
        name: "event",
        value: {
          kind: "enum",
          variant: "renamed",
          payload: { kind: "string", value: "binette" },
        },
      },
    ],
  };

  const encoded = encodeSelfDescribed(value);
  assert.equal(encoded[0], 0x17);
  assert.deepEqual(decodeSelfDescribed(encoded), value);
});

// r[verify binette.aggregate.array]
// r[verify binette.aggregate.tuple]
test("self-describing arrays and tuples carry shape and arity", () => {
  const value: Value = {
    kind: "tuple",
    elements: [
      {
        kind: "array",
        dimensions: [2n, 1n],
        elements: [
          { kind: "u8", value: 10 },
          { kind: "u8", value: 20 },
        ],
      },
      { kind: "bool", value: true },
    ],
  };

  const encoded = encodeSelfDescribed(value);
  assert.equal(encoded[0], 0x16);
  assert.deepEqual(decodeSelfDescribed(encoded), value);
  assert.throws(() => encodeSelfDescribed({ kind: "tuple", elements: [] }), BinetteError);
});

// r[verify binette.aggregate.option]
// r[verify binette.aggregate.dynamic-value]
// r[verify binette.aggregate.list]
test("options and dynamic values encode nested self-described values", () => {
  const inner: Value = { kind: "string", value: "dynamic" };
  const compactDynamic = encodeDynamicValue(inner);
  const selfDescribingDynamic = encodeSelfDescribed({ kind: "dynamic", value: inner });

  assert.equal(compactDynamic[0], 0x0f);
  assert.equal(selfDescribingDynamic[0], 0x1b);
  assert.deepEqual(selfDescribingDynamic.slice(1), compactDynamic);
  assert.deepEqual(decodeDynamicValue(compactDynamic), inner);

  assert.deepEqual(roundTrip({ kind: "option", value: null }), {
    kind: "option",
    value: null,
  });
  assert.deepEqual(
    roundTrip({ kind: "option", value: { kind: "u32", value: 9 } }),
    { kind: "option", value: { kind: "u32", value: 9 } },
  );
  assert.deepEqual(
    roundTrip({
      kind: "list",
      elements: [
        { kind: "u8", value: 1 },
        { kind: "u8", value: 2 },
      ],
    }),
    {
      kind: "list",
      elements: [
        { kind: "u8", value: 1 },
        { kind: "u8", value: 2 },
      ],
    },
  );
});

// r[verify binette.aggregate.set]
// r[verify binette.aggregate.map]
// r[verify binette.aggregate.set-map.canonical]
test("self-describing sets and maps sort by complete encoded key bytes", () => {
  const set: Value = {
    kind: "set",
    elements: [
      { kind: "u16", value: 1 },
      { kind: "u16", value: 256 },
    ],
  };
  const setBytes = encodeSelfDescribed(set);
  assert.deepEqual(setBytes, bytes(0x13, 2, 0, 0, 0, 0x03, 0, 1, 0x03, 1, 0));
  assert.deepEqual(decodeSelfDescribed(setBytes), {
    kind: "set",
    elements: [
      { kind: "u16", value: 256 },
      { kind: "u16", value: 1 },
    ],
  });

  const map: Value = {
    kind: "map",
    entries: [
      { key: { kind: "u16", value: 1 }, value: { kind: "string", value: "one" } },
      {
        key: { kind: "u16", value: 256 },
        value: { kind: "string", value: "two-five-six" },
      },
    ],
  };
  const decoded = decodeSelfDescribed(encodeSelfDescribed(map));
  assert.deepEqual(decoded, {
    kind: "map",
    entries: [
      {
        key: { kind: "u16", value: 256 },
        value: { kind: "string", value: "two-five-six" },
      },
      { key: { kind: "u16", value: 1 }, value: { kind: "string", value: "one" } },
    ],
  });
});

// r[verify binette.aggregate.set-map.decode-policy]
test("self-describing decode rejects noncanonical set order", () => {
  assert.throws(
    () => decodeSelfDescribed(bytes(0x13, 2, 0, 0, 0, 0x03, 1, 0, 0x03, 0, 1)),
    BinetteError,
  );
});

// r[verify binette.aggregate.set-map.float-keys]
test("self-describing set and map keys reject NaN payloads", () => {
  assert.throws(
    () => encodeSelfDescribed({ kind: "set", elements: [{ kind: "f32", value: NaN }] }),
    BinetteError,
  );
  assert.throws(
    () => decodeSelfDescribed(bytes(0x13, 1, 0, 0, 0, 0x0c, 0, 0, 0xc0, 0x7f)),
    BinetteError,
  );
});

// r[verify binette.tags.extension]
// r[verify binette.tags.forward-contract]
// r[verify binette.value-model.extension-form]
test("self-describing extension tags preserve opaque payloads", () => {
  const value: Value = {
    kind: "extension",
    tag: 0x80,
    id: 7,
    payload: bytes(1, 2, 3),
  };
  const encoded = encodeSelfDescribed(value);

  assert.deepEqual(encoded, bytes(0x80, 7, 0, 0, 0, 3, 0, 0, 0, 1, 2, 3));
  assert.deepEqual(decodeSelfDescribed(encoded), value);
});

// r[verify binette.value-model.external-form]
test("external attachments have no core self-describing tag", () => {
  assert.throws(
    () => encodeSelfDescribed({ kind: "externalAttachment" }),
    BinetteError,
  );
});
