use binette::{
    CompatibilityDirection, CompatibilityFailureReason, CompatibilityStatus, EnumPayloadPlan,
    EnumVariantPlan, PlanError, PlanNode, SchemaBundle, SchemaRegistry, StructFieldPlan, TypeRef,
    compatibility_report, reader_plan_for, reader_plan_for_bundles, schema_bundle_for,
};
use facet::Facet;

fn registry_for(bundle: &SchemaBundle) -> SchemaRegistry {
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(bundle).unwrap();
    registry
}

// r[verify binette.compat.plan]
#[test]
fn same_reader_shape_plans_as_direct() {
    #[derive(Facet)]
    struct Account {
        id: u64,
        name: String,
    }

    let writer_bundle = schema_bundle_for::<Account>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let plan = reader_plan_for::<Account>(&writer_bundle.root, &writer_registry).unwrap();
    assert!(matches!(
        plan.root,
        PlanNode::Direct {
            writer: TypeRef::Concrete { .. },
            reader: TypeRef::Concrete { .. },
        }
    ));
}

// r[verify binette.compat.plan]
#[test]
fn bundle_roots_plan_without_local_reader_shape() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
            pub name: String,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub name: String,
            pub id: u64,
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Account>().unwrap();
    let reader_bundle = schema_bundle_for::<reader::Account>().unwrap();
    let plan = reader_plan_for_bundles(&writer_bundle, &reader_bundle).unwrap();

    let PlanNode::Struct { fields } = plan.root else {
        panic!("expected struct plan, got {:#?}", plan.root);
    };
    assert_eq!(fields.len(), 2);
}

// r[verify binette.compat.field-matching]
#[test]
fn struct_fields_are_planned_by_name_not_position() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
            pub name: String,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub name: String,
            pub id: u64,
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Account>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let plan = reader_plan_for::<reader::Account>(&writer_bundle.root, &writer_registry).unwrap();
    let PlanNode::Struct { fields } = plan.root else {
        panic!("expected struct plan, got {:#?}", plan.root);
    };

    assert_eq!(fields.len(), 2);
    assert!(matches!(
        &fields[0],
        StructFieldPlan::Read {
            writer_index: 0,
            reader_index: 1,
            name,
            plan,
        } if name == "id" && matches!(**plan, PlanNode::Direct { .. })
    ));
    assert!(matches!(
        &fields[1],
        StructFieldPlan::Read {
            writer_index: 1,
            reader_index: 0,
            name,
            plan,
        } if name == "name" && matches!(**plan, PlanNode::Direct { .. })
    ));
}

// r[verify binette.compat.skip-unknown]
#[test]
fn writer_only_struct_fields_become_skip_steps() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
            pub name: String,
            pub nickname: String,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
            pub name: String,
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Account>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let plan = reader_plan_for::<reader::Account>(&writer_bundle.root, &writer_registry).unwrap();
    let PlanNode::Struct { fields } = plan.root else {
        panic!("expected struct plan, got {:#?}", plan.root);
    };

    assert_eq!(fields.len(), 3);
    assert!(matches!(
        &fields[2],
        StructFieldPlan::Skip {
            writer_index: 2,
            name,
            writer_type: TypeRef::Concrete { .. },
        } if name == "nickname"
    ));
}

// r[verify binette.compat.fill-defaults]
#[test]
fn reader_only_struct_fields_fail_without_default_provider() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
            pub name: String,
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Account>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let err =
        reader_plan_for::<reader::Account>(&writer_bundle.root, &writer_registry).unwrap_err();
    assert!(matches!(
        err,
        PlanError::MissingReaderField { field, .. } if field == "name"
    ));
}

// r[verify binette.compat.type-compat]
// r[verify binette.compat.type-compat.basic]
#[test]
fn incompatible_field_type_fails_before_payload_decode() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: String,
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Account>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let err =
        reader_plan_for::<reader::Account>(&writer_bundle.root, &writer_registry).unwrap_err();
    assert!(matches!(
        err,
        PlanError::TypeMismatch { path, .. } if path == "$.id"
    ));
}

// r[verify binette.compat.report]
#[test]
fn compatibility_report_classifies_schema_snapshot_direction() {
    mod old {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
        }
    }

    mod added_required {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: u64,
            pub name: String,
        }
    }

    mod reordered {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub name: String,
            pub id: u64,
        }
    }

    mod changed {
        use facet::Facet;

        #[derive(Facet)]
        pub struct Account {
            pub id: String,
        }
    }

    let old = schema_bundle_for::<old::Account>().unwrap();
    let added_required = schema_bundle_for::<added_required::Account>().unwrap();
    let reordered = schema_bundle_for::<reordered::Account>().unwrap();
    let changed = schema_bundle_for::<changed::Account>().unwrap();

    let report = compatibility_report(&old, &added_required).unwrap();
    assert_eq!(report.status, CompatibilityStatus::Forward);
    assert_eq!(report.failures.len(), 1);
    assert!(matches!(
        &report.failures[0],
        failure if failure.direction == CompatibilityDirection::Backward
            && failure.path == "$"
            && matches!(
                &failure.reason,
                CompatibilityFailureReason::MissingReaderField { field } if field == "name"
            )
    ));

    let report = compatibility_report(&added_required, &old).unwrap();
    assert_eq!(report.status, CompatibilityStatus::Backward);

    let report = compatibility_report(&added_required, &reordered).unwrap();
    assert_eq!(report.status, CompatibilityStatus::Bidirectional);
    assert!(report.failures.is_empty());

    let report = compatibility_report(&old, &changed).unwrap();
    assert_eq!(report.status, CompatibilityStatus::Incompatible);
    assert_eq!(report.failures.len(), 2);
    assert!(report.failures.iter().all(|failure| {
        failure.path == "$.id"
            && matches!(
                failure.reason,
                CompatibilityFailureReason::TypeMismatch { .. }
            )
    }));
}

// r[verify binette.compat.tuple]
#[test]
fn tuple_arity_mismatch_fails_before_payload_decode() {
    let writer_bundle = schema_bundle_for::<(u16, u16)>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let err =
        reader_plan_for::<(u16, u16, u16)>(&writer_bundle.root, &writer_registry).unwrap_err();
    assert!(matches!(
        err,
        PlanError::Unsupported {
            path,
            reason: "tuple arity differs"
        } if path == "$"
    ));
}

// r[verify binette.compat.enum]
// r[verify binette.compat.enum.payload]
#[test]
fn enum_variants_are_planned_by_name_not_index() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Started,
            Moved(u32, u32),
            Failed { code: u16 },
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Moved(u32, u32),
            Started,
            Failed { code: u16 },
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Event>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let plan = reader_plan_for::<reader::Event>(&writer_bundle.root, &writer_registry).unwrap();
    let PlanNode::Enum { variants } = plan.root else {
        panic!("expected enum plan, got {:#?}", plan.root);
    };

    assert!(matches!(
        &variants[0],
        EnumVariantPlan::Read {
            writer_index: 0,
            reader_index: 1,
            name,
            payload: EnumPayloadPlan::Unit,
        } if name == "Started"
    ));
    assert!(matches!(
        &variants[1],
        EnumVariantPlan::Read {
            writer_index: 1,
            reader_index: 0,
            name,
            payload: EnumPayloadPlan::Tuple(elements),
        } if name == "Moved" && elements.len() == 2
    ));
}

// r[verify binette.compat.enum.missing-variant]
// r[verify binette.compat.enum.unknown-variant]
#[test]
fn writer_only_enum_variants_become_runtime_reject_steps() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Started,
            Failed { code: u16 },
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Started,
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Event>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let plan = reader_plan_for::<reader::Event>(&writer_bundle.root, &writer_registry).unwrap();
    let PlanNode::Enum { variants } = plan.root else {
        panic!("expected enum plan, got {:#?}", plan.root);
    };

    assert!(matches!(
        &variants[1],
        EnumVariantPlan::Reject {
            writer_index: 1,
            name,
        } if name == "Failed"
    ));
}

// r[verify binette.compat.enum.payload]
#[test]
fn enum_payload_mismatch_fails_before_payload_decode() {
    mod writer {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Moved(u32),
        }
    }

    mod reader {
        use facet::Facet;

        #[derive(Facet)]
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum Event {
            Moved(String),
        }
    }

    let writer_bundle = schema_bundle_for::<writer::Event>().unwrap();
    let writer_registry = registry_for(&writer_bundle);

    let err = reader_plan_for::<reader::Event>(&writer_bundle.root, &writer_registry).unwrap_err();
    assert!(matches!(
        err,
        PlanError::TypeMismatch { path, .. } if path == "$.Moved"
    ));
}
