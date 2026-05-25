use binette::{
    Field, Primitive, Schema, SchemaBundle, SchemaError, SchemaKind, SchemaRegistry, TypeId,
    TypeRef, VariantPayload, primitive_type_id, recursive_schema_type_ids, schema_bundle_for,
    schema_type_id,
};
use facet::Facet;

fn schema(bundle: &SchemaBundle, id: TypeId) -> &Schema {
    bundle
        .schemas
        .iter()
        .find(|schema| schema.id == id)
        .unwrap_or_else(|| panic!("schema {id:?} not found in {bundle:#?}"))
}

fn concrete_id(type_ref: &TypeRef) -> TypeId {
    match type_ref {
        TypeRef::Concrete { type_id, args } => {
            assert!(args.is_empty(), "unexpected type args in {type_ref:#?}");
            *type_id
        }
        TypeRef::Var { .. } => panic!("expected concrete type ref, got {type_ref:#?}"),
    }
}

fn rewrite_schema_type_ids(schema: &mut Schema, replacements: &[(TypeId, TypeId)]) {
    if let Some((_, replacement)) = replacements
        .iter()
        .find(|(original, _)| *original == schema.id)
    {
        schema.id = *replacement;
    }
    rewrite_kind_type_ids(&mut schema.kind, replacements);
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

// r[verify binette.schema.model]
// r[verify binette.schema.fields]
// r[verify binette.schema.name]
// r[verify binette.type-id]
// r[verify binette.hash.recursive.non-recursive]
// r[verify binette.type-id.hash]
// r[verify binette.type-id.hash.struct]
#[test]
fn facet_struct_shape_extracts_to_binette_schema() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        display_name: String,
        lucky: Option<u32>,
    }

    let bundle = schema_bundle_for::<Account>().unwrap();
    let root = concrete_id(&bundle.root);
    let account = schema(&bundle, root);

    let SchemaKind::Struct { name, fields } = &account.kind else {
        panic!("expected struct schema, got {account:#?}");
    };

    assert_eq!(name, "Account");
    assert_eq!(account.type_params, Vec::<String>::new());
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].name, "id");
    assert_eq!(
        fields[0].type_ref,
        TypeRef::concrete(primitive_type_id(Primitive::U64))
    );
    assert_eq!(fields[1].name, "display_name");
    assert_eq!(
        fields[1].type_ref,
        TypeRef::concrete(primitive_type_id(Primitive::String))
    );
    assert_eq!(fields[2].name, "lucky");

    let option_id = concrete_id(&fields[2].type_ref);
    let option = schema(&bundle, option_id);
    assert!(matches!(
        &option.kind,
        SchemaKind::Option { element }
            if *element == TypeRef::concrete(primitive_type_id(Primitive::U32))
    ));
}

// r[verify binette.schema.kinds]
// r[verify binette.schema.array]
// r[verify binette.type-id.context-free]
// r[verify binette.type-id.hash.container]
#[test]
fn facet_container_shapes_keep_distinct_binette_kinds() {
    #[derive(Facet)]
    struct Containers {
        names: Vec<String>,
        fixed: [u16; 4],
    }

    let bundle = schema_bundle_for::<Containers>().unwrap();
    let root = concrete_id(&bundle.root);
    let containers = schema(&bundle, root);
    let SchemaKind::Struct { fields, .. } = &containers.kind else {
        panic!("expected struct schema, got {containers:#?}");
    };

    let names_id = concrete_id(&fields[0].type_ref);
    let fixed_id = concrete_id(&fields[1].type_ref);
    assert_ne!(names_id, fixed_id);

    assert!(matches!(
        &schema(&bundle, names_id).kind,
        SchemaKind::List { element }
            if *element == TypeRef::concrete(primitive_type_id(Primitive::String))
    ));
    assert!(matches!(
        &schema(&bundle, fixed_id).kind,
        SchemaKind::Array { element, dimensions }
            if *element == TypeRef::concrete(primitive_type_id(Primitive::U16))
                && dimensions == &[4]
    ));
}

// r[verify binette.schema.tuple]
// r[verify binette.type-id.hash.tuple]
#[test]
fn facet_tuple_shape_extracts_non_empty_tuple_schema() {
    let bundle = schema_bundle_for::<(u16, String)>().unwrap();
    let tuple = schema(&bundle, concrete_id(&bundle.root));

    assert_eq!(
        tuple.kind,
        SchemaKind::Tuple {
            elements: vec![
                TypeRef::concrete(primitive_type_id(Primitive::U16)),
                TypeRef::concrete(primitive_type_id(Primitive::String)),
            ],
        }
    );
}

// r[verify binette.schema.fields]
// r[verify binette.type-id.hash.enum]
#[test]
fn facet_enum_shape_extracts_variant_payloads() {
    #[derive(Facet)]
    #[repr(u8)]
    #[allow(dead_code)]
    enum Event {
        Started,
        Renamed(String),
        Moved(u32, u32),
        Failed { code: u16, message: String },
    }

    let bundle = schema_bundle_for::<Event>().unwrap();
    let event = schema(&bundle, concrete_id(&bundle.root));
    let SchemaKind::Enum { name, variants } = &event.kind else {
        panic!("expected enum schema, got {event:#?}");
    };

    assert_eq!(name, "Event");
    assert_eq!(variants.len(), 4);
    assert_eq!(variants[0].name, "Started");
    assert_eq!(variants[0].index, 0);
    assert_eq!(variants[0].payload, VariantPayload::Unit);
    assert_eq!(variants[1].name, "Renamed");
    assert_eq!(
        variants[1].payload,
        VariantPayload::Newtype {
            type_ref: TypeRef::concrete(primitive_type_id(Primitive::String))
        }
    );
    assert_eq!(
        variants[2].payload,
        VariantPayload::Tuple {
            elements: vec![
                TypeRef::concrete(primitive_type_id(Primitive::U32)),
                TypeRef::concrete(primitive_type_id(Primitive::U32)),
            ]
        }
    );
    assert!(matches!(
        &variants[3].payload,
        VariantPayload::Struct { fields }
            if fields[0].name == "code"
                && fields[0].type_ref == TypeRef::concrete(primitive_type_id(Primitive::U16))
                && fields[1].name == "message"
                && fields[1].type_ref == TypeRef::concrete(primitive_type_id(Primitive::String))
    ));
}

// r[verify binette.schema.dynamic]
// r[verify binette.type-id.hash.dynamic]
#[test]
fn facet_value_shape_extracts_as_dynamic_value_schema() {
    #[derive(Facet)]
    struct DynamicHolder {
        value: facet_value::Value,
    }

    let bundle = schema_bundle_for::<DynamicHolder>().unwrap();
    let holder = schema(&bundle, concrete_id(&bundle.root));
    let SchemaKind::Struct { fields, .. } = &holder.kind else {
        panic!("expected struct schema, got {holder:#?}");
    };

    let dynamic = schema(&bundle, concrete_id(&fields[0].type_ref));
    assert_eq!(dynamic.kind, SchemaKind::Dynamic);
}

// r[verify binette.schema.type-ref]
// r[verify binette.type-id.hash.typeref]
#[test]
fn facet_generic_shape_extracts_declaration_schema_and_root_args() {
    #[derive(Facet)]
    struct Wrapper<T> {
        value: T,
    }

    let u32_bundle = schema_bundle_for::<Wrapper<u32>>().unwrap();
    let string_bundle = schema_bundle_for::<Wrapper<String>>().unwrap();

    let TypeRef::Concrete {
        type_id: u32_decl,
        args: u32_args,
    } = &u32_bundle.root
    else {
        panic!("unexpected root {:#?}", u32_bundle.root);
    };
    let TypeRef::Concrete {
        type_id: string_decl,
        args: string_args,
    } = &string_bundle.root
    else {
        panic!("unexpected root {:#?}", string_bundle.root);
    };

    assert_eq!(u32_decl, string_decl);
    assert_eq!(
        u32_args,
        &[TypeRef::concrete(primitive_type_id(Primitive::U32))]
    );
    assert_eq!(
        string_args,
        &[TypeRef::concrete(primitive_type_id(Primitive::String))]
    );

    let wrapper = schema(&u32_bundle, *u32_decl);
    assert_eq!(wrapper.type_params, ["T"]);
    assert!(matches!(
        &wrapper.kind,
        SchemaKind::Struct { fields, .. }
            if fields[0].type_ref == TypeRef::Var { name: "T".to_owned() }
    ));
}

// r[verify binette.type-id.context-free]
#[test]
fn transparent_facet_wrapper_uses_inner_type_identity() {
    #[derive(Facet)]
    #[repr(transparent)]
    struct UserId(String);

    let bundle = schema_bundle_for::<UserId>().unwrap();
    assert_eq!(
        bundle.root,
        TypeRef::concrete(primitive_type_id(Primitive::String))
    );
    assert!(bundle.schemas.is_empty());
}

// r[verify binette.schema.registry.install]
#[test]
fn registry_installs_bundle_after_verifying_declared_ids() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        lucky: Option<u32>,
    }

    let mut bundle = schema_bundle_for::<Account>().unwrap();
    bundle.schemas.reverse();

    let mut registry = SchemaRegistry::new();
    registry.install_bundle(&bundle).unwrap();

    assert!(registry.contains(concrete_id(&bundle.root)));
    assert_eq!(registry.len(), bundle.schemas.len());
}

// r[verify binette.hash.recursive]
// r[verify binette.schema.registry.recursive]
#[test]
fn registry_installs_self_recursive_schema_group() {
    let provisional = TypeId(1);
    let mut node = Schema {
        id: provisional,
        type_params: Vec::new(),
        kind: SchemaKind::Struct {
            name: "Node".to_owned(),
            fields: vec![Field {
                name: "next".to_owned(),
                type_ref: TypeRef::concrete(provisional),
            }],
        },
    };

    let final_id = recursive_schema_type_ids(&[node.clone()]).unwrap()[0];
    rewrite_schema_type_ids(&mut node, &[(provisional, final_id)]);

    let bundle = SchemaBundle {
        root: TypeRef::concrete(final_id),
        schemas: vec![node],
        attachments: Vec::new(),
    };
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(&bundle).unwrap();

    assert!(registry.contains(final_id));
    assert_eq!(registry.len(), 1);
}

// r[verify binette.hash.recursive]
// r[verify binette.schema.registry.recursive]
#[test]
fn recursive_hashing_is_stable_for_mutual_recursion() {
    let provisional_a = TypeId(1);
    let provisional_b = TypeId(2);
    let mut first = Schema {
        id: provisional_a,
        type_params: Vec::new(),
        kind: SchemaKind::Struct {
            name: "First".to_owned(),
            fields: vec![Field {
                name: "second".to_owned(),
                type_ref: TypeRef::concrete(provisional_b),
            }],
        },
    };
    let mut second = Schema {
        id: provisional_b,
        type_params: Vec::new(),
        kind: SchemaKind::Struct {
            name: "Second".to_owned(),
            fields: vec![Field {
                name: "first".to_owned(),
                type_ref: TypeRef::concrete(provisional_a),
            }],
        },
    };

    let forward = recursive_schema_type_ids(&[first.clone(), second.clone()]).unwrap();
    let reverse = recursive_schema_type_ids(&[second.clone(), first.clone()]).unwrap();
    assert_eq!(forward, vec![reverse[1], reverse[0]]);
    let replacements = [(provisional_a, forward[0]), (provisional_b, forward[1])];
    rewrite_schema_type_ids(&mut first, &replacements);
    rewrite_schema_type_ids(&mut second, &replacements);

    let bundle = SchemaBundle {
        root: TypeRef::concrete(forward[0]),
        schemas: vec![second, first],
        attachments: Vec::new(),
    };
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(&bundle).unwrap();

    assert!(registry.contains(forward[0]));
    assert!(registry.contains(forward[1]));
    assert_eq!(registry.len(), 2);
}

// r[verify binette.hash.recursive]
#[test]
fn recursive_hashing_deduplicates_identical_canonical_entries() {
    let mut left = Schema {
        id: TypeId(1),
        type_params: Vec::new(),
        kind: SchemaKind::Struct {
            name: "Node".to_owned(),
            fields: vec![Field {
                name: "next".to_owned(),
                type_ref: TypeRef::concrete(TypeId(1)),
            }],
        },
    };
    let mut right = Schema {
        id: TypeId(2),
        type_params: Vec::new(),
        kind: SchemaKind::Struct {
            name: "Node".to_owned(),
            fields: vec![Field {
                name: "next".to_owned(),
                type_ref: TypeRef::concrete(TypeId(2)),
            }],
        },
    };

    let final_ids = recursive_schema_type_ids(&[left.clone(), right.clone()]).unwrap();
    assert_eq!(final_ids[0], final_ids[1]);
    rewrite_schema_type_ids(&mut left, &[(TypeId(1), final_ids[0])]);
    rewrite_schema_type_ids(&mut right, &[(TypeId(2), final_ids[1])]);

    let bundle = SchemaBundle {
        root: TypeRef::concrete(final_ids[0]),
        schemas: vec![left, right],
        attachments: Vec::new(),
    };
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(&bundle).unwrap();

    assert_eq!(registry.len(), 1);
}

// r[verify binette.schema.registry.recursive]
#[test]
fn registry_rejects_recursive_schema_id_mismatch() {
    let provisional = TypeId(1);
    let mut node = Schema {
        id: provisional,
        type_params: Vec::new(),
        kind: SchemaKind::Struct {
            name: "Node".to_owned(),
            fields: vec![Field {
                name: "next".to_owned(),
                type_ref: TypeRef::concrete(provisional),
            }],
        },
    };

    let final_id = recursive_schema_type_ids(&[node.clone()]).unwrap()[0];
    rewrite_schema_type_ids(&mut node, &[(provisional, final_id)]);
    node.id = TypeId(0xDEAD);
    rewrite_kind_type_ids(&mut node.kind, &[(final_id, TypeId(0xDEAD))]);

    let bundle = SchemaBundle {
        root: TypeRef::concrete(TypeId(0xDEAD)),
        schemas: vec![node],
        attachments: Vec::new(),
    };
    let err = SchemaRegistry::new().install_bundle(&bundle).unwrap_err();
    assert!(matches!(
        err,
        SchemaError::SchemaIdMismatch {
            declared: TypeId(0xDEAD),
            computed,
        } if computed == final_id
    ));
}

// r[verify binette.schema.registry.install]
// r[verify binette.schema.primitive]
// r[verify binette.type-id.hash.primitives]
#[test]
fn registry_treats_primitive_schemas_as_builtin() {
    let primitive = Primitive::U8;
    let type_id = primitive_type_id(primitive);
    let bundle = SchemaBundle {
        schemas: vec![Schema {
            id: type_id,
            type_params: Vec::new(),
            kind: SchemaKind::Primitive(primitive),
        }],
        root: TypeRef::concrete(type_id),
        attachments: Vec::new(),
    };

    let mut registry = SchemaRegistry::new();
    registry.install_bundle(&bundle).unwrap();

    assert!(registry.contains(type_id));
    assert!(registry.get(type_id).is_none());
    assert!(registry.is_empty());
}

// r[verify binette.bundle.self-contained]
// r[verify binette.schema.registry+2]
#[test]
fn registry_validates_self_contained_bundles() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        lucky: Option<u32>,
    }

    let bundle = schema_bundle_for::<Account>().unwrap();
    SchemaRegistry::new()
        .validate_self_contained_bundle(&bundle)
        .unwrap();

    let account = schema(&bundle, concrete_id(&bundle.root));
    let SchemaKind::Struct { fields, .. } = &account.kind else {
        panic!("expected struct schema, got {account:#?}");
    };
    let option_id = concrete_id(&fields[1].type_ref);

    let mut missing_option = bundle.clone();
    missing_option
        .schemas
        .retain(|schema| schema.id != option_id);
    let err = SchemaRegistry::new()
        .validate_self_contained_bundle(&missing_option)
        .unwrap_err();
    assert!(matches!(
        err,
        SchemaError::MissingBundleSchema { type_id } if type_id == option_id
    ));

    let mut registry = SchemaRegistry::new();
    let option_schema = schema(&bundle, option_id).clone();
    registry
        .install_bundle(&SchemaBundle {
            schemas: vec![option_schema],
            root: TypeRef::concrete(option_id),
            attachments: Vec::new(),
        })
        .unwrap();
    registry
        .validate_self_contained_bundle(&missing_option)
        .unwrap();
}

// r[verify binette.schema.registry.install]
#[test]
fn registry_rejects_schema_id_mismatch() {
    #[derive(Facet)]
    struct Account {
        id: u64,
    }

    let mut bundle = schema_bundle_for::<Account>().unwrap();
    bundle.schemas[0].id = TypeId(0);

    let err = SchemaRegistry::new().install_bundle(&bundle).unwrap_err();
    assert!(matches!(
        err,
        SchemaError::SchemaIdMismatch {
            declared: TypeId(0),
            ..
        }
    ));
}

// r[verify binette.schema.registry.install]
#[test]
fn registry_rejects_unknown_type_references() {
    let missing = TypeId(0xABCD);
    let mut schema = Schema {
        id: TypeId(0),
        type_params: Vec::new(),
        kind: SchemaKind::Struct {
            name: "Dangling".to_owned(),
            fields: vec![Field {
                name: "missing".to_owned(),
                type_ref: TypeRef::concrete(missing),
            }],
        },
    };
    schema.id = schema_type_id(&schema).unwrap();
    let bundle = SchemaBundle {
        root: TypeRef::concrete(schema.id),
        schemas: vec![schema],
        attachments: Vec::new(),
    };

    let err = SchemaRegistry::new().install_bundle(&bundle).unwrap_err();
    assert!(matches!(
        err,
        SchemaError::UnknownTypeId { type_id } if type_id == missing
    ));
}

// r[verify binette.schema.registry.install]
#[test]
fn registry_rejects_root_type_argument_arity_mismatch() {
    #[derive(Facet)]
    struct Wrapper<T> {
        value: T,
    }

    let mut bundle = schema_bundle_for::<Wrapper<u32>>().unwrap();
    let TypeRef::Concrete { type_id, .. } = bundle.root else {
        panic!("unexpected root {:#?}", bundle.root);
    };
    bundle.root = TypeRef::concrete(type_id);

    let err = SchemaRegistry::new().install_bundle(&bundle).unwrap_err();
    assert!(matches!(
        err,
        SchemaError::TypeArgumentArity {
            type_id: found,
            expected: 1,
            actual: 0,
        } if found == type_id
    ));
}
