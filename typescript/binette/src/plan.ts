import { BinetteError } from "./value.js";

import { SchemaRegistry } from "./registry.js";
import {
  primitiveForTypeId,
  type Field,
  type Schema,
  type SchemaBundle,
  type SchemaKind,
  type TypeId,
  type TypeRef,
  type Variant,
  type VariantPayload,
} from "./schema.js";

export type ReaderPlan = {
  root: PlanNode;
};

export type PlanNode =
  | { kind: "direct"; writer: TypeRef; reader: TypeRef }
  | { kind: "struct"; fields: StructFieldPlan[] }
  | { kind: "tuple"; elements: PlanNode[] }
  | { kind: "list"; element: PlanNode }
  | { kind: "set"; element: PlanNode }
  | { kind: "map"; key: PlanNode; value: PlanNode }
  | { kind: "array"; dimensions: bigint[]; element: PlanNode }
  | { kind: "enum"; variants: EnumVariantPlan[] }
  | { kind: "option"; element: PlanNode }
  | { kind: "dynamic" };

export type StructFieldPlan =
  | {
      kind: "read";
      writerIndex: number;
      readerIndex: number;
      name: string;
      plan: PlanNode;
    }
  | {
      kind: "skip";
      writerIndex: number;
      name: string;
      writerType: TypeRef;
    };

export type EnumVariantPlan =
  | {
      kind: "read";
      writerIndex: number;
      readerIndex: number;
      name: string;
      payload: EnumPayloadPlan;
    }
  | {
      kind: "reject";
      writerIndex: number;
      name: string;
    };

export type EnumPayloadPlan =
  | { kind: "unit" }
  | { kind: "newtype"; plan: PlanNode }
  | { kind: "tuple"; elements: PlanNode[] }
  | { kind: "struct"; fields: StructFieldPlan[] };

export type PlanErrorKind =
  | "unknownWriterType"
  | "unknownReaderType"
  | "unboundTypeParameter"
  | "typeMismatch"
  | "missingReaderField"
  | "unsupported";

export class PlanError extends BinetteError {
  constructor(
    readonly kind: PlanErrorKind,
    readonly path: string,
    message: string,
    readonly detail: unknown = undefined,
  ) {
    super(`${message} at ${path}`);
    this.name = "PlanError";
  }
}

// r[impl binette.compat.plan]
export function readerPlanForBundle(
  writerRoot: TypeRef,
  writerRegistry: SchemaRegistry,
  readerRoot: TypeRef,
  readerRegistry: SchemaRegistry,
): ReaderPlan {
  const builder = new PlanBuilder(writerRegistry, readerRegistry);
  return {
    root: builder.planType(writerRoot, Env.empty(), readerRoot, Env.empty(), "$"),
  };
}

// r[impl binette.compat.plan]
export function readerPlanForBundles(
  writerBundle: SchemaBundle,
  readerBundle: SchemaBundle,
): ReaderPlan {
  const writerRegistry = new SchemaRegistry();
  writerRegistry.installBundle(writerBundle);
  const readerRegistry = new SchemaRegistry();
  readerRegistry.installBundle(readerBundle);
  return readerPlanForBundle(
    writerBundle.root,
    writerRegistry,
    readerBundle.root,
    readerRegistry,
  );
}

type ResolvedKind =
  | { kind: "primitive" }
  | { kind: "schema"; schema: Schema; env: Env };

type SchemaPlanInput = {
  typeRef: TypeRef;
  schema: Schema;
  env: Env;
};

class PlanBuilder {
  constructor(
    private readonly writerRegistry: SchemaRegistry,
    private readonly readerRegistry: SchemaRegistry,
  ) {}

  // r[impl binette.compat.type-compat]
  // r[impl binette.compat.type-compat.basic]
  planType(
    writerRef: TypeRef,
    writerEnv: Env,
    readerRef: TypeRef,
    readerEnv: Env,
    path: string,
  ): PlanNode {
    const resolvedWriter = this.resolveTypeRef(writerRef, writerEnv, path);
    const resolvedReader = this.resolveTypeRef(readerRef, readerEnv, path);

    if (typeRefsEqual(resolvedWriter, resolvedReader)) {
      this.knownTypeRef(resolvedWriter, this.writerRegistry, path, "writer");
      this.knownTypeRef(resolvedReader, this.readerRegistry, path, "reader");
      return { kind: "direct", writer: resolvedWriter, reader: resolvedReader };
    }

    const writerKind = this.resolveKind(
      resolvedWriter,
      this.writerRegistry,
      path,
      "writer",
    );
    const readerKind = this.resolveKind(
      resolvedReader,
      this.readerRegistry,
      path,
      "reader",
    );

    if (writerKind.kind === "primitive" || readerKind.kind === "primitive") {
      throw this.typeMismatch(path, resolvedWriter, resolvedReader);
    }

    return this.planSchemaPair(
      {
        typeRef: resolvedWriter,
        schema: writerKind.schema,
        env: writerKind.env,
      },
      {
        typeRef: resolvedReader,
        schema: readerKind.schema,
        env: readerKind.env,
      },
      path,
    );
  }

  private planSchemaPair(
    writerInput: SchemaPlanInput,
    readerInput: SchemaPlanInput,
    path: string,
  ): PlanNode {
    const writer = writerInput.schema.kind;
    const reader = readerInput.schema.kind;
    if (writer.kind === "struct" && reader.kind === "struct") {
      return this.planStruct(
        writer.fields,
        writerInput.env,
        reader.fields,
        readerInput.env,
        path,
      );
    }
    if (writer.kind === "tuple" && reader.kind === "tuple") {
      return {
        kind: "tuple",
        elements: this.planTupleElements(
          writer.elements,
          writerInput.env,
          reader.elements,
          readerInput.env,
          path,
        ),
      };
    }
    if (writer.kind === "list" && reader.kind === "list") {
      return {
        kind: "list",
        element: this.planType(
          writer.element,
          writerInput.env,
          reader.element,
          readerInput.env,
          `${path}[]`,
        ),
      };
    }
    if (writer.kind === "set" && reader.kind === "set") {
      return {
        kind: "set",
        element: this.planType(
          writer.element,
          writerInput.env,
          reader.element,
          readerInput.env,
          `${path}{}`,
        ),
      };
    }
    if (writer.kind === "map" && reader.kind === "map") {
      return {
        kind: "map",
        key: this.planType(
          writer.key,
          writerInput.env,
          reader.key,
          readerInput.env,
          `${path}.key`,
        ),
        value: this.planType(
          writer.value,
          writerInput.env,
          reader.value,
          readerInput.env,
          `${path}.value`,
        ),
      };
    }
    if (
      writer.kind === "array" &&
      reader.kind === "array" &&
      dimensionsEqual(writer.dimensions, reader.dimensions)
    ) {
      return {
        kind: "array",
        dimensions: [...writer.dimensions],
        element: this.planType(
          writer.element,
          writerInput.env,
          reader.element,
          readerInput.env,
          `${path}[]`,
        ),
      };
    }
    if (writer.kind === "option" && reader.kind === "option") {
      return {
        kind: "option",
        element: this.planType(
          writer.element,
          writerInput.env,
          reader.element,
          readerInput.env,
          `${path}?`,
        ),
      };
    }
    if (writer.kind === "dynamic" && reader.kind === "dynamic") {
      return { kind: "dynamic" };
    }
    if (writer.kind === "enum" && reader.kind === "enum") {
      return this.planEnum(
        writer.variants,
        writerInput.env,
        reader.variants,
        readerInput.env,
        path,
      );
    }

    throw this.typeMismatch(path, writerInput.typeRef, readerInput.typeRef);
  }

  // r[impl binette.compat.field-matching]
  // r[impl binette.compat.skip-unknown]
  private planStruct(
    writerFields: Field[],
    writerEnv: Env,
    readerFields: Field[],
    readerEnv: Env,
    path: string,
  ): PlanNode {
    const readerByName = new Map<string, { index: number; field: Field }>();
    readerFields.forEach((field, index) => {
      readerByName.set(field.name, { index, field });
    });
    const matchedReaders = new Array<boolean>(readerFields.length).fill(false);
    const fields: StructFieldPlan[] = [];

    writerFields.forEach((writerField, writerIndex) => {
      const reader = readerByName.get(writerField.name);
      if (reader === undefined) {
        fields.push({
          kind: "skip",
          writerIndex,
          name: writerField.name,
          writerType: this.resolveTypeRef(writerField.typeRef, writerEnv, path),
        });
      } else {
        matchedReaders[reader.index] = true;
        fields.push({
          kind: "read",
          writerIndex,
          readerIndex: reader.index,
          name: writerField.name,
          plan: this.planType(
            writerField.typeRef,
            writerEnv,
            reader.field.typeRef,
            readerEnv,
            `${path}.${writerField.name}`,
          ),
        });
      }
    });

    readerFields.forEach((readerField, readerIndex) => {
      if (!matchedReaders[readerIndex]) {
        // r[impl binette.compat.fill-defaults]
        throw new PlanError(
          "missingReaderField",
          path,
          `reader field ${readerField.name} is missing from writer struct`,
          { field: readerField.name },
        );
      }
    });

    return { kind: "struct", fields };
  }

  // r[impl binette.compat.enum]
  // r[impl binette.compat.enum.unknown-variant]
  // r[impl binette.compat.enum.missing-variant]
  // r[impl binette.compat.enum.payload]
  private planEnum(
    writerVariants: Variant[],
    writerEnv: Env,
    readerVariants: Variant[],
    readerEnv: Env,
    path: string,
  ): PlanNode {
    const readerByName = new Map<string, { index: number; variant: Variant }>();
    readerVariants.forEach((variant, index) => {
      readerByName.set(variant.name, { index, variant });
    });

    return {
      kind: "enum",
      variants: writerVariants.map((writerVariant) => {
        const reader = readerByName.get(writerVariant.name);
        if (reader === undefined) {
          return {
            kind: "reject",
            writerIndex: writerVariant.index,
            name: writerVariant.name,
          };
        }
        return {
          kind: "read",
          writerIndex: writerVariant.index,
          readerIndex: reader.index,
          name: writerVariant.name,
          payload: this.planVariantPayload(
            writerVariant.payload,
            writerEnv,
            reader.variant.payload,
            readerEnv,
            `${path}.${writerVariant.name}`,
          ),
        };
      }),
    };
  }

  private planVariantPayload(
    writerPayload: VariantPayload,
    writerEnv: Env,
    readerPayload: VariantPayload,
    readerEnv: Env,
    path: string,
  ): EnumPayloadPlan {
    if (writerPayload.kind === "unit" && readerPayload.kind === "unit") {
      return { kind: "unit" };
    }
    if (writerPayload.kind === "newtype" && readerPayload.kind === "newtype") {
      return {
        kind: "newtype",
        plan: this.planType(
          writerPayload.typeRef,
          writerEnv,
          readerPayload.typeRef,
          readerEnv,
          path,
        ),
      };
    }
    if (writerPayload.kind === "tuple" && readerPayload.kind === "tuple") {
      return {
        kind: "tuple",
        elements: this.planTupleElements(
          writerPayload.elements,
          writerEnv,
          readerPayload.elements,
          readerEnv,
          path,
        ),
      };
    }
    if (writerPayload.kind === "struct" && readerPayload.kind === "struct") {
      const plan = this.planStruct(
        writerPayload.fields,
        writerEnv,
        readerPayload.fields,
        readerEnv,
        path,
      );
      if (plan.kind !== "struct") {
        throw new BinetteError("planStruct returned non-struct plan");
      }
      return { kind: "struct", fields: plan.fields };
    }

    throw new PlanError(
      "unsupported",
      path,
      "enum variant payload kind differs",
    );
  }

  // r[impl binette.compat.tuple]
  private planTupleElements(
    writer: TypeRef[],
    writerEnv: Env,
    reader: TypeRef[],
    readerEnv: Env,
    path: string,
  ): PlanNode[] {
    if (writer.length !== reader.length) {
      throw new PlanError("unsupported", path, "tuple arity differs");
    }

    return writer.map((writerElement, index) =>
      this.planType(
        writerElement,
        writerEnv,
        mustTypeRef(reader[index]),
        readerEnv,
        `${path}.${index}`,
      ),
    );
  }

  private resolveTypeRef(typeRef: TypeRef, env: Env, path: string): TypeRef {
    switch (typeRef.kind) {
      case "concrete":
        return {
          kind: "concrete",
          typeId: typeRef.typeId,
          args: typeRef.args.map((arg) => this.resolveTypeRef(arg, env, path)),
        };
      case "var": {
        const resolved = env.get(typeRef.name);
        if (resolved === undefined) {
          throw new PlanError(
            "unboundTypeParameter",
            path,
            `unbound type parameter ${typeRef.name}`,
            { name: typeRef.name },
          );
        }
        return resolved;
      }
    }
  }

  private resolveKind(
    typeRef: TypeRef,
    registry: SchemaRegistry,
    path: string,
    side: "writer" | "reader",
  ): ResolvedKind {
    switch (typeRef.kind) {
      case "concrete": {
        if (primitiveForTypeId(typeRef.typeId) !== null) {
          if (typeRef.args.length === 0) {
            return { kind: "primitive" };
          }
          throw new PlanError(
            "unsupported",
            path,
            "primitive type reference has type arguments",
          );
        }

        const schema = registry.get(typeRef.typeId);
        if (schema === undefined) {
          throw unknownType(side, path, typeRef.typeId);
        }
        return {
          kind: "schema",
          schema,
          env: Env.bind(schema, typeRef.args),
        };
      }
      case "var":
        throw new PlanError(
          "unboundTypeParameter",
          path,
          `unbound type parameter ${typeRef.name}`,
          { name: typeRef.name },
        );
    }
  }

  private knownTypeRef(
    typeRef: TypeRef,
    registry: SchemaRegistry,
    path: string,
    side: "writer" | "reader",
  ): void {
    switch (typeRef.kind) {
      case "concrete":
        if (
          primitiveForTypeId(typeRef.typeId) === null &&
          registry.get(typeRef.typeId) === undefined
        ) {
          throw unknownType(side, path, typeRef.typeId);
        }
        for (const arg of typeRef.args) {
          this.knownTypeRef(arg, registry, path, side);
        }
        break;
      case "var":
        throw new PlanError(
          "unboundTypeParameter",
          path,
          `unbound type parameter ${typeRef.name}`,
          { name: typeRef.name },
        );
    }
  }

  private typeMismatch(path: string, writer: TypeRef, reader: TypeRef): PlanError {
    return new PlanError(
      "typeMismatch",
      path,
      "incompatible types",
      { writer, reader },
    );
  }
}

class Env {
  private constructor(private readonly bindings: Map<string, TypeRef> = new Map()) {}

  static bind(schema: Schema, args: readonly TypeRef[]): Env {
    const bindings = new Map<string, TypeRef>();
    schema.typeParams.forEach((typeParam, index) => {
      const arg = args[index];
      if (arg === undefined) {
        throw new PlanError(
          "unsupported",
          "$",
          `missing type argument for ${typeParam}`,
        );
      }
      bindings.set(typeParam, arg);
    });
    return new Env(bindings);
  }

  static empty(): Env {
    return new Env();
  }

  get(name: string): TypeRef | undefined {
    return this.bindings.get(name);
  }
}

function unknownType(
  side: "writer" | "reader",
  path: string,
  typeId: TypeId,
): PlanError {
  if (side === "writer") {
    return new PlanError(
      "unknownWriterType",
      path,
      `unknown writer type id ${typeId}`,
      { typeId },
    );
  }
  return new PlanError(
    "unknownReaderType",
    path,
    `unknown reader type id ${typeId}`,
    { typeId },
  );
}

function typeRefsEqual(left: TypeRef, right: TypeRef): boolean {
  if (left.kind !== right.kind) {
    return false;
  }
  if (left.kind === "var" && right.kind === "var") {
    return left.name === right.name;
  }
  if (left.kind === "concrete" && right.kind === "concrete") {
    return (
      left.typeId === right.typeId &&
      left.args.length === right.args.length &&
      left.args.every((arg, index) => typeRefsEqual(arg, mustTypeRef(right.args[index])))
    );
  }
  return false;
}

function dimensionsEqual(left: readonly bigint[], right: readonly bigint[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  return left.every((dimension, index) => dimension === right[index]);
}

function mustTypeRef(typeRef: TypeRef | undefined): TypeRef {
  if (typeRef === undefined) {
    throw new BinetteError("missing type reference");
  }
  return typeRef;
}
