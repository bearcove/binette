import assert from "node:assert/strict";
import test from "node:test";

import {
  decodeBool,
  decodeBytes,
  decodeEnumVariant,
  decodeI32,
  decodeOption,
  decodeString,
  decodeTuple2,
  decodeU32,
  decodeU64,
  decodeVec,
  encodeBool,
  encodeBytes,
  encodeEnumVariant,
  encodeI32,
  encodeOption,
  encodeString,
  encodeTuple2,
  encodeU32,
  encodeU64,
  encodeVec,
} from "../src/index.js";

test("fixed-width primitive helpers use little-endian binette bytes", () => {
  assert.deepEqual([...encodeBool(true)], [1]);
  assert.equal(decodeBool(Uint8Array.of(0), 0).value, false);

  assert.deepEqual([...encodeU32(0x1234_5678)], [0x78, 0x56, 0x34, 0x12]);
  assert.deepEqual(decodeU32(Uint8Array.of(9, 0x78, 0x56, 0x34, 0x12), 1), {
    value: 0x1234_5678,
    next: 5,
  });

  assert.deepEqual([...encodeI32(-2)], [0xfe, 0xff, 0xff, 0xff]);
  assert.deepEqual(decodeI32(Uint8Array.of(0xfe, 0xff, 0xff, 0xff), 0), {
    value: -2,
    next: 4,
  });

  assert.deepEqual([...encodeU64(0x0102_0304_0506_0708n)], [
    0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01,
  ]);
  assert.deepEqual(decodeU64(encodeU64(0x0102_0304_0506_0708n), 0), {
    value: 0x0102_0304_0506_0708n,
    next: 8,
  });
});

test("length-prefixed primitive helpers round-trip bytes and strings", () => {
  assert.deepEqual([...encodeBytes(Uint8Array.of(4, 5, 6))], [
    3, 0, 0, 0, 4, 5, 6,
  ]);
  assert.deepEqual(decodeBytes(Uint8Array.of(0, 3, 0, 0, 0, 4, 5, 6), 1), {
    value: Uint8Array.of(4, 5, 6),
    next: 8,
  });

  const encoded = encodeString("éclair");
  assert.deepEqual(decodeString(encoded, 0), {
    value: "éclair",
    next: encoded.length,
  });
});

test("option helpers use compact option tags", () => {
  assert.deepEqual([...encodeOption(null, encodeU32)], [0]);
  assert.deepEqual([...encodeOption(42, encodeU32)], [1, 42, 0, 0, 0]);

  assert.deepEqual(decodeOption(Uint8Array.of(0), 0, decodeU32), {
    value: null,
    next: 1,
  });
  assert.deepEqual(decodeOption(Uint8Array.of(1, 42, 0, 0, 0), 0, decodeU32), {
    value: 42,
    next: 5,
  });
});

test("sequence tuple and enum helpers use compact aggregate bytes", () => {
  assert.deepEqual([...encodeVec([1, 2, 3], encodeU32)], [
    3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0,
  ]);
  assert.deepEqual(decodeVec(encodeVec([1, 2, 3], encodeU32), 0, decodeU32), {
    value: [1, 2, 3],
    next: 16,
  });

  const tuple = encodeTuple2(42, "hello", encodeU32, encodeString);
  assert.deepEqual(decodeTuple2(tuple, 0, decodeU32, decodeString), {
    value: [42, "hello"],
    next: tuple.length,
  });

  assert.deepEqual([...encodeEnumVariant(2)], [2, 0, 0, 0]);
  assert.deepEqual(decodeEnumVariant(Uint8Array.of(2, 0, 0, 0), 0), {
    value: 2,
    next: 4,
  });
});
