import { BinetteError, encodeSelfDescribed } from "./value.js";

import {
  primitiveForTypeId,
  primitiveTypeId,
  recursiveTypeIdMap,
  schemaToValue,
  schemaTypeId,
  type Schema,
  type SchemaBundle,
  type SchemaKind,
  type TypeId,
  type TypeRef,
  type VariantPayload,
} from "./schema.js";

// r[impl binette.schema.registry+2]
export class SchemaRegistry {
  readonly #schemas = new Map<TypeId, Schema>();

  get size(): number {
    return this.#schemas.size;
  }

  get isEmpty(): boolean {
    return this.#schemas.size === 0;
  }

  contains(typeId: TypeId): boolean {
    return primitiveForTypeId(typeId) !== null || this.#schemas.has(typeId);
  }

  get(typeId: TypeId): Schema | undefined {
    return this.#schemas.get(typeId);
  }

  schemas(): IterableIterator<Schema> {
    return this.#schemas.values();
  }

  // r[impl binette.bundle.self-contained]
  validateSelfContainedBundle(bundle: SchemaBundle): void {
    VerifiedSchemaBatch.forSelfContained(this.#schemas, bundle);
  }

  // r[impl binette.schema.registry.install]
  // r[impl binette.bundle.registry]
  installBundle(bundle: SchemaBundle): void {
    const batch = VerifiedSchemaBatch.create(this.#schemas, bundle.schemas);
    batch.validateTypeRef(bundle.root, []);
    for (const attachment of bundle.attachments) {
      if (attachment.metadataSchema !== null) {
        batch.validateTypeRef(attachment.metadataSchema, []);
      }
    }

    for (const schema of batch.intoSchemas()) {
      this.#schemas.set(schema.id, schema);
    }
  }
}

class VerifiedSchemaBatch {
  readonly #existing: ReadonlyMap<TypeId, Schema>;
  readonly #schemas: Map<TypeId, Schema>;

  private constructor(
    existing: ReadonlyMap<TypeId, Schema>,
    schemas: Map<TypeId, Schema>,
  ) {
    this.#existing = existing;
    this.#schemas = schemas;
  }

  static create(
    existing: ReadonlyMap<TypeId, Schema>,
    incoming: readonly Schema[],
  ): VerifiedSchemaBatch {
    const batch = this.collect(existing, incoming);
    batch.validateSchemas();
    batch.verifyDeclaredIds();
    return batch;
  }

  static forSelfContained(
    existing: ReadonlyMap<TypeId, Schema>,
    bundle: SchemaBundle,
  ): VerifiedSchemaBatch {
    const batch = this.collect(existing, bundle.schemas);
    batch.validateSelfContainedBundle(bundle);
    batch.validateSchemas();
    batch.verifyDeclaredIds();
    return batch;
  }

  private static collect(
    existing: ReadonlyMap<TypeId, Schema>,
    incoming: readonly Schema[],
  ): VerifiedSchemaBatch {
    const schemas = new Map<TypeId, Schema>();

    for (const schema of incoming) {
      if (
        schema.kind.kind === "primitive" &&
        schema.id === primitiveTypeId(schema.kind.primitive)
      ) {
        continue;
      }

      const primitive = primitiveForTypeId(schema.id);
      if (primitive !== null) {
        throw new BinetteError(
          `schema id ${schema.id} is reserved for primitive ${primitive}`,
        );
      }

      const installed = existing.get(schema.id);
      if (installed !== undefined) {
        if (!schemasEqual(installed, schema)) {
          throw new BinetteError(`duplicate schema id ${schema.id}`);
        }
        continue;
      }

      const previous = schemas.get(schema.id);
      if (previous !== undefined) {
        if (!schemasEqual(previous, schema)) {
          throw new BinetteError(`duplicate schema id ${schema.id}`);
        }
        continue;
      }

      schemas.set(schema.id, schema);
    }

    return new VerifiedSchemaBatch(existing, schemas);
  }

  intoSchemas(): Schema[] {
    return Array.from(this.#schemas.values());
  }

  validateTypeRef(typeRef: TypeRef, scope: readonly string[]): void {
    switch (typeRef.kind) {
      case "concrete": {
        const primitive = primitiveForTypeId(typeRef.typeId);
        if (primitive !== null) {
          if (typeRef.args.length !== 0) {
            throw new BinetteError(
              `type id ${typeRef.typeId} expects 0 type arguments, got ${typeRef.args.length}`,
            );
          }
        } else {
          const schema = this.schema(typeRef.typeId);
          if (schema === undefined) {
            throw new BinetteError(`unknown type id ${typeRef.typeId}`);
          }
          if (schema.typeParams.length !== typeRef.args.length) {
            throw new BinetteError(
              `type id ${typeRef.typeId} expects ${schema.typeParams.length} type arguments, got ${typeRef.args.length}`,
            );
          }
        }

        for (const arg of typeRef.args) {
          this.validateTypeRef(arg, scope);
        }
        break;
      }
      case "var":
        if (!scope.includes(typeRef.name)) {
          throw new BinetteError(`unknown type parameter ${typeRef.name}`);
        }
        break;
    }
  }

  private validateSelfContainedBundle(bundle: SchemaBundle): void {
    const visited = new Set<TypeId>();
    this.validateSelfContainedTypeRef(bundle.root, [], visited);
    for (const attachment of bundle.attachments) {
      if (attachment.metadataSchema !== null) {
        this.validateSelfContainedTypeRef(
          attachment.metadataSchema,
          [],
          visited,
        );
      }
    }
  }

  private validateSchemas(): void {
    for (const schema of this.#schemas.values()) {
      this.validateTypeParams(schema.typeParams);
      this.validateKind(schema.kind, schema.typeParams);
    }
  }

  private validateTypeParams(typeParams: readonly string[]): void {
    const seen = new Set<string>();
    for (const typeParam of typeParams) {
      if (seen.has(typeParam)) {
        throw new BinetteError(`duplicate type parameter ${typeParam}`);
      }
      seen.add(typeParam);
    }
  }

  private validateKind(kind: SchemaKind, scope: readonly string[]): void {
    switch (kind.kind) {
      case "primitive":
      case "dynamic":
        break;
      case "struct":
        validateName(kind.name);
        for (const field of kind.fields) {
          this.validateTypeRef(field.typeRef, scope);
        }
        break;
      case "enum":
        validateName(kind.name);
        for (const variant of kind.variants) {
          this.validateVariantPayload(variant.payload, scope);
        }
        break;
      case "tuple":
        if (kind.elements.length === 0) {
          throw new BinetteError("tuple schema must not be empty");
        }
        for (const element of kind.elements) {
          this.validateTypeRef(element, scope);
        }
        break;
      case "list":
      case "set":
      case "option":
        this.validateTypeRef(kind.element, scope);
        break;
      case "map":
        this.validateTypeRef(kind.key, scope);
        this.validateTypeRef(kind.value, scope);
        break;
      case "array":
        if (kind.dimensions.length === 0) {
          throw new BinetteError("array schema dimensions must not be empty");
        }
        this.validateTypeRef(kind.element, scope);
        break;
      case "external":
        validateName(kind.externalKind);
        break;
    }
  }

  private validateVariantPayload(
    payload: VariantPayload,
    scope: readonly string[],
  ): void {
    switch (payload.kind) {
      case "unit":
        break;
      case "newtype":
        this.validateTypeRef(payload.typeRef, scope);
        break;
      case "tuple":
        for (const element of payload.elements) {
          this.validateTypeRef(element, scope);
        }
        break;
      case "struct":
        for (const field of payload.fields) {
          this.validateTypeRef(field.typeRef, scope);
        }
        break;
    }
  }

  private validateSelfContainedTypeRef(
    typeRef: TypeRef,
    scope: readonly string[],
    visited: Set<TypeId>,
  ): void {
    switch (typeRef.kind) {
      case "concrete": {
        const primitive = primitiveForTypeId(typeRef.typeId);
        if (primitive !== null) {
          if (typeRef.args.length !== 0) {
            throw new BinetteError(
              `type id ${typeRef.typeId} expects 0 type arguments, got ${typeRef.args.length}`,
            );
          }
        } else {
          const schema = this.schema(typeRef.typeId);
          if (schema === undefined) {
            throw new BinetteError(`missing bundle schema ${typeRef.typeId}`);
          }
          if (schema.typeParams.length !== typeRef.args.length) {
            throw new BinetteError(
              `type id ${typeRef.typeId} expects ${schema.typeParams.length} type arguments, got ${typeRef.args.length}`,
            );
          }
          if (!visited.has(typeRef.typeId)) {
            visited.add(typeRef.typeId);
            this.validateSelfContainedKind(schema.kind, schema.typeParams, visited);
          }
        }

        for (const arg of typeRef.args) {
          this.validateSelfContainedTypeRef(arg, scope, visited);
        }
        break;
      }
      case "var":
        if (!scope.includes(typeRef.name)) {
          throw new BinetteError(`unknown type parameter ${typeRef.name}`);
        }
        break;
    }
  }

  private validateSelfContainedKind(
    kind: SchemaKind,
    scope: readonly string[],
    visited: Set<TypeId>,
  ): void {
    switch (kind.kind) {
      case "primitive":
      case "dynamic":
      case "external":
        break;
      case "struct":
        for (const field of kind.fields) {
          this.validateSelfContainedTypeRef(field.typeRef, scope, visited);
        }
        break;
      case "enum":
        for (const variant of kind.variants) {
          this.validateSelfContainedVariantPayload(
            variant.payload,
            scope,
            visited,
          );
        }
        break;
      case "tuple":
        for (const element of kind.elements) {
          this.validateSelfContainedTypeRef(element, scope, visited);
        }
        break;
      case "list":
      case "set":
      case "option":
        this.validateSelfContainedTypeRef(kind.element, scope, visited);
        break;
      case "map":
        this.validateSelfContainedTypeRef(kind.key, scope, visited);
        this.validateSelfContainedTypeRef(kind.value, scope, visited);
        break;
      case "array":
        this.validateSelfContainedTypeRef(kind.element, scope, visited);
        break;
    }
  }

  private validateSelfContainedVariantPayload(
    payload: VariantPayload,
    scope: readonly string[],
    visited: Set<TypeId>,
  ): void {
    switch (payload.kind) {
      case "unit":
        break;
      case "newtype":
        this.validateSelfContainedTypeRef(payload.typeRef, scope, visited);
        break;
      case "tuple":
        for (const element of payload.elements) {
          this.validateSelfContainedTypeRef(element, scope, visited);
        }
        break;
      case "struct":
        for (const field of payload.fields) {
          this.validateSelfContainedTypeRef(field.typeRef, scope, visited);
        }
        break;
    }
  }

  // r[impl binette.schema.registry.recursive]
  private verifyDeclaredIds(): void {
    for (const component of this.stronglyConnectedComponents()) {
      const firstTypeId = component[0];
      if (firstTypeId === undefined) {
        throw new BinetteError("SCC is unexpectedly empty");
      }
      if (component.length === 1 && !this.hasSelfDependency(firstTypeId)) {
        const schema = mustGet(this.#schemas, firstTypeId);
        const computed = schemaTypeId(schema);
        if (computed !== schema.id) {
          throw new BinetteError(
            `schema declared id ${schema.id} but canonical content hashes to ${computed}`,
          );
        }
      } else {
        const schemas = component.map((typeId) => mustGet(this.#schemas, typeId));
        const computed = recursiveTypeIdMap(schemas);
        for (const schema of schemas) {
          const typeId = computed.get(schema.id);
          if (typeId === undefined) {
            throw new BinetteError(
              `recursive type-id map is missing schema ${schema.id}`,
            );
          }
          if (typeId !== schema.id) {
            throw new BinetteError(
              `schema declared id ${schema.id} but recursive content hashes to ${typeId}`,
            );
          }
        }
      }
    }
  }

  private stronglyConnectedComponents(): TypeId[][] {
    const state: TarjanState = {
      nextIndex: 0,
      indices: new Map(),
      lowlinks: new Map(),
      stack: [],
      onStack: new Set(),
      components: [],
    };

    for (const typeId of this.#schemas.keys()) {
      if (!state.indices.has(typeId)) {
        this.connectComponent(typeId, state);
      }
    }

    return state.components;
  }

  private connectComponent(typeId: TypeId, state: TarjanState): void {
    const index = state.nextIndex;
    state.nextIndex += 1;
    state.indices.set(typeId, index);
    state.lowlinks.set(typeId, index);
    state.stack.push(typeId);
    state.onStack.add(typeId);

    const deps: TypeId[] = [];
    this.collectBatchDeps(mustGet(this.#schemas, typeId).kind, deps);

    for (const dep of deps) {
      if (!state.indices.has(dep)) {
        this.connectComponent(dep, state);
        state.lowlinks.set(
          typeId,
          Math.min(mustGetNumber(state.lowlinks, typeId), mustGetNumber(state.lowlinks, dep)),
        );
      } else if (state.onStack.has(dep)) {
        state.lowlinks.set(
          typeId,
          Math.min(mustGetNumber(state.lowlinks, typeId), mustGetNumber(state.indices, dep)),
        );
      }
    }

    if (
      mustGetNumber(state.lowlinks, typeId) ===
      mustGetNumber(state.indices, typeId)
    ) {
      const component: TypeId[] = [];
      while (state.stack.length > 0) {
        const member = state.stack.pop();
        if (member === undefined) {
          throw new BinetteError("tarjan stack unexpectedly empty");
        }
        state.onStack.delete(member);
        component.push(member);
        if (member === typeId) {
          break;
        }
      }
      state.components.push(component);
    }
  }

  private hasSelfDependency(typeId: TypeId | undefined): boolean {
    if (typeId === undefined) {
      throw new BinetteError("SCC is unexpectedly empty");
    }
    const deps: TypeId[] = [];
    this.collectBatchDeps(mustGet(this.#schemas, typeId).kind, deps);
    return deps.includes(typeId);
  }

  private collectBatchDeps(kind: SchemaKind, out: TypeId[]): void {
    switch (kind.kind) {
      case "primitive":
      case "dynamic":
      case "external":
        break;
      case "struct":
        for (const field of kind.fields) {
          this.collectTypeRefBatchDeps(field.typeRef, out);
        }
        break;
      case "enum":
        for (const variant of kind.variants) {
          this.collectVariantBatchDeps(variant.payload, out);
        }
        break;
      case "tuple":
        for (const element of kind.elements) {
          this.collectTypeRefBatchDeps(element, out);
        }
        break;
      case "list":
      case "set":
      case "option":
        this.collectTypeRefBatchDeps(kind.element, out);
        break;
      case "map":
        this.collectTypeRefBatchDeps(kind.key, out);
        this.collectTypeRefBatchDeps(kind.value, out);
        break;
      case "array":
        this.collectTypeRefBatchDeps(kind.element, out);
        break;
    }
  }

  private collectVariantBatchDeps(payload: VariantPayload, out: TypeId[]): void {
    switch (payload.kind) {
      case "unit":
        break;
      case "newtype":
        this.collectTypeRefBatchDeps(payload.typeRef, out);
        break;
      case "tuple":
        for (const element of payload.elements) {
          this.collectTypeRefBatchDeps(element, out);
        }
        break;
      case "struct":
        for (const field of payload.fields) {
          this.collectTypeRefBatchDeps(field.typeRef, out);
        }
        break;
    }
  }

  private collectTypeRefBatchDeps(typeRef: TypeRef, out: TypeId[]): void {
    switch (typeRef.kind) {
      case "concrete":
        if (this.#schemas.has(typeRef.typeId)) {
          out.push(typeRef.typeId);
        }
        for (const arg of typeRef.args) {
          this.collectTypeRefBatchDeps(arg, out);
        }
        break;
      case "var":
        break;
    }
  }

  private schema(typeId: TypeId): Schema | undefined {
    return this.#schemas.get(typeId) ?? this.#existing.get(typeId);
  }
}

type TarjanState = {
  nextIndex: number;
  indices: Map<TypeId, number>;
  lowlinks: Map<TypeId, number>;
  stack: TypeId[];
  onStack: Set<TypeId>;
  components: TypeId[][];
};

function validateName(name: string): void {
  if (name.length === 0) {
    throw new BinetteError("schema names must not be empty");
  }
}

function schemasEqual(left: Schema, right: Schema): boolean {
  return bytesEqual(
    encodeSelfDescribed(schemaToValue(left)),
    encodeSelfDescribed(schemaToValue(right)),
  );
}

function bytesEqual(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }
  return true;
}

function mustGet(map: ReadonlyMap<TypeId, Schema>, typeId: TypeId): Schema {
  const schema = map.get(typeId);
  if (schema === undefined) {
    throw new BinetteError(`missing batch schema ${typeId}`);
  }
  return schema;
}

function mustGetNumber(map: ReadonlyMap<TypeId, number>, typeId: TypeId): number {
  const value = map.get(typeId);
  if (value === undefined) {
    throw new BinetteError(`missing tarjan state for schema ${typeId}`);
  }
  return value;
}
