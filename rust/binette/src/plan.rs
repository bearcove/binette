use std::collections::HashMap;

use facet_core::{Facet, Shape};
use thiserror::Error;

use crate::error::SchemaError;
use crate::facet::schema_bundle_for_shape;
use crate::hash::primitive_for_type_id;
use crate::registry::SchemaRegistry;
use crate::schema::{Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderPlan {
    pub root: PlanNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanNode {
    Primitive {
        primitive: Primitive,
    },
    Struct {
        fields: Vec<StructFieldPlan>,
    },
    Tuple {
        elements: Vec<PlanNode>,
    },
    List {
        element: Box<PlanNode>,
    },
    Set {
        element: Box<PlanNode>,
    },
    Map {
        key: Box<PlanNode>,
        value: Box<PlanNode>,
    },
    Array {
        dimensions: Vec<u64>,
        element: Box<PlanNode>,
    },
    Enum {
        variants: Vec<EnumVariantPlan>,
    },
    Option {
        element: Box<PlanNode>,
    },
    Dynamic,
    External {
        kind: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructFieldPlan {
    Read {
        writer_index: usize,
        reader_index: usize,
        name: String,
        plan: Box<PlanNode>,
    },
    Skip {
        writer_index: usize,
        name: String,
        writer_type: TypeRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnumVariantPlan {
    Read {
        writer_index: u32,
        reader_index: usize,
        name: String,
        payload: EnumPayloadPlan,
    },
    Reject {
        writer_index: u32,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnumPayloadPlan {
    Unit,
    Newtype(Box<PlanNode>),
    Tuple(Vec<PlanNode>),
    Struct(Vec<StructFieldPlan>),
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error(transparent)]
    Schema(#[from] SchemaError),

    #[error("unknown writer type id {type_id:?} at {path}")]
    UnknownWriterType { path: String, type_id: TypeId },

    #[error("unknown reader type id {type_id:?} at {path}")]
    UnknownReaderType { path: String, type_id: TypeId },

    #[error("unbound type parameter {name} at {path}")]
    UnboundTypeParameter { path: String, name: String },

    #[error("incompatible types at {path}: writer {writer:?}, reader {reader:?}")]
    TypeMismatch {
        path: String,
        writer: TypeRef,
        reader: TypeRef,
    },

    #[error("reader field {field} is missing from writer struct at {path}")]
    MissingReaderField { path: String, field: String },

    #[error("unsupported translation plan at {path}: {reason}")]
    Unsupported { path: String, reason: &'static str },
}

// r[impl binette.compat.plan]
pub fn reader_plan_for<T: Facet<'static>>(
    writer_root: &TypeRef,
    writer_registry: &SchemaRegistry,
) -> Result<ReaderPlan, PlanError> {
    reader_plan_for_shape(writer_root, writer_registry, T::SHAPE)
}

// r[impl binette.compat.plan]
pub fn reader_plan_for_shape(
    writer_root: &TypeRef,
    writer_registry: &SchemaRegistry,
    reader_shape: &'static Shape,
) -> Result<ReaderPlan, PlanError> {
    let reader_bundle = schema_bundle_for_shape(reader_shape)?;
    let mut reader_registry = SchemaRegistry::new();
    reader_registry.install_bundle(&reader_bundle)?;

    let mut builder = PlanBuilder {
        writer_registry,
        reader_registry: &reader_registry,
    };
    let root = builder.plan_type(
        writer_root,
        &Env::default(),
        &reader_bundle.root,
        &Env::default(),
        "$",
    )?;
    Ok(ReaderPlan { root })
}

// r[impl binette.compat.plan]
pub fn reader_plan_for_bundle(
    writer_root: &TypeRef,
    writer_registry: &SchemaRegistry,
    reader_root: &TypeRef,
    reader_registry: &SchemaRegistry,
) -> Result<ReaderPlan, PlanError> {
    let mut builder = PlanBuilder {
        writer_registry,
        reader_registry,
    };
    let root = builder.plan_type(
        writer_root,
        &Env::default(),
        reader_root,
        &Env::default(),
        "$",
    )?;
    Ok(ReaderPlan { root })
}

// r[impl binette.compat.plan]
pub fn reader_plan_for_bundles(
    writer_bundle: &SchemaBundle,
    reader_bundle: &SchemaBundle,
) -> Result<ReaderPlan, PlanError> {
    let mut writer_registry = SchemaRegistry::new();
    writer_registry.install_bundle(writer_bundle)?;
    let mut reader_registry = SchemaRegistry::new();
    reader_registry.install_bundle(reader_bundle)?;
    reader_plan_for_bundle(
        &writer_bundle.root,
        &writer_registry,
        &reader_bundle.root,
        &reader_registry,
    )
}

struct PlanBuilder<'a> {
    writer_registry: &'a SchemaRegistry,
    reader_registry: &'a SchemaRegistry,
}

#[derive(Default)]
struct Env {
    bindings: HashMap<String, TypeRef>,
}

impl Env {
    fn bind(schema: &Schema, args: &[TypeRef]) -> Self {
        Self {
            bindings: schema
                .type_params
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect(),
        }
    }

    fn get(&self, name: &str) -> Option<&TypeRef> {
        self.bindings.get(name)
    }
}

enum ResolvedKind<'a> {
    Primitive(Primitive),
    Schema { schema: &'a Schema, env: Env },
}

struct SchemaPlanInput<'a> {
    type_ref: TypeRef,
    schema: &'a Schema,
    env: Env,
}

impl PlanBuilder<'_> {
    // r[impl binette.compat.type-compat]
    // r[impl binette.compat.type-compat.basic]
    fn plan_type(
        &mut self,
        writer_ref: &TypeRef,
        writer_env: &Env,
        reader_ref: &TypeRef,
        reader_env: &Env,
        path: &str,
    ) -> Result<PlanNode, PlanError> {
        let writer_ref = self.resolve_type_ref(writer_ref, writer_env, path)?;
        let reader_ref = self.resolve_type_ref(reader_ref, reader_env, path)?;

        let writer_kind = self.resolve_kind(
            &writer_ref,
            self.writer_registry,
            path,
            RegistrySide::Writer,
        )?;
        let reader_kind = self.resolve_kind(
            &reader_ref,
            self.reader_registry,
            path,
            RegistrySide::Reader,
        )?;

        match (writer_kind, reader_kind) {
            (ResolvedKind::Primitive(writer), ResolvedKind::Primitive(reader))
                if writer == reader =>
            {
                Ok(PlanNode::Primitive { primitive: writer })
            }
            (ResolvedKind::Primitive(_), ResolvedKind::Primitive(_)) => {
                Err(self.type_mismatch(path, writer_ref, reader_ref))
            }
            (
                ResolvedKind::Schema {
                    schema: writer_schema,
                    env: writer_env,
                },
                ResolvedKind::Schema {
                    schema: reader_schema,
                    env: reader_env,
                },
            ) => self.plan_schema_pair(
                SchemaPlanInput {
                    type_ref: writer_ref,
                    schema: writer_schema,
                    env: writer_env,
                },
                SchemaPlanInput {
                    type_ref: reader_ref,
                    schema: reader_schema,
                    env: reader_env,
                },
                path,
            ),
            _ => Err(self.type_mismatch(path, writer_ref, reader_ref)),
        }
    }

    fn plan_schema_pair(
        &mut self,
        writer_input: SchemaPlanInput<'_>,
        reader_input: SchemaPlanInput<'_>,
        path: &str,
    ) -> Result<PlanNode, PlanError> {
        match (&writer_input.schema.kind, &reader_input.schema.kind) {
            (
                SchemaKind::Struct {
                    fields: writer_fields,
                    ..
                },
                SchemaKind::Struct {
                    fields: reader_fields,
                    ..
                },
            ) => self.plan_struct(
                writer_fields,
                &writer_input.env,
                reader_fields,
                &reader_input.env,
                path,
            ),
            (SchemaKind::Tuple { elements: writer }, SchemaKind::Tuple { elements: reader }) => {
                self.plan_tuple(writer, &writer_input.env, reader, &reader_input.env, path)
            }
            (SchemaKind::List { element: writer }, SchemaKind::List { element: reader }) => {
                Ok(PlanNode::List {
                    element: Box::new(self.plan_type(
                        writer,
                        &writer_input.env,
                        reader,
                        &reader_input.env,
                        &format!("{path}[]"),
                    )?),
                })
            }
            (SchemaKind::Set { element: writer }, SchemaKind::Set { element: reader }) => {
                Ok(PlanNode::Set {
                    element: Box::new(self.plan_type(
                        writer,
                        &writer_input.env,
                        reader,
                        &reader_input.env,
                        &format!("{path}{{}}"),
                    )?),
                })
            }
            (
                SchemaKind::Map {
                    key: writer_key,
                    value: writer_value,
                },
                SchemaKind::Map {
                    key: reader_key,
                    value: reader_value,
                },
            ) => Ok(PlanNode::Map {
                key: Box::new(self.plan_type(
                    writer_key,
                    &writer_input.env,
                    reader_key,
                    &reader_input.env,
                    &format!("{path}.key"),
                )?),
                value: Box::new(self.plan_type(
                    writer_value,
                    &writer_input.env,
                    reader_value,
                    &reader_input.env,
                    &format!("{path}.value"),
                )?),
            }),
            (
                SchemaKind::Array {
                    element: writer,
                    dimensions: writer_dimensions,
                },
                SchemaKind::Array {
                    element: reader,
                    dimensions: reader_dimensions,
                },
            ) if writer_dimensions == reader_dimensions => Ok(PlanNode::Array {
                dimensions: writer_dimensions.clone(),
                element: Box::new(self.plan_type(
                    writer,
                    &writer_input.env,
                    reader,
                    &reader_input.env,
                    &format!("{path}[]"),
                )?),
            }),
            (SchemaKind::Option { element: writer }, SchemaKind::Option { element: reader }) => {
                Ok(PlanNode::Option {
                    element: Box::new(self.plan_type(
                        writer,
                        &writer_input.env,
                        reader,
                        &reader_input.env,
                        &format!("{path}?"),
                    )?),
                })
            }
            (SchemaKind::Dynamic, SchemaKind::Dynamic) => Ok(PlanNode::Dynamic),
            (
                SchemaKind::External {
                    kind: writer_kind,
                    metadata: writer_metadata,
                },
                SchemaKind::External {
                    kind: reader_kind,
                    metadata: reader_metadata,
                },
            ) if writer_kind == reader_kind && writer_metadata == reader_metadata => {
                Ok(PlanNode::External {
                    kind: writer_kind.clone(),
                })
            }
            (
                SchemaKind::Enum {
                    variants: writer, ..
                },
                SchemaKind::Enum {
                    variants: reader, ..
                },
            ) => self.plan_enum(writer, &writer_input.env, reader, &reader_input.env, path),
            _ => Err(self.type_mismatch(path, writer_input.type_ref, reader_input.type_ref)),
        }
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn plan_struct(
        &mut self,
        writer_fields: &[Field],
        writer_env: &Env,
        reader_fields: &[Field],
        reader_env: &Env,
        path: &str,
    ) -> Result<PlanNode, PlanError> {
        let reader_by_name = reader_fields
            .iter()
            .enumerate()
            .map(|(index, field)| (field.name.as_str(), (index, field)))
            .collect::<HashMap<_, _>>();

        let mut matched_readers = vec![false; reader_fields.len()];
        let mut fields = Vec::with_capacity(writer_fields.len());

        for (writer_index, writer_field) in writer_fields.iter().enumerate() {
            if let Some((reader_index, reader_field)) =
                reader_by_name.get(writer_field.name.as_str())
            {
                matched_readers[*reader_index] = true;
                fields.push(StructFieldPlan::Read {
                    writer_index,
                    reader_index: *reader_index,
                    name: writer_field.name.clone(),
                    plan: Box::new(self.plan_type(
                        &writer_field.type_ref,
                        writer_env,
                        &reader_field.type_ref,
                        reader_env,
                        &format!("{path}.{}", writer_field.name),
                    )?),
                });
            } else {
                fields.push(StructFieldPlan::Skip {
                    writer_index,
                    name: writer_field.name.clone(),
                    writer_type: self.resolve_type_ref(&writer_field.type_ref, writer_env, path)?,
                });
            }
        }

        for (reader_index, reader_field) in reader_fields.iter().enumerate() {
            if !matched_readers[reader_index] {
                // r[impl binette.compat.fill-defaults]
                return Err(PlanError::MissingReaderField {
                    path: path.to_owned(),
                    field: reader_field.name.clone(),
                });
            }
        }

        Ok(PlanNode::Struct { fields })
    }

    // r[impl binette.compat.enum]
    // r[impl binette.compat.enum.unknown-variant]
    // r[impl binette.compat.enum.missing-variant]
    // r[impl binette.compat.enum.payload]
    fn plan_enum(
        &mut self,
        writer_variants: &[crate::schema::Variant],
        writer_env: &Env,
        reader_variants: &[crate::schema::Variant],
        reader_env: &Env,
        path: &str,
    ) -> Result<PlanNode, PlanError> {
        let reader_by_name = reader_variants
            .iter()
            .enumerate()
            .map(|(index, variant)| (variant.name.as_str(), (index, variant)))
            .collect::<HashMap<_, _>>();

        let variants = writer_variants
            .iter()
            .map(|writer_variant| {
                if let Some((reader_index, reader_variant)) =
                    reader_by_name.get(writer_variant.name.as_str())
                {
                    Ok::<EnumVariantPlan, PlanError>(EnumVariantPlan::Read {
                        writer_index: writer_variant.index,
                        reader_index: *reader_index,
                        name: writer_variant.name.clone(),
                        payload: self.plan_variant_payload(
                            &writer_variant.payload,
                            writer_env,
                            &reader_variant.payload,
                            reader_env,
                            &format!("{path}.{}", writer_variant.name),
                        )?,
                    })
                } else {
                    Ok::<EnumVariantPlan, PlanError>(EnumVariantPlan::Reject {
                        writer_index: writer_variant.index,
                        name: writer_variant.name.clone(),
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PlanNode::Enum { variants })
    }

    fn plan_variant_payload(
        &mut self,
        writer_payload: &crate::schema::VariantPayload,
        writer_env: &Env,
        reader_payload: &crate::schema::VariantPayload,
        reader_env: &Env,
        path: &str,
    ) -> Result<EnumPayloadPlan, PlanError> {
        match (writer_payload, reader_payload) {
            (crate::schema::VariantPayload::Unit, crate::schema::VariantPayload::Unit) => {
                Ok(EnumPayloadPlan::Unit)
            }
            (
                crate::schema::VariantPayload::Newtype { type_ref: writer },
                crate::schema::VariantPayload::Newtype { type_ref: reader },
            ) => Ok(EnumPayloadPlan::Newtype(Box::new(
                self.plan_type(writer, writer_env, reader, reader_env, path)?,
            ))),
            (
                crate::schema::VariantPayload::Tuple { elements: writer },
                crate::schema::VariantPayload::Tuple { elements: reader },
            ) => Ok(EnumPayloadPlan::Tuple(self.plan_tuple_elements(
                writer, writer_env, reader, reader_env, path,
            )?)),
            (
                crate::schema::VariantPayload::Struct { fields: writer },
                crate::schema::VariantPayload::Struct { fields: reader },
            ) => {
                let PlanNode::Struct { fields } =
                    self.plan_struct(writer, writer_env, reader, reader_env, path)?
                else {
                    unreachable!("plan_struct always returns PlanNode::Struct");
                };
                Ok(EnumPayloadPlan::Struct(fields))
            }
            _ => Err(PlanError::Unsupported {
                path: path.to_owned(),
                reason: "enum variant payload kind differs",
            }),
        }
    }

    // r[impl binette.compat.tuple]
    fn plan_tuple(
        &mut self,
        writer: &[TypeRef],
        writer_env: &Env,
        reader: &[TypeRef],
        reader_env: &Env,
        path: &str,
    ) -> Result<PlanNode, PlanError> {
        let elements = self.plan_tuple_elements(writer, writer_env, reader, reader_env, path)?;

        Ok(PlanNode::Tuple { elements })
    }

    // r[impl binette.compat.tuple]
    fn plan_tuple_elements(
        &mut self,
        writer: &[TypeRef],
        writer_env: &Env,
        reader: &[TypeRef],
        reader_env: &Env,
        path: &str,
    ) -> Result<Vec<PlanNode>, PlanError> {
        if writer.len() != reader.len() {
            return Err(PlanError::Unsupported {
                path: path.to_owned(),
                reason: "tuple arity differs",
            });
        }

        writer
            .iter()
            .zip(reader)
            .enumerate()
            .map(|(index, (writer, reader))| {
                self.plan_type(
                    writer,
                    writer_env,
                    reader,
                    reader_env,
                    &format!("{path}.{index}"),
                )
            })
            .collect()
    }

    fn resolve_type_ref(
        &self,
        type_ref: &TypeRef,
        env: &Env,
        path: &str,
    ) -> Result<TypeRef, PlanError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => Ok(TypeRef::Concrete {
                type_id: *type_id,
                args: args
                    .iter()
                    .map(|arg| self.resolve_type_ref(arg, env, path))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            TypeRef::Var { name } => {
                env.get(name)
                    .cloned()
                    .ok_or_else(|| PlanError::UnboundTypeParameter {
                        path: path.to_owned(),
                        name: name.clone(),
                    })
            }
        }
    }

    fn resolve_kind<'a>(
        &self,
        type_ref: &TypeRef,
        registry: &'a SchemaRegistry,
        path: &str,
        side: RegistrySide,
    ) -> Result<ResolvedKind<'a>, PlanError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if let Some(primitive) = primitive_for_type_id(*type_id) {
                    if args.is_empty() {
                        return Ok(ResolvedKind::Primitive(primitive));
                    }
                    return Err(PlanError::Unsupported {
                        path: path.to_owned(),
                        reason: "primitive type reference has type arguments",
                    });
                }

                let schema = registry
                    .get(*type_id)
                    .ok_or_else(|| side.unknown(path, *type_id))?;
                Ok(ResolvedKind::Schema {
                    schema,
                    env: Env::bind(schema, args),
                })
            }
            TypeRef::Var { name } => Err(PlanError::UnboundTypeParameter {
                path: path.to_owned(),
                name: name.clone(),
            }),
        }
    }

    fn type_mismatch(&self, path: &str, writer: TypeRef, reader: TypeRef) -> PlanError {
        PlanError::TypeMismatch {
            path: path.to_owned(),
            writer,
            reader,
        }
    }
}

#[derive(Clone, Copy)]
enum RegistrySide {
    Writer,
    Reader,
}

impl RegistrySide {
    fn unknown(self, path: &str, type_id: TypeId) -> PlanError {
        match self {
            Self::Writer => PlanError::UnknownWriterType {
                path: path.to_owned(),
                type_id,
            },
            Self::Reader => PlanError::UnknownReaderType {
                path: path.to_owned(),
                type_id,
            },
        }
    }
}
