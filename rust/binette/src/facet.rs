use std::collections::HashMap;

use facet_core::{Def, Facet, ScalarType, Shape, StructKind, Type, UserType};

use crate::error::SchemaError;
use crate::hash::{primitive_type_id, recursive_type_id_map, type_id_for_kind};
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
    let (schemas, root) = finalize_extracted_schemas(ctx.schemas, root)?;
    Ok(SchemaBundle {
        schemas,
        root,
        attachments: Vec::new(),
    })
}

#[derive(Default)]
struct ExtractCtx {
    schemas: Vec<Schema>,
    emitted_by_user_decl: HashMap<facet_core::DeclId, TypeId>,
    next_provisional: u64,
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
            let type_id = self.provisional_type_id();
            self.emitted_by_user_decl.insert(shape.decl_id, type_id);

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

            self.schemas.push(Schema {
                id: type_id,
                type_params,
                kind,
            });
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
            // r[impl binette.schema.tuple]
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
        let id = self.provisional_type_id();
        self.schemas.push(Schema {
            id,
            type_params: Vec::new(),
            kind,
        });
        Ok(TypeRef::concrete(id))
    }

    fn provisional_type_id(&mut self) -> TypeId {
        self.next_provisional += 1;
        TypeId(0xB1AE_7700_0000_0000 | self.next_provisional)
    }
}

fn finalize_extracted_schemas(
    mut schemas: Vec<Schema>,
    mut root: TypeRef,
) -> Result<(Vec<Schema>, TypeRef), SchemaError> {
    let components = schema_components(&schemas);
    let mut processed = vec![false; components.len()];

    for component_index in 0..components.len() {
        finalize_component(
            component_index,
            &components,
            &mut processed,
            &mut schemas,
            &mut root,
        )?;
    }

    Ok((dedupe_schemas(schemas)?, root))
}

fn finalize_component(
    component_index: usize,
    components: &[SchemaComponent],
    processed: &mut [bool],
    schemas: &mut [Schema],
    root: &mut TypeRef,
) -> Result<(), SchemaError> {
    if processed[component_index] {
        return Ok(());
    }

    for dependency in &components[component_index].dependencies {
        finalize_component(*dependency, components, processed, schemas, root)?;
    }

    let replacements = if components[component_index].recursive {
        let group = components[component_index]
            .schemas
            .iter()
            .map(|index| &schemas[*index])
            .collect::<Vec<_>>();
        recursive_type_id_map(&group)?
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        components[component_index]
            .schemas
            .iter()
            .map(|index| {
                let schema = &schemas[*index];
                Ok((
                    schema.id,
                    type_id_for_kind(&schema.kind, &schema.type_params)?,
                ))
            })
            .collect::<Result<Vec<_>, SchemaError>>()?
    };

    rewrite_all_type_ids(schemas, root, &replacements);
    processed[component_index] = true;
    Ok(())
}

#[derive(Debug)]
struct SchemaComponent {
    schemas: Vec<usize>,
    dependencies: Vec<usize>,
    recursive: bool,
}

fn schema_components(schemas: &[Schema]) -> Vec<SchemaComponent> {
    let graph = schema_dependency_graph(schemas);
    let raw_components = strong_components(&graph);
    let mut component_for_schema = vec![0; schemas.len()];
    for (component_index, component) in raw_components.iter().enumerate() {
        for schema_index in component {
            component_for_schema[*schema_index] = component_index;
        }
    }

    raw_components
        .iter()
        .enumerate()
        .map(|(component_index, component)| {
            let mut dependencies = Vec::new();
            let mut self_dependency = false;
            for schema_index in component {
                for dependency in &graph[*schema_index] {
                    let dependency_component = component_for_schema[*dependency];
                    if dependency_component == component_index {
                        self_dependency = true;
                    } else if !dependencies.contains(&dependency_component) {
                        dependencies.push(dependency_component);
                    }
                }
            }
            SchemaComponent {
                schemas: component.clone(),
                dependencies,
                recursive: component.len() > 1 || self_dependency,
            }
        })
        .collect()
}

fn schema_dependency_graph(schemas: &[Schema]) -> Vec<Vec<usize>> {
    let index_by_id = schemas
        .iter()
        .enumerate()
        .map(|(index, schema)| (schema.id, index))
        .collect::<HashMap<_, _>>();
    schemas
        .iter()
        .map(|schema| {
            let mut deps = Vec::new();
            collect_kind_dependencies(&schema.kind, &index_by_id, &mut deps);
            deps
        })
        .collect()
}

fn collect_kind_dependencies(
    kind: &SchemaKind,
    index_by_id: &HashMap<TypeId, usize>,
    out: &mut Vec<usize>,
) {
    match kind {
        SchemaKind::Primitive(_) | SchemaKind::Dynamic | SchemaKind::External { .. } => {}
        SchemaKind::Struct { fields, .. } => {
            for field in fields {
                collect_type_ref_dependencies(&field.type_ref, index_by_id, out);
            }
        }
        SchemaKind::Enum { variants, .. } => {
            for variant in variants {
                collect_payload_dependencies(&variant.payload, index_by_id, out);
            }
        }
        SchemaKind::Tuple { elements } => {
            for element in elements {
                collect_type_ref_dependencies(element, index_by_id, out);
            }
        }
        SchemaKind::List { element }
        | SchemaKind::Set { element }
        | SchemaKind::Array { element, .. }
        | SchemaKind::Option { element } => {
            collect_type_ref_dependencies(element, index_by_id, out);
        }
        SchemaKind::Map { key, value } => {
            collect_type_ref_dependencies(key, index_by_id, out);
            collect_type_ref_dependencies(value, index_by_id, out);
        }
    }
}

fn collect_payload_dependencies(
    payload: &VariantPayload,
    index_by_id: &HashMap<TypeId, usize>,
    out: &mut Vec<usize>,
) {
    match payload {
        VariantPayload::Unit => {}
        VariantPayload::Newtype { type_ref } => {
            collect_type_ref_dependencies(type_ref, index_by_id, out);
        }
        VariantPayload::Tuple { elements } => {
            for element in elements {
                collect_type_ref_dependencies(element, index_by_id, out);
            }
        }
        VariantPayload::Struct { fields } => {
            for field in fields {
                collect_type_ref_dependencies(&field.type_ref, index_by_id, out);
            }
        }
    }
}

fn collect_type_ref_dependencies(
    type_ref: &TypeRef,
    index_by_id: &HashMap<TypeId, usize>,
    out: &mut Vec<usize>,
) {
    match type_ref {
        TypeRef::Concrete { type_id, args } => {
            if let Some(index) = index_by_id.get(type_id)
                && !out.contains(index)
            {
                out.push(*index);
            }
            for arg in args {
                collect_type_ref_dependencies(arg, index_by_id, out);
            }
        }
        TypeRef::Var { .. } => {}
    }
}

fn strong_components(graph: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut tarjan = Tarjan::new(graph.len());
    for index in 0..graph.len() {
        if tarjan.indices[index].is_none() {
            tarjan.connect(index, graph);
        }
    }
    tarjan.components
}

struct Tarjan {
    next_index: usize,
    indices: Vec<Option<usize>>,
    lowlinks: Vec<usize>,
    stack: Vec<usize>,
    on_stack: Vec<bool>,
    components: Vec<Vec<usize>>,
}

impl Tarjan {
    fn new(len: usize) -> Self {
        Self {
            next_index: 0,
            indices: vec![None; len],
            lowlinks: vec![0; len],
            stack: Vec::new(),
            on_stack: vec![false; len],
            components: Vec::new(),
        }
    }

    fn connect(&mut self, node: usize, graph: &[Vec<usize>]) {
        let index = self.next_index;
        self.next_index += 1;
        self.indices[node] = Some(index);
        self.lowlinks[node] = index;
        self.stack.push(node);
        self.on_stack[node] = true;

        for dependency in &graph[node] {
            if self.indices[*dependency].is_none() {
                self.connect(*dependency, graph);
                self.lowlinks[node] = self.lowlinks[node].min(self.lowlinks[*dependency]);
            } else if self.on_stack[*dependency] {
                let dependency_index = self.indices[*dependency].expect("indexed stack node");
                self.lowlinks[node] = self.lowlinks[node].min(dependency_index);
            }
        }

        if self.lowlinks[node] == self.indices[node].expect("indexed node") {
            let mut component = Vec::new();
            loop {
                let member = self.stack.pop().expect("root node has stack members");
                self.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}

fn rewrite_all_type_ids(
    schemas: &mut [Schema],
    root: &mut TypeRef,
    replacements: &[(TypeId, TypeId)],
) {
    rewrite_type_ref_ids(root, replacements);
    for schema in schemas {
        if let Some((_, replacement)) = replacements
            .iter()
            .find(|(original, _)| *original == schema.id)
        {
            schema.id = *replacement;
        }
        rewrite_kind_type_ids(&mut schema.kind, replacements);
    }
}

fn rewrite_kind_type_ids(kind: &mut SchemaKind, replacements: &[(TypeId, TypeId)]) {
    match kind {
        SchemaKind::Primitive(_) | SchemaKind::Dynamic | SchemaKind::External { .. } => {}
        SchemaKind::Struct { fields, .. } => {
            for field in fields {
                rewrite_type_ref_ids(&mut field.type_ref, replacements);
            }
        }
        SchemaKind::Enum { variants, .. } => {
            for variant in variants {
                rewrite_payload_type_ids(&mut variant.payload, replacements);
            }
        }
        SchemaKind::Tuple { elements } => {
            for element in elements {
                rewrite_type_ref_ids(element, replacements);
            }
        }
        SchemaKind::List { element }
        | SchemaKind::Set { element }
        | SchemaKind::Array { element, .. }
        | SchemaKind::Option { element } => rewrite_type_ref_ids(element, replacements),
        SchemaKind::Map { key, value } => {
            rewrite_type_ref_ids(key, replacements);
            rewrite_type_ref_ids(value, replacements);
        }
    }
}

fn rewrite_payload_type_ids(payload: &mut VariantPayload, replacements: &[(TypeId, TypeId)]) {
    match payload {
        VariantPayload::Unit => {}
        VariantPayload::Newtype { type_ref } => rewrite_type_ref_ids(type_ref, replacements),
        VariantPayload::Tuple { elements } => {
            for element in elements {
                rewrite_type_ref_ids(element, replacements);
            }
        }
        VariantPayload::Struct { fields } => {
            for field in fields {
                rewrite_type_ref_ids(&mut field.type_ref, replacements);
            }
        }
    }
}

fn rewrite_type_ref_ids(type_ref: &mut TypeRef, replacements: &[(TypeId, TypeId)]) {
    match type_ref {
        TypeRef::Concrete { type_id, args } => {
            if let Some((_, replacement)) = replacements
                .iter()
                .find(|(original, _)| original == type_id)
            {
                *type_id = *replacement;
            }
            for arg in args {
                rewrite_type_ref_ids(arg, replacements);
            }
        }
        TypeRef::Var { .. } => {}
    }
}

fn dedupe_schemas(schemas: Vec<Schema>) -> Result<Vec<Schema>, SchemaError> {
    let mut unique = Vec::<Schema>::new();
    for schema in schemas {
        if let Some(existing) = unique.iter().find(|existing| existing.id == schema.id) {
            if existing != &schema {
                return Err(SchemaError::DuplicateSchemaId { type_id: schema.id });
            }
        } else {
            unique.push(schema);
        }
    }
    Ok(unique)
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
