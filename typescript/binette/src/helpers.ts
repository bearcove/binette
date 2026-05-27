import { BinetteError } from "./value.js";
import type { DecodeResult } from "./primitives.js";

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
