import {
  PlanError,
  readerPlanForBundle,
  type PlanErrorKind,
} from "./plan.js";
import { SchemaRegistry } from "./registry.js";
import { type SchemaBundle } from "./schema.js";

export type CompatibilityStatus =
  | "backward"
  | "forward"
  | "bidirectional"
  | "incompatible";

export type CompatibilityDirection = "backward" | "forward";

export type CompatibilityFailure = {
  direction: CompatibilityDirection;
  path: string;
  reason: CompatibilityFailureReason;
};

export type CompatibilityFailureReason =
  | { kind: PlanErrorKind; detail: unknown }
  | { kind: "schema"; message: string };

export type CompatibilityReport = {
  status: CompatibilityStatus;
  failures: CompatibilityFailure[];
};

// r[impl binette.compat.report]
export function compatibilityReport(
  oldBundle: SchemaBundle,
  newBundle: SchemaBundle,
): CompatibilityReport {
  const oldRegistry = new SchemaRegistry();
  oldRegistry.installBundle(oldBundle);
  const newRegistry = new SchemaRegistry();
  newRegistry.installBundle(newBundle);

  const backward = directionResult(
    "backward",
    () =>
      readerPlanForBundle(
        oldBundle.root,
        oldRegistry,
        newBundle.root,
        newRegistry,
      ),
  );
  const forward = directionResult(
    "forward",
    () =>
      readerPlanForBundle(
        newBundle.root,
        newRegistry,
        oldBundle.root,
        oldRegistry,
      ),
  );
  const failures = [backward.failure, forward.failure].filter(
    (failure): failure is CompatibilityFailure => failure !== null,
  );

  let status: CompatibilityStatus;
  if (backward.ok && forward.ok) {
    status = "bidirectional";
  } else if (backward.ok) {
    status = "backward";
  } else if (forward.ok) {
    status = "forward";
  } else {
    status = "incompatible";
  }

  return { status, failures };
}

function directionResult(
  direction: CompatibilityDirection,
  plan: () => unknown,
): { ok: boolean; failure: CompatibilityFailure | null } {
  try {
    plan();
    return { ok: true, failure: null };
  } catch (error) {
    return { ok: false, failure: compatibilityFailure(direction, error) };
  }
}

function compatibilityFailure(
  direction: CompatibilityDirection,
  error: unknown,
): CompatibilityFailure {
  if (error instanceof PlanError) {
    return {
      direction,
      path: error.path,
      reason: { kind: error.kind, detail: error.detail },
    };
  }
  return {
    direction,
    path: "$",
    reason: { kind: "schema", message: String(error) },
  };
}
