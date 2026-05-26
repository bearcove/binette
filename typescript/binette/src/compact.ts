import {
  BinetteError,
  decodeSelfDescribedPrefix,
  encodeDynamicValue,
  type Value,
} from "./value.js";

import { SchemaRegistry } from "./registry.js";
import {
  primitiveForTypeId,
  type Field,
  type Primitive,
  type SchemaKind,
  type TypeRef,
  type VariantPayload,
} from "./schema.js";

export type ExternalAttachmentSlot = {
  bytePosition: number;
  kind: string;
  metadata: Value;
};

// r[impl binette.mode.compact]
export function encodeCompact(
  value: Value,
  typeRef: TypeRef,
  registry: SchemaRegistry,
): Uint8Array {
  const writer = new CompactWriter();
  writer.writeType(value, typeRef, registry, new Env());
  return writer.finish();
}

// r[impl binette.mode.compact]
export function decodeCompact(
  bytes: Uint8Array,
  typeRef: TypeRef,
  registry: SchemaRegistry,
): Value {
  const reader = new CompactReader(bytes);
  const value = reader.readType(typeRef, registry, new Env());
  if (!reader.isEmpty()) {
    throw new BinetteError(
      `trailing bytes after compact value at byte ${reader.position}: ${reader.remaining} bytes remain`,
    );
  }
  return value;
}

export function skipCompact(
  bytes: Uint8Array,
  typeRef: TypeRef,
  registry: SchemaRegistry,
): number {
  const reader = new CompactReader(bytes);
  reader.skipValue(typeRef, registry);
  return reader.position;
}

export function externalAttachmentSlots(
  bytes: Uint8Array,
  typeRef: TypeRef,
  registry: SchemaRegistry,
): ExternalAttachmentSlot[] {
  const reader = new CompactReader(bytes);
  return reader.externalAttachmentSlots(typeRef, registry);
}

export class CompactReader {
  #offset = 0;

  constructor(private readonly bytes: Uint8Array) {}

  get position(): number {
    return this.#offset;
  }

  get remaining(): number {
    return this.bytes.length - this.#offset;
  }

  isEmpty(): boolean {
    return this.remaining === 0;
  }

  consumedFrom(start: number): Uint8Array {
    return this.bytes.slice(start, this.#offset);
  }

  // r[impl binette.aggregate.schema-driven-skip]
  skipValue(typeRef: TypeRef, registry: SchemaRegistry): void {
    this.walkType(typeRef, registry, new Env(), () => undefined);
  }

  // r[impl binette.aggregate.external-attachment]
  externalAttachmentSlots(
    typeRef: TypeRef,
    registry: SchemaRegistry,
  ): ExternalAttachmentSlot[] {
    const slots: ExternalAttachmentSlot[] = [];
    this.walkType(typeRef, registry, new Env(), (slot) => slots.push(slot));
    return slots;
  }

  readType(typeRef: TypeRef, registry: SchemaRegistry, env: Env): Value {
    const resolved = resolveTypeRef(typeRef, env);
    switch (resolved.kind) {
      case "concrete": {
        const primitive = primitiveForTypeId(resolved.typeId);
        if (primitive !== null) {
          return this.readPrimitive(primitive);
        }

        const schema = registry.get(resolved.typeId);
        if (schema === undefined) {
          throw new BinetteError(
            `unknown compact type id ${resolved.typeId} at byte ${this.position}`,
          );
        }
        const mark = env.push(schema.typeParams, resolved.args);
        const value = this.readKind(schema.kind, registry, env);
        env.truncate(mark);
        return value;
      }
      case "var":
        throw new BinetteError(
          `unbound type parameter ${resolved.name} at byte ${this.position}`,
        );
    }
  }

  private readKind(
    kind: SchemaKind,
    registry: SchemaRegistry,
    env: Env,
  ): Value {
    switch (kind.kind) {
      case "primitive":
        return this.readPrimitive(kind.primitive);
      // r[impl binette.aggregate.struct.compact]
      case "struct":
        return {
          kind: "struct",
          fields: kind.fields.map((field) => ({
            name: field.name,
            value: this.readType(field.typeRef, registry, env),
          })),
        };
      // r[impl binette.aggregate.enum.compact]
      case "enum": {
        const position = this.position;
        const variantIndex = this.u32();
        const variant = kind.variants.find((candidate) => candidate.index === variantIndex);
        if (variant === undefined) {
          throw new BinetteError(
            `compact enum variant index ${variantIndex} is out of range at byte ${position}`,
          );
        }
        return {
          kind: "enum",
          variant: variant.name,
          payload: this.readVariantPayload(variant.payload, registry, env),
        };
      }
      case "tuple":
        return {
          kind: "tuple",
          elements: kind.elements.map((element) =>
            this.readType(element, registry, env),
          ),
        };
      case "list": {
        const count = this.u32();
        const elements: Value[] = [];
        for (let index = 0; index < count; index += 1) {
          elements.push(this.readType(kind.element, registry, env));
        }
        return { kind: "list", elements };
      }
      case "set": {
        const count = this.u32();
        const elements: Value[] = [];
        let previous: Uint8Array | null = null;
        for (let index = 0; index < count; index += 1) {
          const start = this.position;
          const element = this.readType(kind.element, registry, env);
          rejectNanKey(element, "set");
          previous = validateCanonicalBytes(this.consumedFrom(start), previous, "set");
          elements.push(element);
        }
        return { kind: "set", elements };
      }
      case "map": {
        const count = this.u32();
        const entries: Array<{ key: Value; value: Value }> = [];
        let previous: Uint8Array | null = null;
        for (let index = 0; index < count; index += 1) {
          const keyStart = this.position;
          const key = this.readType(kind.key, registry, env);
          rejectNanKey(key, "map");
          previous = validateCanonicalBytes(this.consumedFrom(keyStart), previous, "map");
          const value = this.readType(kind.value, registry, env);
          entries.push({ key, value });
        }
        return { kind: "map", entries };
      }
      case "array": {
        const count = arrayElementCount(kind.dimensions);
        const elements: Value[] = [];
        for (let index = 0; index < count; index += 1) {
          elements.push(this.readType(kind.element, registry, env));
        }
        return { kind: "array", dimensions: [...kind.dimensions], elements };
      }
      case "option": {
        const position = this.position;
        const tag = this.u8();
        if (tag === 0x00) {
          return { kind: "option", value: null };
        }
        if (tag === 0x01) {
          return {
            kind: "option",
            value: this.readType(kind.element, registry, env),
          };
        }
        throw new BinetteError(
          `invalid option tag ${tag} at byte ${position}`,
        );
      }
      case "dynamic": {
        const { value, bytesRead } = decodeSelfDescribedPrefix(this.remainingBytes());
        this.#offset += bytesRead;
        return { kind: "dynamic", value };
      }
      case "external":
        return { kind: "externalAttachment" };
    }
  }

  private readVariantPayload(
    payload: VariantPayload,
    registry: SchemaRegistry,
    env: Env,
  ): Value {
    switch (payload.kind) {
      case "unit":
        return { kind: "unit" };
      case "newtype":
        return this.readType(payload.typeRef, registry, env);
      case "tuple":
        return {
          kind: "tuple",
          elements: payload.elements.map((element) =>
            this.readType(element, registry, env),
          ),
        };
      case "struct":
        return {
          kind: "struct",
          fields: payload.fields.map((field) => ({
            name: field.name,
            value: this.readType(field.typeRef, registry, env),
          })),
        };
    }
  }

  private walkType(
    typeRef: TypeRef,
    registry: SchemaRegistry,
    env: Env,
    onExternal: (slot: ExternalAttachmentSlot) => void,
  ): void {
    const resolved = resolveTypeRef(typeRef, env);
    switch (resolved.kind) {
      case "concrete": {
        const primitive = primitiveForTypeId(resolved.typeId);
        if (primitive !== null) {
          this.skipPrimitive(primitive);
          return;
        }

        const schema = registry.get(resolved.typeId);
        if (schema === undefined) {
          throw new BinetteError(
            `unknown compact type id ${resolved.typeId} at byte ${this.position}`,
          );
        }
        const mark = env.push(schema.typeParams, resolved.args);
        this.walkKind(schema.kind, registry, env, onExternal);
        env.truncate(mark);
        break;
      }
      case "var":
        throw new BinetteError(
          `unbound type parameter ${resolved.name} at byte ${this.position}`,
        );
    }
  }

  private walkKind(
    kind: SchemaKind,
    registry: SchemaRegistry,
    env: Env,
    onExternal: (slot: ExternalAttachmentSlot) => void,
  ): void {
    switch (kind.kind) {
      case "primitive":
        this.skipPrimitive(kind.primitive);
        break;
      case "struct":
        for (const field of kind.fields) {
          this.walkType(field.typeRef, registry, env, onExternal);
        }
        break;
      case "enum": {
        const position = this.position;
        const variantIndex = this.u32();
        const variant = kind.variants.find((candidate) => candidate.index === variantIndex);
        if (variant === undefined) {
          throw new BinetteError(
            `compact enum variant index ${variantIndex} is out of range at byte ${position}`,
          );
        }
        this.walkVariantPayload(variant.payload, registry, env, onExternal);
        break;
      }
      case "tuple":
        for (const element of kind.elements) {
          this.walkType(element, registry, env, onExternal);
        }
        break;
      case "list":
      case "set": {
        const count = this.u32();
        for (let index = 0; index < count; index += 1) {
          this.walkType(kind.element, registry, env, onExternal);
        }
        break;
      }
      case "map": {
        const count = this.u32();
        for (let index = 0; index < count; index += 1) {
          this.walkType(kind.key, registry, env, onExternal);
          this.walkType(kind.value, registry, env, onExternal);
        }
        break;
      }
      case "array": {
        const count = arrayElementCount(kind.dimensions);
        for (let index = 0; index < count; index += 1) {
          this.walkType(kind.element, registry, env, onExternal);
        }
        break;
      }
      case "option": {
        const position = this.position;
        const tag = this.u8();
        if (tag === 0x00) {
          break;
        }
        if (tag === 0x01) {
          this.walkType(kind.element, registry, env, onExternal);
          break;
        }
        throw new BinetteError(`invalid option tag ${tag} at byte ${position}`);
      }
      case "dynamic": {
        const { bytesRead } = decodeSelfDescribedPrefix(this.remainingBytes());
        this.#offset += bytesRead;
        break;
      }
      case "external":
        onExternal({
          bytePosition: this.position,
          kind: kind.externalKind,
          metadata: kind.metadata,
        });
        break;
    }
  }

  private walkVariantPayload(
    payload: VariantPayload,
    registry: SchemaRegistry,
    env: Env,
    onExternal: (slot: ExternalAttachmentSlot) => void,
  ): void {
    switch (payload.kind) {
      case "unit":
        break;
      case "newtype":
        this.walkType(payload.typeRef, registry, env, onExternal);
        break;
      case "tuple":
        for (const element of payload.elements) {
          this.walkType(element, registry, env, onExternal);
        }
        break;
      case "struct":
        for (const field of payload.fields) {
          this.walkType(field.typeRef, registry, env, onExternal);
        }
        break;
    }
  }

  private readPrimitive(primitive: Primitive): Value {
    switch (primitive) {
      case "unit":
        return { kind: "unit" };
      // r[impl binette.scalar.never]
      case "never":
        throw new BinetteError(`never type has no compact value at byte ${this.position}`);
      case "bool":
        return { kind: "bool", value: this.bool() };
      case "u8":
        return { kind: "u8", value: this.u8() };
      case "u16":
        return { kind: "u16", value: this.u16() };
      case "u32":
        return { kind: "u32", value: this.u32() };
      case "u64":
        return { kind: "u64", value: this.u64() };
      case "u128":
        return { kind: "u128", value: this.u128() };
      case "i8":
        return { kind: "i8", value: this.i8() };
      case "i16":
        return { kind: "i16", value: this.i16() };
      case "i32":
        return { kind: "i32", value: this.i32() };
      case "i64":
        return { kind: "i64", value: this.i64() };
      case "i128":
        return { kind: "i128", value: this.i128() };
      case "f32":
        return this.f32();
      case "f64":
        return this.f64();
      case "char":
        return { kind: "char", value: this.char() };
      case "string":
        return { kind: "string", value: this.string() };
      case "bytes":
        return { kind: "bytes", value: this.bytesWithLength() };
      case "payload":
        return { kind: "payload", value: this.bytesWithLength() };
    }
  }

  private skipPrimitive(primitive: Primitive): void {
    switch (primitive) {
      case "unit":
        break;
      // r[impl binette.scalar.never]
      case "never":
        throw new BinetteError(`never type has no compact value at byte ${this.position}`);
      case "bool":
        this.bool();
        break;
      case "u8":
      case "i8":
        this.skipBytes(1);
        break;
      case "u16":
      case "i16":
        this.skipBytes(2);
        break;
      case "u32":
      case "i32":
      case "f32":
      case "char":
        if (primitive === "char") {
          this.char();
        } else {
          this.skipBytes(4);
        }
        break;
      case "u64":
      case "i64":
      case "f64":
        this.skipBytes(8);
        break;
      case "u128":
      case "i128":
        this.skipBytes(16);
        break;
      case "string":
        this.string();
        break;
      case "bytes":
      case "payload":
        this.skipBytes(this.u32());
        break;
    }
  }

  private bool(): boolean {
    const position = this.position;
    const value = this.u8();
    if (value === 0x00) {
      return false;
    }
    if (value === 0x01) {
      return true;
    }
    throw new BinetteError(`invalid bool byte ${value} at byte ${position}`);
  }

  private u8(): number {
    this.require(1);
    const value = this.bytes[this.#offset];
    if (value === undefined) {
      throw new BinetteError("read past end of compact input");
    }
    this.#offset += 1;
    return value;
  }

  private u16(): number {
    const value = this.view(2).getUint16(0, true);
    this.#offset += 2;
    return value;
  }

  private u32(): number {
    const value = this.view(4).getUint32(0, true);
    this.#offset += 4;
    return value;
  }

  private u64(): bigint {
    return this.bigUint(8);
  }

  private u128(): bigint {
    return this.bigUint(16);
  }

  private i8(): number {
    const value = this.view(1).getInt8(0);
    this.#offset += 1;
    return value;
  }

  private i16(): number {
    const value = this.view(2).getInt16(0, true);
    this.#offset += 2;
    return value;
  }

  private i32(): number {
    const value = this.view(4).getInt32(0, true);
    this.#offset += 4;
    return value;
  }

  private i64(): bigint {
    return BigInt.asIntN(64, this.bigUint(8));
  }

  private i128(): bigint {
    return BigInt.asIntN(128, this.bigUint(16));
  }

  private f32(): Extract<Value, { kind: "f32" }> {
    const bits = this.u32();
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setUint32(0, bits, true);
    return {
      kind: "f32",
      value: new DataView(bytes.buffer).getFloat32(0, true),
      bits,
    };
  }

  private f64(): Extract<Value, { kind: "f64" }> {
    const bits = this.u64();
    const bytes = new Uint8Array(8);
    writeBigUint(bytes, bits);
    return {
      kind: "f64",
      value: new DataView(bytes.buffer).getFloat64(0, true),
      bits,
    };
  }

  private char(): string {
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

  private string(): string {
    return stringFromUtf8(this.bytesWithLength());
  }

  private bytesWithLength(): Uint8Array {
    return this.raw(this.u32());
  }

  private remainingBytes(): Uint8Array {
    return this.bytes.slice(this.#offset);
  }

  private raw(length: number): Uint8Array {
    this.require(length);
    const value = this.bytes.slice(this.#offset, this.#offset + length);
    this.#offset += length;
    return value;
  }

  private skipBytes(length: number): void {
    this.require(length);
    this.#offset += length;
  }

  private view(length: number): DataView {
    this.require(length);
    return new DataView(
      this.bytes.buffer,
      this.bytes.byteOffset + this.#offset,
      length,
    );
  }

  private bigUint(byteCount: number): bigint {
    this.require(byteCount);
    let value = 0n;
    for (let index = 0; index < byteCount; index += 1) {
      const byte = this.bytes[this.#offset + index];
      if (byte === undefined) {
        throw new BinetteError("read past end of compact input");
      }
      value |= BigInt(byte) << BigInt(index * 8);
    }
    this.#offset += byteCount;
    return value;
  }

  private require(length: number): void {
    if (this.remaining < length) {
      throw new BinetteError(
        `input ended at byte ${this.#offset}; ${length} bytes required`,
      );
    }
  }
}

class CompactWriter {
  readonly #chunks: number[] = [];

  finish(): Uint8Array {
    return Uint8Array.from(this.#chunks);
  }

  writeType(
    value: Value,
    typeRef: TypeRef,
    registry: SchemaRegistry,
    env: Env,
  ): void {
    const resolved = resolveTypeRef(typeRef, env);
    switch (resolved.kind) {
      case "concrete": {
        const primitive = primitiveForTypeId(resolved.typeId);
        if (primitive !== null) {
          this.writePrimitive(value, primitive);
          return;
        }

        const schema = registry.get(resolved.typeId);
        if (schema === undefined) {
          throw new BinetteError(`unknown compact type id ${resolved.typeId}`);
        }
        const mark = env.push(schema.typeParams, resolved.args);
        this.writeKind(value, schema.kind, registry, env);
        env.truncate(mark);
        break;
      }
      case "var":
        throw new BinetteError(`unbound type parameter ${resolved.name}`);
    }
  }

  private writeKind(
    value: Value,
    kind: SchemaKind,
    registry: SchemaRegistry,
    env: Env,
  ): void {
    switch (kind.kind) {
      case "primitive":
        this.writePrimitive(value, kind.primitive);
        break;
      // r[impl binette.aggregate.struct.compact]
      case "struct": {
        const fields = expectStruct(value, kind.fields);
        for (const field of kind.fields) {
          this.writeType(fields.get(field.name) as Value, field.typeRef, registry, env);
        }
        break;
      }
      // r[impl binette.aggregate.enum.compact]
      case "enum": {
        if (value.kind !== "enum") {
          throw new BinetteError(`expected enum, got ${value.kind}`);
        }
        const variant = kind.variants.find((candidate) => candidate.name === value.variant);
        if (variant === undefined) {
          throw new BinetteError(`unknown enum variant ${value.variant}`);
        }
        this.u32(variant.index);
        this.writeVariantPayload(value.payload, variant.payload, registry, env);
        break;
      }
      case "tuple":
        if (value.kind !== "tuple") {
          throw new BinetteError(`expected tuple, got ${value.kind}`);
        }
        if (value.elements.length !== kind.elements.length) {
          throw new BinetteError(
            `tuple expects ${kind.elements.length} elements, got ${value.elements.length}`,
          );
        }
        for (let index = 0; index < kind.elements.length; index += 1) {
          this.writeType(mustValue(value.elements[index]), mustTypeRef(kind.elements[index]), registry, env);
        }
        break;
      case "list":
        if (value.kind !== "list") {
          throw new BinetteError(`expected list, got ${value.kind}`);
        }
        this.u32(value.elements.length);
        for (const element of value.elements) {
          this.writeType(element, kind.element, registry, env);
        }
        break;
      case "set": {
        if (value.kind !== "set") {
          throw new BinetteError(`expected set, got ${value.kind}`);
        }
        const encoded = value.elements.map((element) => {
          rejectNanKey(element, "set");
          return encodeCompact(element, kind.element, registry);
        });
        encoded.sort(compareBytes);
        rejectDuplicateKeys("set", encoded);
        this.u32(encoded.length);
        for (const element of encoded) {
          this.raw(element);
        }
        break;
      }
      case "map": {
        if (value.kind !== "map") {
          throw new BinetteError(`expected map, got ${value.kind}`);
        }
        const encoded = value.entries.map((entry) => {
          rejectNanKey(entry.key, "map");
          return {
            key: encodeCompact(entry.key, kind.key, registry),
            value: encodeCompact(entry.value, kind.value, registry),
          };
        });
        encoded.sort((left, right) => compareBytes(left.key, right.key));
        rejectDuplicateKeys(
          "map",
          encoded.map((entry) => entry.key),
        );
        this.u32(encoded.length);
        for (const entry of encoded) {
          this.raw(entry.key);
          this.raw(entry.value);
        }
        break;
      }
      case "array": {
        if (value.kind !== "array") {
          throw new BinetteError(`expected array, got ${value.kind}`);
        }
        if (!dimensionsEqual(value.dimensions, kind.dimensions)) {
          throw new BinetteError("array dimensions do not match schema");
        }
        const count = arrayElementCount(kind.dimensions);
        if (value.elements.length !== count) {
          throw new BinetteError(
            `array expects ${count} elements, got ${value.elements.length}`,
          );
        }
        for (const element of value.elements) {
          this.writeType(element, kind.element, registry, env);
        }
        break;
      }
      case "option":
        if (value.kind !== "option") {
          throw new BinetteError(`expected option, got ${value.kind}`);
        }
        if (value.value === null) {
          this.u8(0x00);
        } else {
          this.u8(0x01);
          this.writeType(value.value, kind.element, registry, env);
        }
        break;
      case "dynamic":
        if (value.kind !== "dynamic") {
          throw new BinetteError(`expected dynamic, got ${value.kind}`);
        }
        this.raw(encodeDynamicValue(value.value));
        break;
      case "external":
        if (value.kind !== "externalAttachment") {
          throw new BinetteError(`expected external attachment, got ${value.kind}`);
        }
        break;
    }
  }

  private writeVariantPayload(
    value: Value,
    payload: VariantPayload,
    registry: SchemaRegistry,
    env: Env,
  ): void {
    switch (payload.kind) {
      case "unit":
        if (value.kind !== "unit") {
          throw new BinetteError(`expected unit enum payload, got ${value.kind}`);
        }
        break;
      case "newtype":
        this.writeType(value, payload.typeRef, registry, env);
        break;
      case "tuple":
        if (value.kind !== "tuple") {
          throw new BinetteError(`expected tuple enum payload, got ${value.kind}`);
        }
        if (value.elements.length !== payload.elements.length) {
          throw new BinetteError(
            `enum tuple payload expects ${payload.elements.length} elements, got ${value.elements.length}`,
          );
        }
        for (let index = 0; index < payload.elements.length; index += 1) {
          this.writeType(mustValue(value.elements[index]), mustTypeRef(payload.elements[index]), registry, env);
        }
        break;
      case "struct": {
        const fields = expectStruct(value, payload.fields);
        for (const field of payload.fields) {
          this.writeType(fields.get(field.name) as Value, field.typeRef, registry, env);
        }
        break;
      }
    }
  }

  private writePrimitive(value: Value, primitive: Primitive): void {
    switch (primitive) {
      case "unit":
        expectKind(value, "unit");
        break;
      // r[impl binette.scalar.never]
      case "never":
        throw new BinetteError("never type has no compact value");
      case "bool":
        expectKind(value, "bool");
        this.u8(value.value ? 1 : 0);
        break;
      case "u8":
        expectKind(value, "u8");
        this.u8(value.value);
        break;
      case "u16":
        expectKind(value, "u16");
        this.u16(value.value);
        break;
      case "u32":
        expectKind(value, "u32");
        this.u32(value.value);
        break;
      case "u64":
        expectKind(value, "u64");
        this.u64(value.value);
        break;
      case "u128":
        expectKind(value, "u128");
        this.u128(value.value);
        break;
      case "i8":
        expectKind(value, "i8");
        this.i8(value.value);
        break;
      case "i16":
        expectKind(value, "i16");
        this.i16(value.value);
        break;
      case "i32":
        expectKind(value, "i32");
        this.i32(value.value);
        break;
      case "i64":
        expectKind(value, "i64");
        this.i64(value.value);
        break;
      case "i128":
        expectKind(value, "i128");
        this.i128(value.value);
        break;
      case "f32":
        expectKind(value, "f32");
        this.f32(value);
        break;
      case "f64":
        expectKind(value, "f64");
        this.f64(value);
        break;
      case "char":
        expectKind(value, "char");
        this.char(value.value);
        break;
      case "string":
        expectKind(value, "string");
        this.bytesWithLength(utf8(value.value));
        break;
      case "bytes":
        expectKind(value, "bytes");
        this.bytesWithLength(value.value);
        break;
      case "payload":
        expectKind(value, "payload");
        this.bytesWithLength(value.value);
        break;
    }
  }

  private raw(bytes: Uint8Array): void {
    for (const byte of bytes) {
      this.u8(byte);
    }
  }

  private bytesWithLength(bytes: Uint8Array): void {
    this.u32(bytes.length);
    this.raw(bytes);
  }

  private u8(value: number): void {
    assertUnsigned("u8", value, 8);
    this.#chunks.push(value);
  }

  private u16(value: number): void {
    assertUnsigned("u16", value, 16);
    this.fixed(2, (view) => view.setUint16(0, value, true));
  }

  private u32(value: number): void {
    assertUnsigned("u32", value, 32);
    this.fixed(4, (view) => view.setUint32(0, value, true));
  }

  private u64(value: bigint): void {
    this.bigUint(value, 8);
  }

  private u128(value: bigint): void {
    this.bigUint(value, 16);
  }

  private i8(value: number): void {
    assertSigned("i8", value, 8);
    this.fixed(1, (view) => view.setInt8(0, value));
  }

  private i16(value: number): void {
    assertSigned("i16", value, 16);
    this.fixed(2, (view) => view.setInt16(0, value, true));
  }

  private i32(value: number): void {
    assertSigned("i32", value, 32);
    this.fixed(4, (view) => view.setInt32(0, value, true));
  }

  private i64(value: bigint): void {
    assertSignedBig("i64", value, 64);
    this.bigUint(BigInt.asUintN(64, value), 8);
  }

  private i128(value: bigint): void {
    assertSignedBig("i128", value, 128);
    this.bigUint(BigInt.asUintN(128, value), 16);
  }

  private f32(value: Extract<Value, { kind: "f32" }>): void {
    if (value.bits !== undefined) {
      this.u32(value.bits);
    } else {
      this.fixed(4, (view) => view.setFloat32(0, value.value, true));
    }
  }

  private f64(value: Extract<Value, { kind: "f64" }>): void {
    if (value.bits !== undefined) {
      this.bigUint(value.bits, 8);
    } else {
      this.fixed(8, (view) => view.setFloat64(0, value.value, true));
    }
  }

  private char(value: string): void {
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
      this.#chunks.push(Number((value >> BigInt(index * 8)) & 0xffn));
    }
  }
}

class Env {
  readonly #bindings: Array<{ name: string; typeRef: TypeRef }> = [];

  push(typeParams: readonly string[], args: readonly TypeRef[]): number {
    const mark = this.#bindings.length;
    typeParams.forEach((name, index) => {
      const typeRef = args[index];
      if (typeRef === undefined) {
        throw new BinetteError(`missing type argument for ${name}`);
      }
      this.#bindings.push({ name, typeRef });
    });
    return mark;
  }

  truncate(mark: number): void {
    this.#bindings.length = mark;
  }

  resolve(name: string): TypeRef | undefined {
    for (let index = this.#bindings.length - 1; index >= 0; index -= 1) {
      const binding = this.#bindings[index];
      if (binding?.name === name) {
        return binding.typeRef;
      }
    }
    return undefined;
  }
}

function resolveTypeRef(typeRef: TypeRef, env: Env): TypeRef {
  switch (typeRef.kind) {
    case "concrete":
      return typeRef;
    case "var": {
      const resolved = env.resolve(typeRef.name);
      if (resolved === undefined) {
        return typeRef;
      }
      return resolved;
    }
  }
}

function expectStruct(value: Value, fields: Field[]): Map<string, Value> {
  if (value.kind !== "struct") {
    throw new BinetteError(`expected struct, got ${value.kind}`);
  }
  const expected = new Set(fields.map((field) => field.name));
  const values = new Map<string, Value>();
  for (const field of value.fields) {
    if (!expected.has(field.name)) {
      throw new BinetteError(`unexpected struct field ${field.name}`);
    }
    if (values.has(field.name)) {
      throw new BinetteError(`duplicate struct field ${field.name}`);
    }
    values.set(field.name, field.value);
  }
  for (const field of fields) {
    if (!values.has(field.name)) {
      throw new BinetteError(`missing struct field ${field.name}`);
    }
  }
  return values;
}

function expectKind<K extends Value["kind"]>(
  value: Value,
  kind: K,
): asserts value is Extract<Value, { kind: K }> {
  if (value.kind !== kind) {
    throw new BinetteError(`expected ${kind}, got ${value.kind}`);
  }
}

function mustValue(value: Value | undefined): Value {
  if (value === undefined) {
    throw new BinetteError("missing value");
  }
  return value;
}

function mustTypeRef(typeRef: TypeRef | undefined): TypeRef {
  if (typeRef === undefined) {
    throw new BinetteError("missing type reference");
  }
  return typeRef;
}

function arrayElementCount(dimensions: readonly bigint[]): number {
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

function dimensionsEqual(left: readonly bigint[], right: readonly bigint[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  return left.every((dimension, index) => dimension === right[index]);
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
  return new TextEncoder().encode(value);
}

function stringFromUtf8(bytes: Uint8Array): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch (error) {
    throw new BinetteError(`invalid UTF-8 string: ${String(error)}`);
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
