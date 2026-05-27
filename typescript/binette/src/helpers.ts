import { BinetteError } from "./value.js";
import { decodeU32, encodeU32, type DecodeResult } from "./primitives.js";

export function concat(...parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((n, part) => n + part.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

export function encodeOption<T>(
  value: T | null,
  encodeInner: (value: T) => Uint8Array,
): Uint8Array {
  if (value === null) {
    return Uint8Array.of(0);
  }
  return concat(Uint8Array.of(1), encodeInner(value));
}

export function decodeOption<T>(
  buf: Uint8Array,
  offset: number,
  decodeInner: (buf: Uint8Array, offset: number) => DecodeResult<T>,
): DecodeResult<T | null> {
  if (offset < 0 || offset >= buf.length) {
    throw new BinetteError("option: input ended before tag");
  }
  const tag = buf[offset];
  if (tag === 0) {
    return { value: null, next: offset + 1 };
  }
  if (tag === 1) {
    return decodeInner(buf, offset + 1);
  }
  throw new BinetteError(`option: invalid tag ${tag}`);
}

export function encodeVec<T>(
  values: readonly T[],
  encodeItem: (value: T) => Uint8Array,
): Uint8Array {
  return concat(encodeU32(values.length), ...values.map(encodeItem));
}

export function decodeVec<T>(
  buf: Uint8Array,
  offset: number,
  decodeItem: (buf: Uint8Array, offset: number) => DecodeResult<T>,
): DecodeResult<T[]> {
  const len = decodeU32(buf, offset);
  let next = len.next;
  const values: T[] = [];
  for (let index = 0; index < len.value; index += 1) {
    const item = decodeItem(buf, next);
    values.push(item.value);
    next = item.next;
  }
  return { value: values, next };
}

export function encodeTuple2<A, B>(
  a: A,
  b: B,
  encodeA: (value: A) => Uint8Array,
  encodeB: (value: B) => Uint8Array,
): Uint8Array {
  return concat(encodeA(a), encodeB(b));
}

export function decodeTuple2<A, B>(
  buf: Uint8Array,
  offset: number,
  decodeA: (buf: Uint8Array, offset: number) => DecodeResult<A>,
  decodeB: (buf: Uint8Array, offset: number) => DecodeResult<B>,
): DecodeResult<[A, B]> {
  const a = decodeA(buf, offset);
  const b = decodeB(buf, a.next);
  return { value: [a.value, b.value], next: b.next };
}

export function encodeTuple3<A, B, C>(
  a: A,
  b: B,
  c: C,
  encodeA: (value: A) => Uint8Array,
  encodeB: (value: B) => Uint8Array,
  encodeC: (value: C) => Uint8Array,
): Uint8Array {
  return concat(encodeA(a), encodeB(b), encodeC(c));
}

export function decodeTuple3<A, B, C>(
  buf: Uint8Array,
  offset: number,
  decodeA: (buf: Uint8Array, offset: number) => DecodeResult<A>,
  decodeB: (buf: Uint8Array, offset: number) => DecodeResult<B>,
  decodeC: (buf: Uint8Array, offset: number) => DecodeResult<C>,
): DecodeResult<[A, B, C]> {
  const a = decodeA(buf, offset);
  const b = decodeB(buf, a.next);
  const c = decodeC(buf, b.next);
  return { value: [a.value, b.value, c.value], next: c.next };
}

export function encodeEnumVariant(variantIndex: number): Uint8Array {
  return encodeU32(variantIndex);
}

export function decodeEnumVariant(
  buf: Uint8Array,
  offset: number,
): DecodeResult<number> {
  return decodeU32(buf, offset);
}
