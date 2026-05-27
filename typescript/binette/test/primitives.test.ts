import assert from "node:assert/strict";
import test from "node:test";

import {
  decodeBool,
  decodeBytes,
  decodeI32,
  decodeString,
  decodeU32,
  decodeU64,
  encodeBool,
  encodeBytes,
  encodeI32,
  encodeString,
  encodeU32,
  encodeU64,
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
