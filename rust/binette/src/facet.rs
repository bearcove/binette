use std::collections::{HashMap, HashSet};

use facet_core::{Def, Facet, ScalarType, Shape, StructKind, Type, UserType};

use crate::error::SchemaError;
use crate::hash::{primitive_type_id, type_id_for_kind};
use crate::schema::{
    Field, Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef, Variant, VariantPayload,
};

// r[impl binette.schema.model]
// r[impl binette.bundle.model]
pub fn schema_bundle_for<T: Facet<'static>>() -> Result<SchemaBundle, SchemaError> {
    schema_bundle_for_shape(T::SHAPE)
}

// r[impl binette.schema.model]
// r[impl binette.bundle.model]
pub fn schema_bundle_for_shape(shape: &'static Shape) -> Result<SchemaBundle, SchemaError> {
    let mut ctx = ExtractCtx::default();
    let root = ctx.extract(shape)?;
    Ok(SchemaBundle {
        schemas: ctx.schemas,
        root,
        attachments: Vec::new(),
    })
}

#[derive(Default)]
struct ExtractCtx {
    schemas: Vec<Schema>,
    emitted_by_user_decl: HashMap<facet_core::DeclId, TypeId>,
    active_user_decls: HashSet<facet_core::DeclId>,
}

impl ExtractCtx {
    // r[impl binette.type-id.context-free]
    fn extract(&mut self, shape: &'static Shape) -> Result<TypeRef, SchemaError> {
        if shape.is_transparent()
            && let Some(inner) = shape.inner
        {
            return self.extract(inner);
        }

        if let Def::Pointer(pointer) = shape.def
            && let Some(pointee) = pointer.pointee
        {
            return self.extract(pointee);
        }

        if let Some(primitive) = primitive_for_scalar(shape)? {
            return Ok(TypeRef::concrete(primitive_type_id(primitive)));
        }

        match shape.def {
            Def::List(list) if scalar_primitive(list.t()) == Some(Primitive::U8) => {
                Ok(TypeRef::concrete(primitive_type_id(Primitive::Bytes)))
            }
            Def::Slice(slice) if scalar_primitive(slice.t()) == Some(Primitive::U8) => {
                Ok(TypeRef::concrete(primitive_type_id(Primitive::Bytes)))
            }
            Def::List(list) => {
                let element = self.extract(list.t())?;
                self.emit_anonymous(SchemaKind::List { element })
            }
            Def::Slice(slice) => {
                let element = self.extract(slice.t())?;
                self.emit_anonymous(SchemaKind::List { element })
            }
            Def::Set(set) => {
                let element = self.extract(set.t())?;
                self.emit_anonymous(SchemaKind::Set { element })
            }
            Def::Map(map) => {
                let key = self.extract(map.k())?;
                let value = self.extract(map.v())?;
                self.emit_anonymous(SchemaKind::Map { key, value })
            }
            Def::Array(array) => {
                let element = self.extract(array.t())?;
                self.emit_anonymous(SchemaKind::Array {
                    element,
                    dimensions: vec![array.n as u64],
                })
            }
            Def::Option(option) => {
                let element = self.extract(option.t())?;
                self.emit_anonymous(SchemaKind::Option { element })
            }
            Def::DynamicValue(_) => self.emit_anonymous(SchemaKind::Dynamic),
            _ => self.extract_user(shape),
        }
    }

    fn extract_user(&mut self, shape: &'static Shape) -> Result<TypeRef, SchemaError> {
        let Type::User(user_type) = shape.ty else {
            return Err(SchemaError::UnsupportedShape {
                type_name: shape.type_identifier,
                reason: "shape is neither a supported container nor a user type",
            });
        };

        let type_id = if let Some(type_id) = self.emitted_by_user_decl.get(&shape.decl_id) {
            *type_id
        } else {
            if !self.active_user_decls.insert(shape.decl_id) {
                return Err(SchemaError::RecursiveSchemaUnsupported {
                    type_name: shape.type_identifier,
                });
            }

            let type_params = type_param_names(shape);
            let param_map = type_param_map(shape);
            let kind = match user_type {
                UserType::Struct(struct_type) => {
                    self.struct_kind(shape, struct_type, &param_map)?
                }
                UserType::Enum(enum_type) => self.enum_kind(shape, enum_type, &param_map)?,
                UserType::Union(_) => {
                    return Err(SchemaError::UnsupportedShape {
                        type_name: shape.type_identifier,
                        reason: "unions are not compact-capable binette schemas",
                    });
                }
                UserType::Opaque => {
                    return Err(SchemaError::UnsupportedShape {
                        type_name: shape.type_identifier,
                        reason: "opaque user types are not compact-capable binette schemas",
                    });
                }
            };

            let type_id = type_id_for_kind(&kind, &type_params)?;
            self.schemas.push(Schema {
                id: type_id,
                type_params,
                kind,
            });
            self.emitted_by_user_decl.insert(shape.decl_id, type_id);
            self.active_user_decls.remove(&shape.decl_id);
            type_id
        };

        let args = self.extract_type_args(shape)?;
        Ok(if args.is_empty() {
            TypeRef::concrete(type_id)
        } else {
            TypeRef::generic(type_id, args)
        })
    }

    fn struct_kind(
        &mut self,
        shape: &'static Shape,
        struct_type: facet_core::StructType,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<SchemaKind, SchemaError> {
        match struct_type.kind {
            StructKind::Tuple => {
                let elements = struct_type
                    .fields
                    .iter()
                    .map(|field| self.type_ref_for_shape(field.shape(), param_map))
                    .collect::<Result<Vec<_>, _>>()?;
                if elements.is_empty() {
                    return Ok(SchemaKind::Primitive(Primitive::Unit));
                }
                Ok(SchemaKind::Tuple { elements })
            }
            StructKind::Unit | StructKind::Struct | StructKind::TupleStruct => {
                Ok(SchemaKind::Struct {
                    name: schema_name(shape),
                    fields: self.fields(struct_type.fields, param_map)?,
                })
            }
        }
    }

    fn enum_kind(
        &mut self,
        shape: &'static Shape,
        enum_type: facet_core::EnumType,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<SchemaKind, SchemaError> {
        let variants = enum_type
            .variants
            .iter()
            .enumerate()
            .map(|(index, variant)| {
                Ok(Variant {
                    name: variant.effective_name().to_owned(),
                    index: index as u32,
                    payload: self.variant_payload(variant.data, param_map)?,
                })
            })
            .collect::<Result<Vec<_>, SchemaError>>()?;

        Ok(SchemaKind::Enum {
            name: schema_name(shape),
            variants,
        })
    }

    fn variant_payload(
        &mut self,
        data: facet_core::StructType,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<VariantPayload, SchemaError> {
        match data.kind {
            StructKind::Unit => Ok(VariantPayload::Unit),
            StructKind::Tuple | StructKind::TupleStruct if data.fields.len() == 1 => {
                Ok(VariantPayload::Newtype {
                    type_ref: self.type_ref_for_shape(data.fields[0].shape(), param_map)?,
                })
            }
            StructKind::Tuple | StructKind::TupleStruct => Ok(VariantPayload::Tuple {
                elements: data
                    .fields
                    .iter()
                    .map(|field| self.type_ref_for_shape(field.shape(), param_map))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            StructKind::Struct => Ok(VariantPayload::Struct {
                fields: self.fields(data.fields, param_map)?,
            }),
        }
    }

    fn fields(
        &mut self,
        fields: &'static [facet_core::Field],
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<Vec<Field>, SchemaError> {
        fields
            .iter()
            .filter(|field| !field.should_skip_serializing_unconditional())
            .map(|field| {
                Ok(Field {
                    name: field.effective_name().to_owned(),
                    type_ref: self.type_ref_for_shape(field.shape(), param_map)?,
                })
            })
            .collect()
    }

    fn type_ref_for_shape(
        &mut self,
        shape: &'static Shape,
        param_map: &[(facet_core::ConstTypeId, String)],
    ) -> Result<TypeRef, SchemaError> {
        if let Some((_, name)) = param_map.iter().find(|(id, _)| *id == shape.id) {
            Ok(TypeRef::Var { name: name.clone() })
        } else {
            self.extract(shape)
        }
    }

    fn extract_type_args(&mut self, shape: &'static Shape) -> Result<Vec<TypeRef>, SchemaError> {
        shape
            .type_params
            .iter()
            .map(|param| self.extract(param.shape()))
            .collect()
    }

    fn emit_anonymous(&mut self, kind: SchemaKind) -> Result<TypeRef, SchemaError> {
        let id = type_id_for_kind(&kind, &[])?;
        if !self.schemas.iter().any(|schema| schema.id == id) {
            self.schemas.push(Schema {
                id,
                type_params: Vec::new(),
                kind,
            });
        }
        Ok(TypeRef::concrete(id))
    }
}

// r[impl binette.schema.name]
fn schema_name(shape: &'static Shape) -> String {
    shape.type_identifier.to_owned()
}

fn type_param_names(shape: &'static Shape) -> Vec<String> {
    shape
        .type_params
        .iter()
        .map(|param| param.name.to_owned())
        .collect()
}

fn type_param_map(shape: &'static Shape) -> Vec<(facet_core::ConstTypeId, String)> {
    shape
        .type_params
        .iter()
        .map(|param| (param.shape().id, param.name.to_owned()))
        .collect()
}

fn primitive_for_scalar(shape: &'static Shape) -> Result<Option<Primitive>, SchemaError> {
    match shape.scalar_type() {
        Some(scalar) => scalar_to_primitive(scalar, shape.type_identifier).map(Some),
        None => Ok(None),
    }
}

fn scalar_primitive(shape: &'static Shape) -> Option<Primitive> {
    shape
        .scalar_type()
        .and_then(|scalar| scalar_to_primitive(scalar, shape.type_identifier).ok())
}

fn scalar_to_primitive(
    scalar: ScalarType,
    type_name: &'static str,
) -> Result<Primitive, SchemaError> {
    match scalar {
        ScalarType::Unit => Ok(Primitive::Unit),
        ScalarType::Bool => Ok(Primitive::Bool),
        ScalarType::Char => Ok(Primitive::Char),
        ScalarType::Str | ScalarType::String | ScalarType::CowStr => Ok(Primitive::String),
        ScalarType::F32 => Ok(Primitive::F32),
        ScalarType::F64 => Ok(Primitive::F64),
        ScalarType::U8 => Ok(Primitive::U8),
        ScalarType::U16 => Ok(Primitive::U16),
        ScalarType::U32 => Ok(Primitive::U32),
        ScalarType::U64 => Ok(Primitive::U64),
        ScalarType::U128 => Ok(Primitive::U128),
        ScalarType::I8 => Ok(Primitive::I8),
        ScalarType::I16 => Ok(Primitive::I16),
        ScalarType::I32 => Ok(Primitive::I32),
        ScalarType::I64 => Ok(Primitive::I64),
        ScalarType::I128 => Ok(Primitive::I128),
        other => Err(SchemaError::UnsupportedScalar {
            scalar: other,
            type_name,
        }),
    }
}
