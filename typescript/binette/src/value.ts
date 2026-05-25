const TAG_UNIT = 0x00;
const TAG_BOOL = 0x01;
const TAG_U8 = 0x02;
const TAG_U16 = 0x03;
const TAG_U32 = 0x04;
const TAG_U64 = 0x05;
const TAG_U128 = 0x06;
const TAG_I8 = 0x07;
const TAG_I16 = 0x08;
const TAG_I32 = 0x09;
const TAG_I64 = 0x0a;
const TAG_I128 = 0x0b;
const TAG_F32 = 0x0c;
const TAG_F64 = 0x0d;
const TAG_CHAR = 0x0e;
const TAG_STRING = 0x0f;
const TAG_BYTES = 0x10;
const TAG_PAYLOAD = 0x11;
const TAG_LIST = 0x12;
const TAG_SET = 0x13;
const TAG_MAP = 0x14;
const TAG_ARRAY = 0x15;
const TAG_TUPLE = 0x16;
const TAG_STRUCT = 0x17;
const TAG_ENUM = 0x18;
const TAG_OPTION_NONE = 0x19;
const TAG_OPTION_SOME = 0x1a;
const TAG_DYNAMIC = 0x1b;
const FIRST_EXTENSION_TAG = 0x80;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder("utf-8", { fatal: true });

// r[impl binette.value-model]
// r[impl binette.value-kind.preserved]
export type Value =
  | { kind: "unit" }
  | { kind: "bool"; value: boolean }
  | { kind: "u8"; value: number }
  | { kind: "u16"; value: number }
  | { kind: "u32"; value: number }
  | { kind: "u64"; value: bigint }
  | { kind: "u128"; value: bigint }
  | { kind: "i8"; value: number }
  | { kind: "i16"; value: number }
  | { kind: "i32"; value: number }
  | { kind: "i64"; value: bigint }
  | { kind: "i128"; value: bigint }
  | { kind: "f32"; value: number; bits?: number }
  | { kind: "f64"; value: number; bits?: bigint }
  | { kind: "char"; value: string }
  | { kind: "string"; value: string }
  | { kind: "bytes"; value: Uint8Array }
  | { kind: "payload"; value: Uint8Array }
  | { kind: "list"; elements: Value[] }
  | { kind: "set"; elements: Value[] }
  | { kind: "map"; entries: Array<{ key: Value; value: Value }> }
  | { kind: "array"; dimensions: bigint[]; elements: Value[] }
  | { kind: "tuple"; elements: Value[] }
  | { kind: "struct"; fields: Array<{ name: string; value: Value }> }
  | { kind: "enum"; variant: string; payload: Value }
  | { kind: "option"; value: Value | null }
  | { kind: "dynamic"; value: Value }
  // r[impl binette.value-model.external-form]
  | { kind: "externalAttachment" }
  // r[impl binette.value-model.extension-form]
  | { kind: "extension"; tag: number; id: number; payload: Uint8Array };

export class BinetteError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "BinetteError";
  }
}

// r[impl binette.forms]
// r[impl binette.mode.self-describing]
export function encodeSelfDescribed(value: Value): Uint8Array {
  const out = new ByteWriter();
  writeSelfDescribed(out, value);
  return out.finish();
}

// r[impl binette.aggregate.dynamic-value]
export function encodeDynamicValue(value: Value): Uint8Array {
  return encodeSelfDescribed(value);
}

// r[impl binette.forms]
// r[impl binette.mode.self-describing]
export function decodeSelfDescribed(bytes: Uint8Array): Value {
  const reader = new ByteReader(bytes);
  const value = readSelfDescribed(reader);
  if (!reader.isEmpty()) {
    throw new BinetteError(
      `trailing bytes after self-described value at byte ${reader.position}: ${reader.remaining} bytes remain`,
    );
  }
  return value;
}

// r[impl binette.aggregate.dynamic-value]
export function decodeDynamicValue(bytes: Uint8Array): Value {
  return decodeSelfDescribed(bytes);
}

// r[impl binette.tags]
// r[impl binette.tags.scalar-payload]
// r[impl binette.tags.aggregate-payload]
// r[impl binette.endianness]
// r[impl binette.length.canonical-width]
// r[impl binette.length.u32]
// r[impl binette.scalar.bool]
// r[impl binette.scalar.bytes]
// r[impl binette.scalar.char]
// r[impl binette.scalar.float]
// r[impl binette.scalar.signed]
// r[impl binette.scalar.string]
// r[impl binette.scalar.unit]
// r[impl binette.scalar.unsigned]
// r[impl binette.aggregate.list]
// r[impl binette.aggregate.option]
function writeSelfDescribed(out: ByteWriter, value: Value): void {
  switch (value.kind) {
    case "unit":
      out.u8(TAG_UNIT);
      break;
    case "bool":
      out.u8(TAG_BOOL);
      out.u8(value.value ? 1 : 0);
      break;
    case "u8":
      out.u8(TAG_U8);
      out.u8(value.value);
      break;
    case "u16":
      out.u8(TAG_U16);
      out.u16(value.value);
      break;
    case "u32":
      out.u8(TAG_U32);
      out.u32(value.value);
      break;
    case "u64":
      out.u8(TAG_U64);
      out.u64(value.value);
      break;
    case "u128":
      out.u8(TAG_U128);
      out.u128(value.value);
      break;
    case "i8":
      out.u8(TAG_I8);
      out.i8(value.value);
      break;
    case "i16":
      out.u8(TAG_I16);
      out.i16(value.value);
      break;
    case "i32":
      out.u8(TAG_I32);
      out.i32(value.value);
      break;
    case "i64":
      out.u8(TAG_I64);
      out.i64(value.value);
      break;
    case "i128":
      out.u8(TAG_I128);
      out.i128(value.value);
      break;
    case "f32":
      out.u8(TAG_F32);
      out.f32(value);
      break;
    case "f64":
      out.u8(TAG_F64);
      out.f64(value);
      break;
    case "char":
      out.u8(TAG_CHAR);
      out.char(value.value);
      break;
    case "string":
      out.u8(TAG_STRING);
      out.bytes(utf8(value.value));
      break;
    case "bytes":
      out.u8(TAG_BYTES);
      out.bytes(value.value);
      break;
    case "payload":
      out.u8(TAG_PAYLOAD);
      out.bytes(value.value);
      break;
    case "list":
      out.u8(TAG_LIST);
      out.u32(value.elements.length);
      for (const element of value.elements) {
        writeSelfDescribed(out, element);
      }
      break;
    case "set":
      writeSet(out, value.elements);
      break;
    case "map":
      writeMap(out, value.entries);
      break;
    case "array":
      writeArray(out, value);
      break;
    case "tuple":
      writeTuple(out, value.elements);
      break;
    case "struct":
      writeStruct(out, value.fields);
      break;
    case "enum":
      writeEnum(out, value);
      break;
    case "option":
      if (value.value === null) {
        out.u8(TAG_OPTION_NONE);
      } else {
        out.u8(TAG_OPTION_SOME);
        writeSelfDescribed(out, value.value);
      }
      break;
    case "dynamic":
      out.u8(TAG_DYNAMIC);
      writeSelfDescribed(out, value.value);
      break;
    case "externalAttachment":
      throw new BinetteError(
        "external attachment values do not have a core self-describing tag",
      );
    case "extension":
      writeExtension(out, value);
      break;
  }
}

// r[impl binette.aggregate.set]
// r[impl binette.aggregate.set-map.canonical]
// r[impl binette.aggregate.set-map.float-keys]
function writeSet(out: ByteWriter, elements: Value[]): void {
  const encoded = elements.map((element) => {
    rejectNanKey(element, "set");
    return encodeSelfDescribed(element);
  });
  encoded.sort(compareBytes);
  rejectDuplicateKeys("set", encoded);

  out.u8(TAG_SET);
  out.u32(encoded.length);
  for (const element of encoded) {
    out.raw(element);
  }
}

// r[impl binette.aggregate.map]
// r[impl binette.aggregate.set-map.canonical]
// r[impl binette.aggregate.set-map.float-keys]
function writeMap(
  out: ByteWriter,
  entries: Array<{ key: Value; value: Value }>,
): void {
  const encoded = entries.map((entry) => {
    rejectNanKey(entry.key, "map");
    return {
      key: encodeSelfDescribed(entry.key),
      value: encodeSelfDescribed(entry.value),
    };
  });
  encoded.sort((left, right) => compareBytes(left.key, right.key));
  rejectDuplicateKeys(
    "map",
    encoded.map((entry) => entry.key),
  );

  out.u8(TAG_MAP);
  out.u32(encoded.length);
  for (const entry of encoded) {
    out.raw(entry.key);
    out.raw(entry.value);
  }
}

// r[impl binette.aggregate.array]
function writeArray(
  out: ByteWriter,
  value: Extract<Value, { kind: "array" }>,
): void {
  const count = arrayElementCount(value.dimensions);
  if (count !== value.elements.length) {
    throw new BinetteError(
      `array dimensions require ${count} elements, value has ${value.elements.length}`,
    );
  }

  out.u8(TAG_ARRAY);
  out.u32(value.dimensions.length);
  for (const dimension of value.dimensions) {
    out.u64(dimension);
  }
  for (const element of value.elements) {
    writeSelfDescribed(out, element);
  }
}

// r[impl binette.aggregate.tuple]
function writeTuple(out: ByteWriter, elements: Value[]): void {
  if (elements.length === 0) {
    throw new BinetteError("tuple must contain at least one element");
  }
  out.u8(TAG_TUPLE);
  out.u32(elements.length);
  for (const element of elements) {
    writeSelfDescribed(out, element);
  }
}

// r[impl binette.aggregate.struct.self-describing]
function writeStruct(
  out: ByteWriter,
  fields: Array<{ name: string; value: Value }>,
): void {
  out.u8(TAG_STRUCT);
  out.u32(fields.length);
  for (const field of fields) {
    out.bytes(utf8(field.name));
    writeSelfDescribed(out, field.value);
  }
}

// r[impl binette.aggregate.enum.self-describing]
function writeEnum(out: ByteWriter, value: Extract<Value, { kind: "enum" }>): void {
  out.u8(TAG_ENUM);
  out.bytes(utf8(value.variant));
  writeSelfDescribed(out, value.payload);
}

// r[impl binette.tags.extension]
// r[impl binette.tags.forward-contract]
function writeExtension(
  out: ByteWriter,
  value: Extract<Value, { kind: "extension" }>,
): void {
  if (value.tag < FIRST_EXTENSION_TAG || value.tag > 0xff) {
    throw new BinetteError(
      `extension tag must be in 0x80..0xFF, got ${hex(value.tag)}`,
    );
  }
  out.u8(value.tag);
  out.u32(value.id);
  out.bytes(value.payload);
}

function readSelfDescribed(reader: ByteReader): Value {
  const position = reader.position;
  const tag = reader.u8();
  switch (tag) {
    case TAG_UNIT:
      return { kind: "unit" };
    case TAG_BOOL:
      return { kind: "bool", value: reader.bool() };
    case TAG_U8:
      return { kind: "u8", value: reader.u8() };
    case TAG_U16:
      return { kind: "u16", value: reader.u16() };
    case TAG_U32:
      return { kind: "u32", value: reader.u32() };
    case TAG_U64:
      return { kind: "u64", value: reader.u64() };
    case TAG_U128:
      return { kind: "u128", value: reader.u128() };
    case TAG_I8:
      return { kind: "i8", value: reader.i8() };
    case TAG_I16:
      return { kind: "i16", value: reader.i16() };
    case TAG_I32:
      return { kind: "i32", value: reader.i32() };
    case TAG_I64:
      return { kind: "i64", value: reader.i64() };
    case TAG_I128:
      return { kind: "i128", value: reader.i128() };
    case TAG_F32:
      return reader.f32();
    case TAG_F64:
      return reader.f64();
    case TAG_CHAR:
      return { kind: "char", value: reader.char() };
    case TAG_STRING:
      return { kind: "string", value: reader.string() };
    case TAG_BYTES:
      return { kind: "bytes", value: reader.bytes() };
    case TAG_PAYLOAD:
      return { kind: "payload", value: reader.bytes() };
    case TAG_LIST:
      return readList(reader);
    case TAG_SET:
      return readSet(reader);
    case TAG_MAP:
      return readMap(reader);
    case TAG_ARRAY:
      return readArray(reader);
    case TAG_TUPLE:
      return readTuple(reader);
    case TAG_STRUCT:
      return readStruct(reader);
    case TAG_ENUM:
      return readEnum(reader);
    case TAG_OPTION_NONE:
      return { kind: "option", value: null };
    case TAG_OPTION_SOME:
      return { kind: "option", value: readSelfDescribed(reader) };
    case TAG_DYNAMIC:
      return { kind: "dynamic", value: readSelfDescribed(reader) };
    default:
      if (tag >= 0x1c && tag <= 0x7f) {
        throw new BinetteError(
          `reserved self-describing tag ${hex(tag)} at byte ${position}`,
        );
      }
      return readExtension(reader, tag);
  }
}

function readList(reader: ByteReader): Value {
  const count = reader.u32();
  const elements: Value[] = [];
  for (let index = 0; index < count; index += 1) {
    elements.push(readSelfDescribed(reader));
  }
  return { kind: "list", elements };
}

function readSet(reader: ByteReader): Value {
  const count = reader.u32();
  const elements: Value[] = [];
  let previous: Uint8Array | null = null;
  for (let index = 0; index < count; index += 1) {
    const start = reader.position;
    const element = readSelfDescribed(reader);
    rejectNanKey(element, "set");
    previous = validateCanonicalBytes(reader.consumedFrom(start), previous, "set");
    elements.push(element);
  }
  return { kind: "set", elements };
}

function readMap(reader: ByteReader): Value {
  const count = reader.u32();
  const entries: Array<{ key: Value; value: Value }> = [];
  let previous: Uint8Array | null = null;
  for (let index = 0; index < count; index += 1) {
    const keyStart = reader.position;
    const key = readSelfDescribed(reader);
    rejectNanKey(key, "map");
    previous = validateCanonicalBytes(reader.consumedFrom(keyStart), previous, "map");
    const value = readSelfDescribed(reader);
    entries.push({ key, value });
  }
  return { kind: "map", entries };
}

function readArray(reader: ByteReader): Value {
  const rank = reader.u32();
  if (rank === 0) {
    throw new BinetteError("array rank must be at least one");
  }

  const dimensions: bigint[] = [];
  for (let index = 0; index < rank; index += 1) {
    dimensions.push(reader.u64());
  }

  const count = arrayElementCount(dimensions);
  const elements: Value[] = [];
  for (let index = 0; index < count; index += 1) {
    elements.push(readSelfDescribed(reader));
  }
  return { kind: "array", dimensions, elements };
}

function readTuple(reader: ByteReader): Value {
  const count = reader.u32();
  if (count === 0) {
    throw new BinetteError("tuple must contain at least one element");
  }
  const elements: Value[] = [];
  for (let index = 0; index < count; index += 1) {
    elements.push(readSelfDescribed(reader));
  }
  return { kind: "tuple", elements };
}

function readStruct(reader: ByteReader): Value {
  const count = reader.u32();
  const fields: Array<{ name: string; value: Value }> = [];
  for (let index = 0; index < count; index += 1) {
    fields.push({
      name: reader.string(),
      value: readSelfDescribed(reader),
    });
  }
  return { kind: "struct", fields };
}

function readEnum(reader: ByteReader): Value {
  return {
    kind: "enum",
    variant: reader.string(),
    payload: readSelfDescribed(reader),
  };
}

function readExtension(reader: ByteReader, tag: number): Value {
  return {
    kind: "extension",
    tag,
    id: reader.u32(),
    payload: reader.bytes(),
  };
}

function arrayElementCount(dimensions: bigint[]): number {
  if (dimensions.length === 0) {
    throw new BinetteError("array rank must be at least one");
  }
  let count = 1n;
  for (const dimension of dimensions) {
    assertUnsigned("array dimension", dimension, 64);
    count *= dimension;
    if (count > BigInt(Number.MAX_SAFE_INTEGER)) {
      throw new BinetteError("array element count overflows safe integer range");
    }
  }
  return Number(count);
}

function rejectNanKey(value: Value, aggregate: "set" | "map"): void {
  switch (value.kind) {
    case "f32":
    case "f64":
      if (Number.isNaN(value.value)) {
        throw new BinetteError(`NaN is not a valid ${aggregate} key payload`);
      }
      break;
    case "list":
    case "set":
    case "tuple":
      for (const element of value.elements) {
        rejectNanKey(element, aggregate);
      }
      break;
    case "map":
      for (const entry of value.entries) {
        rejectNanKey(entry.key, aggregate);
        rejectNanKey(entry.value, aggregate);
      }
      break;
    case "array":
      for (const element of value.elements) {
        rejectNanKey(element, aggregate);
      }
      break;
    case "struct":
      for (const field of value.fields) {
        rejectNanKey(field.value, aggregate);
      }
      break;
    case "enum":
      rejectNanKey(value.payload, aggregate);
      break;
    case "option":
      if (value.value !== null) {
        rejectNanKey(value.value, aggregate);
      }
      break;
    case "dynamic":
      rejectNanKey(value.value, aggregate);
      break;
  }
}

function validateCanonicalBytes(
  current: Uint8Array,
  previous: Uint8Array | null,
  aggregate: "set" | "map",
): Uint8Array {
  // r[impl binette.aggregate.set-map.decode-policy]
  if (previous !== null) {
    const order = compareBytes(previous, current);
    if (order === 0) {
      throw new BinetteError(`${aggregate} contains duplicate canonical key bytes`);
    }
    if (order > 0) {
      throw new BinetteError(`${aggregate} entries are not in canonical byte order`);
    }
  }
  return current.slice();
}

function rejectDuplicateKeys(aggregate: "set" | "map", keys: Uint8Array[]): void {
  let previous: Uint8Array | null = null;
  for (const key of keys) {
    if (previous !== null && compareBytes(previous, key) === 0) {
      throw new BinetteError(`${aggregate} contains duplicate canonical key bytes`);
    }
    previous = key;
  }
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const shared = Math.min(left.length, right.length);
  for (let index = 0; index < shared; index += 1) {
    const leftByte = left[index];
    const rightByte = right[index];
    if (leftByte === undefined || rightByte === undefined) {
      throw new BinetteError("byte comparison index out of bounds");
    }
    if (leftByte !== rightByte) {
      return leftByte - rightByte;
    }
  }
  return left.length - right.length;
}

function utf8(value: string): Uint8Array {
  return textEncoder.encode(value);
}

function stringFromUtf8(bytes: Uint8Array): string {
  try {
    return textDecoder.decode(bytes);
  } catch (error) {
    throw new BinetteError(`invalid UTF-8 string: ${String(error)}`);
  }
}

function hex(value: number): string {
  return `0x${value.toString(16).padStart(2, "0")}`;
}

class ByteWriter {
  private readonly chunks: number[] = [];

  finish(): Uint8Array {
    return Uint8Array.from(this.chunks);
  }

  raw(bytes: Uint8Array): void {
    for (const byte of bytes) {
      this.u8(byte);
    }
  }

  bytes(bytes: Uint8Array): void {
    this.u32(bytes.length);
    this.raw(bytes);
  }

  u8(value: number): void {
    assertUnsigned("u8", value, 8);
    this.chunks.push(value);
  }

  u16(value: number): void {
    assertUnsigned("u16", value, 16);
    this.fixed(2, (view) => view.setUint16(0, value, true));
  }

  u32(value: number): void {
    assertUnsigned("u32", value, 32);
    this.fixed(4, (view) => view.setUint32(0, value, true));
  }

  u64(value: bigint): void {
    this.bigUint(value, 8);
  }

  u128(value: bigint): void {
    this.bigUint(value, 16);
  }

  i8(value: number): void {
    assertSigned("i8", value, 8);
    this.fixed(1, (view) => view.setInt8(0, value));
  }

  i16(value: number): void {
    assertSigned("i16", value, 16);
    this.fixed(2, (view) => view.setInt16(0, value, true));
  }

  i32(value: number): void {
    assertSigned("i32", value, 32);
    this.fixed(4, (view) => view.setInt32(0, value, true));
  }

  i64(value: bigint): void {
    assertSignedBig("i64", value, 64);
    this.bigUint(BigInt.asUintN(64, value), 8);
  }

  i128(value: bigint): void {
    assertSignedBig("i128", value, 128);
    this.bigUint(BigInt.asUintN(128, value), 16);
  }

  f32(value: Extract<Value, { kind: "f32" }>): void {
    if (value.bits !== undefined) {
      assertUnsigned("f32 bits", value.bits, 32);
      this.u32(value.bits);
    } else {
      this.fixed(4, (view) => view.setFloat32(0, value.value, true));
    }
  }

  f64(value: Extract<Value, { kind: "f64" }>): void {
    if (value.bits !== undefined) {
      this.bigUint(value.bits, 8);
    } else {
      this.fixed(8, (view) => view.setFloat64(0, value.value, true));
    }
  }

  char(value: string): void {
    const codePoints = Array.from(value);
    if (codePoints.length !== 1) {
      throw new BinetteError("char must contain exactly one Unicode scalar value");
    }
    const codePoint = codePoints[0]?.codePointAt(0);
    if (codePoint === undefined || codePoint > 0x10ffff) {
      throw new BinetteError("invalid Unicode scalar value");
    }
    this.u32(codePoint);
  }

  private fixed(length: number, write: (view: DataView) => void): void {
    const bytes = new Uint8Array(length);
    const view = new DataView(bytes.buffer);
    write(view);
    this.raw(bytes);
  }

  private bigUint(value: bigint, byteCount: number): void {
    assertUnsigned(`u${byteCount * 8}`, value, byteCount * 8);
    for (let index = 0; index < byteCount; index += 1) {
      this.chunks.push(Number((value >> BigInt(index * 8)) & 0xffn));
    }
  }
}

class ByteReader {
  private offset = 0;

  constructor(private readonly bytes_: Uint8Array) {}

  get position(): number {
    return this.offset;
  }

  get remaining(): number {
    return this.bytes_.length - this.offset;
  }

  isEmpty(): boolean {
    return this.remaining === 0;
  }

  consumedFrom(start: number): Uint8Array {
    return this.bytes_.slice(start, this.offset);
  }

  u8(): number {
    this.require(1);
    const value = this.bytes_[this.offset];
    if (value === undefined) {
      throw new BinetteError("read past end of input");
    }
    this.offset += 1;
    return value;
  }

  bool(): boolean {
    const position = this.position;
    const value = this.u8();
    if (value === 0) {
      return false;
    }
    if (value === 1) {
      return true;
    }
    throw new BinetteError(`invalid bool byte ${value} at byte ${position}`);
  }

  u16(): number {
    const value = this.view(2).getUint16(0, true);
    this.offset += 2;
    return value;
  }

  u32(): number {
    const value = this.view(4).getUint32(0, true);
    this.offset += 4;
    return value;
  }

  u64(): bigint {
    return this.bigUint(8);
  }

  u128(): bigint {
    return this.bigUint(16);
  }

  i8(): number {
    const value = this.view(1).getInt8(0);
    this.offset += 1;
    return value;
  }

  i16(): number {
    const value = this.view(2).getInt16(0, true);
    this.offset += 2;
    return value;
  }

  i32(): number {
    const value = this.view(4).getInt32(0, true);
    this.offset += 4;
    return value;
  }

  i64(): bigint {
    return BigInt.asIntN(64, this.bigUint(8));
  }

  i128(): bigint {
    return BigInt.asIntN(128, this.bigUint(16));
  }

  f32(): Extract<Value, { kind: "f32" }> {
    const bits = this.u32();
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setUint32(0, bits, true);
    return {
      kind: "f32",
      value: new DataView(bytes.buffer).getFloat32(0, true),
      bits,
    };
  }

  f64(): Extract<Value, { kind: "f64" }> {
    const bits = this.u64();
    const bytes = new Uint8Array(8);
    writeBigUint(bytes, bits);
    return {
      kind: "f64",
      value: new DataView(bytes.buffer).getFloat64(0, true),
      bits,
    };
  }

  char(): string {
    const position = this.position;
    const value = this.u32();
    if (value >= 0xd800 && value <= 0xdfff) {
      throw new BinetteError(`invalid Unicode scalar value ${value} at byte ${position}`);
    }
    try {
      return String.fromCodePoint(value);
    } catch {
      throw new BinetteError(`invalid Unicode scalar value ${value} at byte ${position}`);
    }
  }

  string(): string {
    return stringFromUtf8(this.bytes());
  }

  bytes(): Uint8Array {
    const length = this.u32();
    return this.raw(length);
  }

  private raw(length: number): Uint8Array {
    this.require(length);
    const value = this.bytes_.slice(this.offset, this.offset + length);
    this.offset += length;
    return value;
  }

  private view(length: number): DataView {
    this.require(length);
    return new DataView(
      this.bytes_.buffer,
      this.bytes_.byteOffset + this.offset,
      length,
    );
  }

  private bigUint(byteCount: number): bigint {
    this.require(byteCount);
    let value = 0n;
    for (let index = 0; index < byteCount; index += 1) {
      const byte = this.bytes_[this.offset + index];
      if (byte === undefined) {
        throw new BinetteError("read past end of input");
      }
      value |= BigInt(byte) << BigInt(index * 8);
    }
    this.offset += byteCount;
    return value;
  }

  private require(length: number): void {
    if (this.remaining < length) {
      throw new BinetteError(
        `input ended at byte ${this.offset}; ${length} bytes required`,
      );
    }
  }
}

function writeBigUint(out: Uint8Array, value: bigint): void {
  for (let index = 0; index < out.length; index += 1) {
    out[index] = Number((value >> BigInt(index * 8)) & 0xffn);
  }
}

function assertUnsigned(name: string, value: number | bigint, bits: number): void {
  if (typeof value === "number") {
    if (!Number.isInteger(value) || value < 0 || value >= 2 ** bits) {
      throw new BinetteError(`${name} out of range`);
    }
  } else if (value < 0n || value >= 1n << BigInt(bits)) {
    throw new BinetteError(`${name} out of range`);
  }
}

function assertSigned(name: string, value: number, bits: number): void {
  const min = -(2 ** (bits - 1));
  const max = 2 ** (bits - 1) - 1;
  if (!Number.isInteger(value) || value < min || value > max) {
    throw new BinetteError(`${name} out of range`);
  }
}

function assertSignedBig(name: string, value: bigint, bits: number): void {
  const min = -(1n << BigInt(bits - 1));
  const max = (1n << BigInt(bits - 1)) - 1n;
  if (value < min || value > max) {
    throw new BinetteError(`${name} out of range`);
  }
}
