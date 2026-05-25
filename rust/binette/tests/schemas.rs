use binette::{
    Primitive, Schema, SchemaBundle, SchemaKind, TypeId, TypeRef, VariantPayload,
    primitive_type_id, schema_bundle_for,
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

// r[verify binette.schema.model]
// r[verify binette.schema.fields]
// r[verify binette.schema.name]
// r[verify binette.type-id]
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
