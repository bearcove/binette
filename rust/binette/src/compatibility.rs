use crate::error::SchemaError;
use crate::plan::{PlanError, reader_plan_for_bundle};
use crate::registry::SchemaRegistry;
use crate::schema::{SchemaBundle, TypeId, TypeRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityStatus {
    Backward,
    Forward,
    Bidirectional,
    Incompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityDirection {
    Backward,
    Forward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityFailure {
    pub direction: CompatibilityDirection,
    pub path: String,
    pub reason: CompatibilityFailureReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityFailureReason {
    UnknownWriterType { type_id: TypeId },
    UnknownReaderType { type_id: TypeId },
    UnboundTypeParameter { name: String },
    TypeMismatch { writer: TypeRef, reader: TypeRef },
    MissingReaderField { field: String },
    Unsupported { reason: &'static str },
    Schema { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityReport {
    pub status: CompatibilityStatus,
    pub failures: Vec<CompatibilityFailure>,
}

// r[impl binette.compat.report]
pub fn compatibility_report(
    old: &SchemaBundle,
    new: &SchemaBundle,
) -> Result<CompatibilityReport, SchemaError> {
    let mut old_registry = SchemaRegistry::new();
    old_registry.install_bundle(old)?;
    let mut new_registry = SchemaRegistry::new();
    new_registry.install_bundle(new)?;

    let backward = reader_plan_for_bundle(&old.root, &old_registry, &new.root, &new_registry)
        .map(|_| ())
        .map_err(|error| compatibility_failure(CompatibilityDirection::Backward, error));
    let forward = reader_plan_for_bundle(&new.root, &new_registry, &old.root, &old_registry)
        .map(|_| ())
        .map_err(|error| compatibility_failure(CompatibilityDirection::Forward, error));

    let mut failures = Vec::new();
    let backward_ok = backward.is_ok();
    let forward_ok = forward.is_ok();

    if let Err(failure) = backward {
        failures.push(failure);
    }
    if let Err(failure) = forward {
        failures.push(failure);
    }

    let status = match (backward_ok, forward_ok) {
        (true, true) => CompatibilityStatus::Bidirectional,
        (true, false) => CompatibilityStatus::Backward,
        (false, true) => CompatibilityStatus::Forward,
        (false, false) => CompatibilityStatus::Incompatible,
    };

    Ok(CompatibilityReport { status, failures })
}

fn compatibility_failure(
    direction: CompatibilityDirection,
    error: PlanError,
) -> CompatibilityFailure {
    let (path, reason) = match error {
        PlanError::Schema(error) => (
            "$".to_owned(),
            CompatibilityFailureReason::Schema {
                message: error.to_string(),
            },
        ),
        PlanError::UnknownWriterType { path, type_id } => (
            path,
            CompatibilityFailureReason::UnknownWriterType { type_id },
        ),
        PlanError::UnknownReaderType { path, type_id } => (
            path,
            CompatibilityFailureReason::UnknownReaderType { type_id },
        ),
        PlanError::UnboundTypeParameter { path, name } => (
            path,
            CompatibilityFailureReason::UnboundTypeParameter { name },
        ),
        PlanError::TypeMismatch {
            path,
            writer,
            reader,
        } => (
            path,
            CompatibilityFailureReason::TypeMismatch { writer, reader },
        ),
        PlanError::MissingReaderField { path, field } => (
            path,
            CompatibilityFailureReason::MissingReaderField { field },
        ),
        PlanError::Unsupported { path, reason } => {
            (path, CompatibilityFailureReason::Unsupported { reason })
        }
    };

    CompatibilityFailure {
        direction,
        path,
        reason,
    }
}
