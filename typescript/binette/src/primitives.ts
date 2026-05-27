import { BinetteError } from "./value.js";

export type DecodeResult<T> = {
  value: T;
  next: number;
};

function fixed(width: number, write: (view: DataView) => void): Uint8Array {
  const buf = new ArrayBuffer(width);
  write(new DataView(buf));
  return new Uint8Array(buf);
}

function viewFor(
  buf: Uint8Array,
  offset: number,
  width: number,
  context: string,
): DataView {
  if (offset < 0 || offset + width > buf.length) {
    throw new BinetteError(`${context}: input ended before ${width} bytes`);
  }
  return new DataView(buf.buffer, buf.byteOffset + offset, width);
}

export function encodeBool(value: boolean): Uint8Array {
  return Uint8Array.of(value ? 1 : 0);
}

export function decodeBool(buf: Uint8Array, offset: number): DecodeResult<boolean> {
  if (offset < 0 || offset >= buf.length) {
    throw new BinetteError("bool: input ended before 1 byte");
  }
  const byte = buf[offset];
  if (byte === undefined || byte > 1) {
    throw new BinetteError(`bool: invalid value ${byte}`);
  }
  return { value: byte === 1, next: offset + 1 };
}

export function encodeU8(value: number): Uint8Array {
  return Uint8Array.of(value & 0xff);
}

export function decodeU8(buf: Uint8Array, offset: number): DecodeResult<number> {
  if (offset < 0 || offset >= buf.length) {
    throw new BinetteError("u8: input ended before 1 byte");
  }
  const value = buf[offset];
  if (value === undefined) {
    throw new BinetteError("u8: input ended before 1 byte");
  }
  return { value, next: offset + 1 };
}

export function encodeI8(value: number): Uint8Array {
  return Uint8Array.of(value & 0xff);
}

export function decodeI8(buf: Uint8Array, offset: number): DecodeResult<number> {
  return {
    value: viewFor(buf, offset, 1, "i8").getInt8(0),
    next: offset + 1,
  };
}

export function encodeU16(value: number): Uint8Array {
  return fixed(2, (view) => view.setUint16(0, value, true));
}

export function decodeU16(buf: Uint8Array, offset: number): DecodeResult<number> {
  return {
    value: viewFor(buf, offset, 2, "u16").getUint16(0, true),
    next: offset + 2,
  };
}

export function encodeI16(value: number): Uint8Array {
  return fixed(2, (view) => view.setInt16(0, value, true));
}

export function decodeI16(buf: Uint8Array, offset: number): DecodeResult<number> {
  return {
    value: viewFor(buf, offset, 2, "i16").getInt16(0, true),
    next: offset + 2,
  };
}

export function encodeU32(value: number): Uint8Array {
  return fixed(4, (view) => view.setUint32(0, value, true));
}

export function decodeU32(buf: Uint8Array, offset: number): DecodeResult<number> {
  return {
    value: viewFor(buf, offset, 4, "u32").getUint32(0, true),
    next: offset + 4,
  };
}

export function encodeI32(value: number): Uint8Array {
  return fixed(4, (view) => view.setInt32(0, value, true));
}

export function decodeI32(buf: Uint8Array, offset: number): DecodeResult<number> {
  return {
    value: viewFor(buf, offset, 4, "i32").getInt32(0, true),
    next: offset + 4,
  };
}

export function encodeU64(value: bigint): Uint8Array {
  return fixed(8, (view) => view.setBigUint64(0, value, true));
}

export function decodeU64(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  return {
    value: viewFor(buf, offset, 8, "u64").getBigUint64(0, true),
    next: offset + 8,
  };
}

export function encodeI64(value: bigint): Uint8Array {
  return fixed(8, (view) => view.setBigInt64(0, value, true));
}

export function decodeI64(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  return {
    value: viewFor(buf, offset, 8, "i64").getBigInt64(0, true),
    next: offset + 8,
  };
}

export function encodeU128(value: bigint): Uint8Array {
  return fixed(16, (view) => {
    view.setBigUint64(0, value & 0xffff_ffff_ffff_ffffn, true);
    view.setBigUint64(8, value >> 64n, true);
  });
}

export function decodeU128(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  const view = viewFor(buf, offset, 16, "u128");
  const lo = view.getBigUint64(0, true);
  const hi = view.getBigUint64(8, true);
  return { value: lo | (hi << 64n), next: offset + 16 };
}

export function encodeI128(value: bigint): Uint8Array {
  return encodeU128(BigInt.asUintN(128, value));
}

export function decodeI128(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  const decoded = decodeU128(buf, offset);
  return { value: BigInt.asIntN(128, decoded.value), next: decoded.next };
}

export function encodeF32(value: number): Uint8Array {
  return fixed(4, (view) => view.setFloat32(0, value, true));
}

export function decodeF32(buf: Uint8Array, offset: number): DecodeResult<number> {
  return {
    value: viewFor(buf, offset, 4, "f32").getFloat32(0, true),
    next: offset + 4,
  };
}

export function encodeF64(value: number): Uint8Array {
  return fixed(8, (view) => view.setFloat64(0, value, true));
}

export function decodeF64(buf: Uint8Array, offset: number): DecodeResult<number> {
  return {
    value: viewFor(buf, offset, 8, "f64").getFloat64(0, true),
    next: offset + 8,
  };
}

export function encodeString(value: string): Uint8Array {
  return encodeBytes(new TextEncoder().encode(value));
}

export function decodeString(buf: Uint8Array, offset: number): DecodeResult<string> {
  const decoded = decodeBytes(buf, offset);
  return {
    value: new TextDecoder().decode(decoded.value),
    next: decoded.next,
  };
}

export function encodeBytes(value: Uint8Array): Uint8Array {
  const out = new Uint8Array(4 + value.length);
  out.set(encodeU32(value.length), 0);
  out.set(value, 4);
  return out;
}

export function decodeBytes(buf: Uint8Array, offset: number): DecodeResult<Uint8Array> {
  const len = decodeU32(buf, offset);
  const start = len.next;
  const end = start + len.value;
  if (end > buf.length) {
    throw new BinetteError("bytes: input ended before payload");
  }
  return { value: buf.slice(start, end), next: end };
}
