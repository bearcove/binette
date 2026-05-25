use std::collections::{HashMap, HashSet};

use crate::error::SchemaError;
use crate::hash::{
    primitive_for_type_id, primitive_type_id, recursive_type_id_map, schema_type_id,
};
use crate::schema::{Schema, SchemaBundle, SchemaKind, TypeId, TypeRef, VariantPayload};

#[derive(Debug, Default, Clone)]
pub struct SchemaRegistry {
    schemas: HashMap<TypeId, Schema>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }

    pub fn contains(&self, type_id: TypeId) -> bool {
        primitive_for_type_id(type_id).is_some() || self.schemas.contains_key(&type_id)
    }

    pub fn get(&self, type_id: TypeId) -> Option<&Schema> {
        self.schemas.get(&type_id)
    }

    pub fn schemas(&self) -> impl Iterator<Item = &Schema> {
        self.schemas.values()
    }

    // r[impl binette.schema.registry+2]
    // r[impl binette.bundle.self-contained]
    pub fn validate_self_contained_bundle(&self, bundle: &SchemaBundle) -> Result<(), SchemaError> {
        VerifiedSchemaBatch::for_self_contained(&self.schemas, bundle).map(|_| ())
    }

    // r[impl binette.schema.registry.install]
    // r[impl binette.bundle.registry]
    pub fn install_bundle(&mut self, bundle: &SchemaBundle) -> Result<(), SchemaError> {
        let batch = VerifiedSchemaBatch::new(&self.schemas, &bundle.schemas)?;
        batch.validate_type_ref(&bundle.root, &[])?;
        for attachment in &bundle.attachments {
            if let Some(metadata_schema) = &attachment.metadata_schema {
                batch.validate_type_ref(metadata_schema, &[])?;
            }
        }

        for schema in batch.into_schemas() {
            self.schemas.insert(schema.id, schema);
        }

        Ok(())
    }
}

struct VerifiedSchemaBatch<'a> {
    existing: &'a HashMap<TypeId, Schema>,
    schemas: HashMap<TypeId, Schema>,
}

impl<'a> VerifiedSchemaBatch<'a> {
    fn new(
        existing: &'a HashMap<TypeId, Schema>,
        incoming: &[Schema],
    ) -> Result<Self, SchemaError> {
        let batch = Self::collect(existing, incoming)?;
        batch.validate_schemas()?;
        batch.verify_declared_ids()?;
        Ok(batch)
    }

    fn for_self_contained(
        existing: &'a HashMap<TypeId, Schema>,
        bundle: &SchemaBundle,
    ) -> Result<Self, SchemaError> {
        let batch = Self::collect(existing, &bundle.schemas)?;
        batch.validate_self_contained_bundle(bundle)?;
        batch.validate_schemas()?;
        batch.verify_declared_ids()?;
        Ok(batch)
    }

    fn collect(
        existing: &'a HashMap<TypeId, Schema>,
        incoming: &[Schema],
    ) -> Result<Self, SchemaError> {
        let mut schemas = HashMap::new();

        for schema in incoming {
            if let SchemaKind::Primitive(primitive) = schema.kind
                && schema.id == primitive_type_id(primitive)
            {
                continue;
            }

            if let Some(primitive) = primitive_for_type_id(schema.id) {
                return Err(SchemaError::SchemaIdReservedForPrimitive {
                    type_id: schema.id,
                    primitive,
                });
            }

            if let Some(installed) = existing.get(&schema.id) {
                if installed != schema {
                    return Err(SchemaError::DuplicateSchemaId { type_id: schema.id });
                }
                continue;
            }

            if let Some(previous) = schemas.get(&schema.id) {
                if previous != schema {
                    return Err(SchemaError::DuplicateSchemaId { type_id: schema.id });
                }
                continue;
            }

            schemas.insert(schema.id, schema.clone());
        }

        Ok(Self { existing, schemas })
    }

    fn into_schemas(self) -> Vec<Schema> {
        self.schemas.into_values().collect()
    }

    fn validate_self_contained_bundle(&self, bundle: &SchemaBundle) -> Result<(), SchemaError> {
        let mut visited = HashSet::new();
        self.validate_self_contained_type_ref(&bundle.root, &[], &mut visited)?;
        for attachment in &bundle.attachments {
            if let Some(metadata_schema) = &attachment.metadata_schema {
                self.validate_self_contained_type_ref(metadata_schema, &[], &mut visited)?;
            }
        }
        Ok(())
    }

    fn validate_schemas(&self) -> Result<(), SchemaError> {
        for schema in self.schemas.values() {
            self.validate_type_params(&schema.type_params)?;
            self.validate_kind(&schema.kind, &schema.type_params)?;
        }
        Ok(())
    }

    fn validate_type_params(&self, type_params: &[String]) -> Result<(), SchemaError> {
        let mut seen = HashSet::new();
        for type_param in type_params {
            if !seen.insert(type_param) {
                return Err(SchemaError::DuplicateTypeParameter {
                    name: type_param.clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_kind(&self, kind: &SchemaKind, scope: &[String]) -> Result<(), SchemaError> {
        match kind {
            SchemaKind::Primitive(_) | SchemaKind::Dynamic => Ok(()),
            SchemaKind::Struct { name, fields } => {
                self.validate_name(name)?;
                for field in fields {
                    self.validate_type_ref(&field.type_ref, scope)?;
                }
                Ok(())
            }
            SchemaKind::Enum { name, variants } => {
                self.validate_name(name)?;
                for variant in variants {
                    self.validate_variant_payload(&variant.payload, scope)?;
                }
                Ok(())
            }
            SchemaKind::Tuple { elements } => {
                if elements.is_empty() {
                    return Err(SchemaError::InvalidTupleArity);
                }
                for element in elements {
                    self.validate_type_ref(element, scope)?;
                }
                Ok(())
            }
            SchemaKind::List { element } | SchemaKind::Set { element } => {
                self.validate_type_ref(element, scope)
            }
            SchemaKind::Map { key, value } => {
                self.validate_type_ref(key, scope)?;
                self.validate_type_ref(value, scope)
            }
            SchemaKind::Array {
                element,
                dimensions,
            } => {
                if dimensions.is_empty() {
                    return Err(SchemaError::InvalidArrayRank);
                }
                self.validate_type_ref(element, scope)
            }
            SchemaKind::Option { element } => self.validate_type_ref(element, scope),
            // r[impl binette.schema.external]
            SchemaKind::External { kind, metadata: _ } => self.validate_name(kind),
        }
    }

    fn validate_variant_payload(
        &self,
        payload: &VariantPayload,
        scope: &[String],
    ) -> Result<(), SchemaError> {
        match payload {
            VariantPayload::Unit => Ok(()),
            VariantPayload::Newtype { type_ref } => self.validate_type_ref(type_ref, scope),
            VariantPayload::Tuple { elements } => {
                for element in elements {
                    self.validate_type_ref(element, scope)?;
                }
                Ok(())
            }
            VariantPayload::Struct { fields } => {
                for field in fields {
                    self.validate_type_ref(&field.type_ref, scope)?;
                }
                Ok(())
            }
        }
    }

    fn validate_name(&self, name: &str) -> Result<(), SchemaError> {
        if name.is_empty() {
            Err(SchemaError::EmptySchemaName)
        } else {
            Ok(())
        }
    }

    fn validate_type_ref(&self, type_ref: &TypeRef, scope: &[String]) -> Result<(), SchemaError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if primitive_for_type_id(*type_id).is_some() {
                    if !args.is_empty() {
                        return Err(SchemaError::TypeArgumentArity {
                            type_id: *type_id,
                            expected: 0,
                            actual: args.len(),
                        });
                    }
                } else {
                    let schema = self
                        .schema(*type_id)
                        .ok_or(SchemaError::UnknownTypeId { type_id: *type_id })?;
                    if schema.type_params.len() != args.len() {
                        return Err(SchemaError::TypeArgumentArity {
                            type_id: *type_id,
                            expected: schema.type_params.len(),
                            actual: args.len(),
                        });
                    }
                }

                for arg in args {
                    self.validate_type_ref(arg, scope)?;
                }

                Ok(())
            }
            TypeRef::Var { name } => {
                if scope.contains(name) {
                    Ok(())
                } else {
                    Err(SchemaError::UnknownTypeParameter { name: name.clone() })
                }
            }
        }
    }

    fn validate_self_contained_type_ref(
        &self,
        type_ref: &TypeRef,
        scope: &[String],
        visited: &mut HashSet<TypeId>,
    ) -> Result<(), SchemaError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if primitive_for_type_id(*type_id).is_some() {
                    if !args.is_empty() {
                        return Err(SchemaError::TypeArgumentArity {
                            type_id: *type_id,
                            expected: 0,
                            actual: args.len(),
                        });
                    }
                } else {
                    let schema = self
                        .schema(*type_id)
                        .ok_or(SchemaError::MissingBundleSchema { type_id: *type_id })?;
                    if schema.type_params.len() != args.len() {
                        return Err(SchemaError::TypeArgumentArity {
                            type_id: *type_id,
                            expected: schema.type_params.len(),
                            actual: args.len(),
                        });
                    }

                    if visited.insert(*type_id) {
                        self.validate_self_contained_kind(
                            &schema.kind,
                            &schema.type_params,
                            visited,
                        )?;
                    }
                }

                for arg in args {
                    self.validate_self_contained_type_ref(arg, scope, visited)?;
                }

                Ok(())
            }
            TypeRef::Var { name } => {
                if scope.contains(name) {
                    Ok(())
                } else {
                    Err(SchemaError::UnknownTypeParameter { name: name.clone() })
                }
            }
        }
    }

    fn validate_self_contained_kind(
        &self,
        kind: &SchemaKind,
        scope: &[String],
        visited: &mut HashSet<TypeId>,
    ) -> Result<(), SchemaError> {
        match kind {
            SchemaKind::Primitive(_) | SchemaKind::Dynamic | SchemaKind::External { .. } => Ok(()),
            SchemaKind::Struct { fields, .. } => {
                for field in fields {
                    self.validate_self_contained_type_ref(&field.type_ref, scope, visited)?;
                }
                Ok(())
            }
            SchemaKind::Enum { variants, .. } => {
                for variant in variants {
                    self.validate_self_contained_variant_payload(&variant.payload, scope, visited)?;
                }
                Ok(())
            }
            SchemaKind::Tuple { elements } => {
                for element in elements {
                    self.validate_self_contained_type_ref(element, scope, visited)?;
                }
                Ok(())
            }
            SchemaKind::List { element } | SchemaKind::Set { element } => {
                self.validate_self_contained_type_ref(element, scope, visited)
            }
            SchemaKind::Map { key, value } => {
                self.validate_self_contained_type_ref(key, scope, visited)?;
                self.validate_self_contained_type_ref(value, scope, visited)
            }
            SchemaKind::Array { element, .. } | SchemaKind::Option { element } => {
                self.validate_self_contained_type_ref(element, scope, visited)
            }
        }
    }

    fn validate_self_contained_variant_payload(
        &self,
        payload: &VariantPayload,
        scope: &[String],
        visited: &mut HashSet<TypeId>,
    ) -> Result<(), SchemaError> {
        match payload {
            VariantPayload::Unit => Ok(()),
            VariantPayload::Newtype { type_ref } => {
                self.validate_self_contained_type_ref(type_ref, scope, visited)
            }
            VariantPayload::Tuple { elements } => {
                for element in elements {
                    self.validate_self_contained_type_ref(element, scope, visited)?;
                }
                Ok(())
            }
            VariantPayload::Struct { fields } => {
                for field in fields {
                    self.validate_self_contained_type_ref(&field.type_ref, scope, visited)?;
                }
                Ok(())
            }
        }
    }

    fn schema(&self, type_id: TypeId) -> Option<&Schema> {
        self.schemas
            .get(&type_id)
            .or_else(|| self.existing.get(&type_id))
    }

    // r[impl binette.schema.registry.recursive]
    fn verify_declared_ids(&self) -> Result<(), SchemaError> {
        for component in self.strongly_connected_components() {
            if component.len() == 1 && !self.has_self_dependency(component[0]) {
                let schema = self
                    .schemas
                    .get(&component[0])
                    .expect("SCCs only contain batch schemas");
                let computed = schema_type_id(schema)?;
                if computed != schema.id {
                    return Err(SchemaError::SchemaIdMismatch {
                        declared: schema.id,
                        computed,
                    });
                }
            } else {
                let schemas = component
                    .iter()
                    .map(|type_id| {
                        self.schemas
                            .get(type_id)
                            .expect("SCCs only contain batch schemas")
                    })
                    .collect::<Vec<_>>();
                let computed = recursive_type_id_map(&schemas)?;
                for schema in schemas {
                    let computed = computed
                        .get(&schema.id)
                        .copied()
                        .expect("recursive hash map contains every group schema");
                    if computed != schema.id {
                        return Err(SchemaError::SchemaIdMismatch {
                            declared: schema.id,
                            computed,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn strongly_connected_components(&self) -> Vec<Vec<TypeId>> {
        let mut state = TarjanState::default();

        for type_id in self.schemas.keys().copied() {
            if !state.indices.contains_key(&type_id) {
                self.connect_component(type_id, &mut state);
            }
        }

        state.components
    }

    fn connect_component(&self, type_id: TypeId, state: &mut TarjanState) {
        let index = state.next_index;
        state.next_index += 1;
        state.indices.insert(type_id, index);
        state.lowlinks.insert(type_id, index);
        state.stack.push(type_id);
        state.on_stack.insert(type_id);

        let mut deps = Vec::new();
        self.collect_batch_deps(
            &self
                .schemas
                .get(&type_id)
                .expect("cycle walk only starts from batch schemas")
                .kind,
            &mut deps,
        );

        for dep in deps {
            if !state.indices.contains_key(&dep) {
                self.connect_component(dep, state);
                let dep_lowlink = state.lowlinks[&dep];
                let lowlink = state.lowlinks[&type_id];
                state.lowlinks.insert(type_id, lowlink.min(dep_lowlink));
            } else if state.on_stack.contains(&dep) {
                let dep_index = state.indices[&dep];
                let lowlink = state.lowlinks[&type_id];
                state.lowlinks.insert(type_id, lowlink.min(dep_index));
            }
        }

        if state.lowlinks[&type_id] == state.indices[&type_id] {
            let mut component = Vec::new();
            while let Some(member) = state.stack.pop() {
                state.on_stack.remove(&member);
                component.push(member);
                if member == type_id {
                    break;
                }
            }
            state.components.push(component);
        }
    }

    fn has_self_dependency(&self, type_id: TypeId) -> bool {
        let mut deps = Vec::new();
        self.collect_batch_deps(
            &self
                .schemas
                .get(&type_id)
                .expect("SCCs only contain batch schemas")
                .kind,
            &mut deps,
        );
        deps.contains(&type_id)
    }

    fn collect_batch_deps(&self, kind: &SchemaKind, out: &mut Vec<TypeId>) {
        match kind {
            SchemaKind::Primitive(_) | SchemaKind::Dynamic | SchemaKind::External { .. } => {}
            SchemaKind::Struct { fields, .. } => {
                for field in fields {
                    self.collect_type_ref_batch_deps(&field.type_ref, out);
                }
            }
            SchemaKind::Enum { variants, .. } => {
                for variant in variants {
                    self.collect_variant_batch_deps(&variant.payload, out);
                }
            }
            SchemaKind::Tuple { elements } => {
                for element in elements {
                    self.collect_type_ref_batch_deps(element, out);
                }
            }
            SchemaKind::List { element } | SchemaKind::Set { element } => {
                self.collect_type_ref_batch_deps(element, out);
            }
            SchemaKind::Map { key, value } => {
                self.collect_type_ref_batch_deps(key, out);
                self.collect_type_ref_batch_deps(value, out);
            }
            SchemaKind::Array { element, .. } | SchemaKind::Option { element } => {
                self.collect_type_ref_batch_deps(element, out);
            }
        }
    }

    fn collect_variant_batch_deps(&self, payload: &VariantPayload, out: &mut Vec<TypeId>) {
        match payload {
            VariantPayload::Unit => {}
            VariantPayload::Newtype { type_ref } => self.collect_type_ref_batch_deps(type_ref, out),
            VariantPayload::Tuple { elements } => {
                for element in elements {
                    self.collect_type_ref_batch_deps(element, out);
                }
            }
            VariantPayload::Struct { fields } => {
                for field in fields {
                    self.collect_type_ref_batch_deps(&field.type_ref, out);
                }
            }
        }
    }

    fn collect_type_ref_batch_deps(&self, type_ref: &TypeRef, out: &mut Vec<TypeId>) {
        match type_ref {
            TypeRef::Concrete { type_id, args } => {
                if self.schemas.contains_key(type_id) {
                    out.push(*type_id);
                }
                for arg in args {
                    self.collect_type_ref_batch_deps(arg, out);
                }
            }
            TypeRef::Var { .. } => {}
        }
    }
}

#[derive(Default)]
struct TarjanState {
    next_index: usize,
    indices: HashMap<TypeId, usize>,
    lowlinks: HashMap<TypeId, usize>,
    stack: Vec<TypeId>,
    on_stack: HashSet<TypeId>,
    components: Vec<Vec<TypeId>>,
}
