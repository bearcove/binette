use super::*;
use crate::hash::primitive_type_id;
use crate::local_access::{
    LocalAccess, LocalFieldDescriptor, LocalOptionRepresentation, LocalScalarAccess,
    LocalSchemaRef, LocalSequenceStorage, LocalTypeDescriptor, LocalTypeKind,
    rust_facet_descriptor_for_shape,
};
use crate::schema::TypeRef;

pub(super) struct StencilCompiler<'registry> {
    pub(super) writer_registry: &'registry SchemaRegistry,
    pub(super) plan_nodes: &'registry [PlanNode],
    pub(super) ops: Vec<StencilOp>,
    pub(super) failures: Vec<StencilFailure>,
    pub(super) input_offset: usize,
}

impl StencilCompiler<'_> {
    pub(super) fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &PlanNode,
    ) -> Result<LengthCheck, StencilError> {
        let reader = rust_facet_descriptor(T::SHAPE, "$")?;
        if let PlanNode::Enum { variants } = root {
            return self.compile_enum_root(T::SHAPE, &reader, variants, "$");
        }
        if let PlanNode::List { element } = root {
            return self.compile_list_root(T::SHAPE, &reader, element, "$");
        }

        self.compile_node(T::SHAPE, &reader, root, 0, "$")?;
        Ok(LengthCheck::Exact(self.input_offset))
    }

    // r[impl binette.aggregate.list]
    fn compile_list_root(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        element: &PlanNode,
        path: &str,
    ) -> Result<LengthCheck, StencilError> {
        if self.input_offset != 0 || !self.ops.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "list root stencil must be the first decode op",
            });
        }

        let Def::List(list) = reader_shape.def else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader list stencil requires Facet list shape",
            });
        };
        if list.init_in_place_with_capacity().is_none()
            || list.as_mut_ptr_typed().is_none()
            || list.set_len().is_none()
        {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader list shape does not expose direct-fill operations",
            });
        }

        let element_shape = list.t();
        let (element_descriptor, element_stride) = local_sequence_element(reader_descriptor, path)?;

        let mut element_compiler = StencilCompiler {
            writer_registry: self.writer_registry,
            plan_nodes: self.plan_nodes,
            ops: Vec::new(),
            failures: Vec::new(),
            input_offset: 0,
        };
        element_compiler.compile_node(
            element_shape,
            element_descriptor,
            element,
            0,
            &format!("{path}[]"),
        )?;
        if !element_compiler.failures.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "strict list decode currently supports only infallible element stencils",
            });
        }
        let Some(element_ops) = copy_ops_from_stencil_ops(&element_compiler.ops) else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "strict list decode currently supports only fixed-copy element stencils",
            });
        };
        let element_input_len = element_compiler.input_offset;

        let failure_index = self.failures.len();
        let _ = status_for_failure(failure_index)?;
        self.failures.push(StencilFailure::Helper {
            path: path.to_owned(),
        });
        self.ops.push(StencilOp::RootList {
            shape: reader_shape,
            element_ops,
            element_input_len,
            element_stride,
            failure_index,
        });

        Ok(LengthCheck::RootList {
            count_position: 0,
            element_input_len,
        })
    }

    fn compile_node(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        node: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        match node {
            PlanNode::Ref { node_index } => {
                let node =
                    self.plan_nodes
                        .get(*node_index)
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "recursive reader plan node reference is out of range",
                        })?;
                self.compile_node(reader_shape, reader_descriptor, node, output_offset, path)
            }
            PlanNode::Primitive { primitive } => {
                self.compile_primitive_read(*primitive, output_offset, path)
            }
            PlanNode::Struct { fields } => self.compile_struct_plan(
                reader_shape,
                reader_descriptor,
                fields,
                output_offset,
                path,
            ),
            PlanNode::Tuple { elements } => self.compile_tuple_plan(
                reader_shape,
                reader_descriptor,
                elements,
                output_offset,
                path,
            ),
            PlanNode::Array {
                dimensions,
                element,
            } => self.compile_array_plan(
                reader_shape,
                reader_descriptor,
                dimensions,
                element,
                output_offset,
                path,
            ),
            PlanNode::Enum { variants } if output_offset == 0 => self
                .compile_enum_root(reader_shape, reader_descriptor, variants, path)
                .map(|_| ()),
            PlanNode::External { .. } => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "stencil external attachment decode is not implemented yet",
            }),
            _ => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports scalar structs, tuples, and root enums",
            }),
        }
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct_plan(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        let descriptor_fields = local_struct_fields(reader_descriptor, path)?;
        self.compile_struct_fields_plan(
            reader_fields,
            descriptor_fields,
            fields,
            output_offset,
            path,
        )
    }

    fn compile_struct_fields_plan(
        &mut self,
        reader_fields: &'static [facet_core::Field],
        descriptor_fields: &[LocalFieldDescriptor],
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        for field in fields {
            match field {
                StructFieldPlan::Read {
                    reader_index,
                    name,
                    plan,
                    ..
                } => {
                    let reader_field = reader_fields.get(*reader_index).ok_or_else(|| {
                        StencilError::Unsupported {
                            path: format!("{path}.{name}"),
                            reason: "reader field index is out of range",
                        }
                    })?;
                    let field_descriptor =
                        descriptor_fields.get(*reader_index).ok_or_else(|| {
                            StencilError::Unsupported {
                                path: format!("{path}.{name}"),
                                reason: "reader descriptor field index is out of range",
                            }
                        })?;
                    let field_offset = checked_offset(
                        output_offset,
                        local_direct_offset(&field_descriptor.access, path)?,
                        path,
                    )?;
                    self.compile_read_plan(
                        plan,
                        reader_field.shape.get(),
                        &field_descriptor.descriptor,
                        field_offset,
                        &format!("{path}.{name}"),
                    )?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.compile_skip(writer_type, &format!("{path}.{name}"))?,
                StructFieldPlan::Default { name, .. } => {
                    return Err(StencilError::Unsupported {
                        path: format!("{path}.{name}"),
                        reason: "default-filled reader fields require interpreter decode",
                    });
                }
            }
        }
        Ok(())
    }

    fn compile_tuple_plan(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        elements: &[PlanNode],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        let descriptor_fields = local_struct_fields(reader_descriptor, path)?;
        if reader_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "stencil tuple element count differs from reader shape",
            });
        }
        for (index, element) in elements.iter().enumerate() {
            let reader_field = &reader_fields[index];
            let field_descriptor = &descriptor_fields[index];
            let field_offset = checked_offset(
                output_offset,
                local_direct_offset(&field_descriptor.access, path)?,
                path,
            )?;
            self.compile_read_plan(
                element,
                reader_field.shape.get(),
                &field_descriptor.descriptor,
                field_offset,
                &format!("{path}.{index}"),
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.array]
    fn compile_array_plan(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        dimensions: &[u64],
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (element_shape, count, stride) =
            fixed_array_parts(reader_shape, dimensions, path, "reader array")?;
        let (element_descriptor, descriptor_count, descriptor_stride) =
            local_inline_fixed_array(reader_descriptor, path)?;
        if descriptor_count != count || descriptor_stride != stride {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader array descriptor differs from Facet array layout",
            });
        }
        for index in 0..count {
            let element_output_offset =
                checked_offset(output_offset, checked_mul(index, stride, path)?, path)?;
            self.compile_read_plan(
                element,
                element_shape,
                element_descriptor,
                element_output_offset,
                &format!("{path}[{index}]"),
            )?;
        }
        Ok(())
    }

    // r[impl binette.compat.enum]
    // r[impl binette.compat.enum.payload]
    fn compile_enum_root(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        variants: &[EnumVariantPlan],
        path: &str,
    ) -> Result<LengthCheck, StencilError> {
        if self.input_offset != 0 || !self.ops.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports enums at the root",
            });
        }

        let enum_type = shape_enum_type(reader_shape, path)?;
        if enum_type.enum_repr != EnumRepr::U8 {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil enum backend requires repr(u8) reader enums",
            });
        }

        let input_offset = self.input_offset;
        self.input_offset = checked_offset(self.input_offset, 4, path)?;
        let unknown_failure_index = self.failures.len();
        self.failures.push(StencilFailure::UnknownVariantIndex {
            position: input_offset,
        });

        let mut cases = Vec::with_capacity(variants.len());
        let mut bodies = Vec::new();
        let mut lengths = Vec::new();

        for variant in variants {
            match variant {
                EnumVariantPlan::Read {
                    writer_index,
                    reader_index,
                    name,
                    payload,
                } => {
                    let reader_variant =
                        enum_type.variants.get(*reader_index).ok_or_else(|| {
                            StencilError::Unsupported {
                                path: format!("{path}.{name}"),
                                reason: "reader enum variant index is out of range",
                            }
                        })?;
                    let reader_discriminant =
                        enum_discriminant_u8(reader_variant, &format!("{path}.{name}"))?;
                    let local_variant = local_enum_variant(reader_descriptor, *reader_index, path)?;
                    let (body, expected) =
                        self.compile_branch_body(input_offset + 4, |compiler| {
                            compiler.compile_enum_payload_plan(
                                reader_variant.data.fields,
                                local_variant.payload.as_deref(),
                                payload,
                                &format!("{path}.{name}"),
                            )
                        })?;
                    let body_index = bodies.len();
                    bodies.push(body);
                    lengths.push(TaggedLength {
                        variant_index: *writer_index,
                        expected,
                    });
                    cases.push(EnumCase {
                        writer_index: *writer_index,
                        reader_discriminant: Some(reader_discriminant),
                        body_index: Some(body_index),
                        failure_index: None,
                    });
                }
                EnumVariantPlan::Reject { writer_index, name } => {
                    let failure_index = self.failures.len();
                    self.failures.push(StencilFailure::UnreadableWriterVariant {
                        position: input_offset,
                        variant_index: *writer_index,
                        variant: name.clone(),
                    });
                    cases.push(EnumCase {
                        writer_index: *writer_index,
                        reader_discriminant: None,
                        body_index: None,
                        failure_index: Some(failure_index),
                    });
                }
            }
        }

        self.ops.push(StencilOp::RootEnum {
            input_offset,
            cases,
            bodies,
            unknown_failure_index,
        });

        Ok(LengthCheck::RootU32Tag {
            position: input_offset,
            cases: lengths,
        })
    }

    fn compile_branch_body(
        &mut self,
        input_offset: usize,
        compile: impl FnOnce(&mut Self) -> Result<(), StencilError>,
    ) -> Result<(Vec<StencilOp>, usize), StencilError> {
        let parent_ops = std::mem::take(&mut self.ops);
        let parent_input_offset = self.input_offset;
        self.input_offset = input_offset;

        let result = compile(self);
        let body_ops = std::mem::take(&mut self.ops);
        let body_input_offset = self.input_offset;
        self.ops = parent_ops;
        self.input_offset = parent_input_offset;

        result?;
        Ok((body_ops, body_input_offset))
    }

    fn compile_enum_payload_plan(
        &mut self,
        reader_fields: &'static [facet_core::Field],
        payload_descriptor: Option<&LocalTypeDescriptor>,
        payload: &EnumPayloadPlan,
        path: &str,
    ) -> Result<(), StencilError> {
        match payload {
            EnumPayloadPlan::Unit => Ok(()),
            EnumPayloadPlan::Newtype(element) => {
                let reader_field =
                    reader_fields
                        .first()
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "newtype enum payload is missing reader field",
                        })?;
                let payload_descriptor =
                    payload_descriptor.ok_or_else(|| StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "newtype enum payload is missing local descriptor",
                    })?;
                self.compile_read_plan(
                    element,
                    reader_field.shape.get(),
                    payload_descriptor,
                    reader_field.offset,
                    path,
                )
            }
            EnumPayloadPlan::Tuple(elements) => {
                if reader_fields.len() != elements.len() {
                    return Err(StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "tuple enum payload arity differs from reader shape",
                    });
                }
                let descriptor_fields = local_struct_fields(
                    payload_descriptor.ok_or_else(|| StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "tuple enum payload is missing local descriptor",
                    })?,
                    path,
                )?;
                for (index, element) in elements.iter().enumerate() {
                    let reader_field = &reader_fields[index];
                    let field_descriptor = &descriptor_fields[index];
                    self.compile_read_plan(
                        element,
                        reader_field.shape.get(),
                        &field_descriptor.descriptor,
                        local_direct_offset(&field_descriptor.access, path)?,
                        &format!("{path}.{index}"),
                    )?;
                }
                Ok(())
            }
            EnumPayloadPlan::Struct(fields) => {
                let descriptor_fields = local_struct_fields(
                    payload_descriptor.ok_or_else(|| StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "struct enum payload is missing local descriptor",
                    })?,
                    path,
                )?;
                self.compile_struct_fields_plan(reader_fields, descriptor_fields, fields, 0, path)
            }
        }
    }

    fn compile_read_plan(
        &mut self,
        node: &PlanNode,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        self.compile_node(reader_shape, reader_descriptor, node, output_offset, path)
    }

    fn compile_skip(&mut self, type_ref: &TypeRef, path: &str) -> Result<(), StencilError> {
        self.compile_skip_type(type_ref, path)
    }

    fn compile_skip_type(&mut self, type_ref: &TypeRef, path: &str) -> Result<(), StencilError> {
        if let Some(primitive) = primitive_for_plain_type_ref(type_ref) {
            return self.compile_primitive_skip(primitive, path);
        }

        let kind = self.schema_for(type_ref, path)?.kind.clone();
        self.compile_skip_kind(&kind, path)
    }

    fn compile_skip_kind(&mut self, kind: &SchemaKind, path: &str) -> Result<(), StencilError> {
        match kind {
            SchemaKind::Primitive(primitive) => self.compile_primitive_skip(*primitive, path),
            SchemaKind::Struct { fields, .. } => {
                for field in fields {
                    self.compile_skip_type(&field.type_ref, &format!("{path}.{}", field.name))?;
                }
                Ok(())
            }
            SchemaKind::Tuple { elements } => {
                for (index, element) in elements.iter().enumerate() {
                    self.compile_skip_type(element, &format!("{path}.{index}"))?;
                }
                Ok(())
            }
            SchemaKind::Array {
                dimensions,
                element,
            } => {
                let count = dimensions_element_count(dimensions, path)?;
                for index in 0..count {
                    self.compile_skip_type(element, &format!("{path}[{index}]"))?;
                }
                Ok(())
            }
            _ => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend only supports scalar structs and tuples",
            }),
        }
    }

    fn compile_primitive_read(
        &mut self,
        primitive: Primitive,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        if primitive == Primitive::Bool {
            return self.emit_bool(path, Some(output_offset));
        }

        let Some(widths) = primitive_widths(primitive) else {
            return Err(unsupported_primitive(path, primitive));
        };
        self.emit_primitive_copies(path, output_offset, widths)
    }

    fn compile_primitive_skip(
        &mut self,
        primitive: Primitive,
        path: &str,
    ) -> Result<(), StencilError> {
        if primitive == Primitive::Bool {
            return self.emit_bool(path, None);
        }

        let Some(widths) = primitive_widths(primitive) else {
            return Err(unsupported_primitive(path, primitive));
        };
        for width in widths {
            self.input_offset = checked_offset(self.input_offset, width.bytes(), path)?;
        }
        Ok(())
    }

    // r[impl binette.scalar.unsigned]
    // r[impl binette.scalar.signed]
    // r[impl binette.scalar.float]
    fn emit_primitive_copies(
        &mut self,
        path: &str,
        output_offset: usize,
        widths: &'static [CopyWidth],
    ) -> Result<(), StencilError> {
        let mut output_offset = output_offset;
        for width in widths {
            self.ops.push(StencilOp::Copy(CopyOp {
                input_offset: self.input_offset,
                output_offset,
                width: *width,
            }));
            self.input_offset = checked_offset(self.input_offset, width.bytes(), path)?;
            output_offset = checked_offset(output_offset, width.bytes(), path)?;
        }
        Ok(())
    }

    // r[impl binette.scalar.bool]
    fn emit_bool(&mut self, path: &str, output_offset: Option<usize>) -> Result<(), StencilError> {
        let input_offset = self.input_offset;
        let failure_index = self.failures.len();
        self.failures.push(StencilFailure::InvalidBool {
            path: path.to_owned(),
            position: input_offset,
        });
        self.ops.push(StencilOp::Bool {
            input_offset,
            output_offset,
            failure_index,
        });
        self.input_offset = checked_offset(self.input_offset, 1, path)?;
        Ok(())
    }

    fn schema_for(
        &self,
        type_ref: &TypeRef,
        path: &str,
    ) -> Result<&crate::schema::Schema, StencilError> {
        match type_ref {
            TypeRef::Concrete { type_id, args } if args.is_empty() => self
                .writer_registry
                .get(*type_id)
                .ok_or_else(|| StencilError::UnknownWriterType {
                    path: path.to_owned(),
                    type_id: *type_id,
                }),
            TypeRef::Concrete { .. } | TypeRef::Var { .. } => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "first stencil backend does not support generic type refs",
            }),
        }
    }
}

pub(super) struct CursorStencilCompiler<'registry> {
    pub(super) writer_registry: &'registry SchemaRegistry,
    pub(super) plan_nodes: &'registry [PlanNode],
    pub(super) ops: Vec<HybridStencilOp>,
    pub(super) helpers: Vec<StencilHelper>,
    pub(super) failures: Vec<StencilFailure>,
    pub(super) allow_helpers: bool,
}

impl CursorStencilCompiler<'_> {
    pub(super) fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &PlanNode,
    ) -> Result<(), StencilError> {
        let reader = rust_facet_descriptor(T::SHAPE, "$")?;
        self.compile_node(T::SHAPE, &reader, root, 0, "$")
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        let descriptor_fields = local_struct_fields(reader_descriptor, path)?;
        for field in fields {
            match field {
                StructFieldPlan::Read {
                    reader_index,
                    name,
                    plan,
                    ..
                } => {
                    let reader_field = reader_fields.get(*reader_index).ok_or_else(|| {
                        StencilError::Unsupported {
                            path: format!("{path}.{name}"),
                            reason: "reader field index is out of range",
                        }
                    })?;
                    let field_descriptor =
                        descriptor_fields.get(*reader_index).ok_or_else(|| {
                            StencilError::Unsupported {
                                path: format!("{path}.{name}"),
                                reason: "reader descriptor field index is out of range",
                            }
                        })?;
                    let field_offset = checked_offset(
                        output_offset,
                        local_direct_offset(&field_descriptor.access, path)?,
                        path,
                    )?;
                    self.compile_node(
                        reader_field.shape.get(),
                        &field_descriptor.descriptor,
                        plan,
                        field_offset,
                        &format!("{path}.{name}"),
                    )?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.push_fixed_skip(writer_type, &format!("{path}.{name}"))?,
                StructFieldPlan::Default { name, .. } => {
                    return Err(StencilError::Unsupported {
                        path: format!("{path}.{name}"),
                        reason: "default-filled reader fields require interpreter encode",
                    });
                }
            }
        }
        Ok(())
    }

    fn compile_node(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        node: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let result = match node {
            PlanNode::Ref { node_index } => {
                let node =
                    self.plan_nodes
                        .get(*node_index)
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "recursive reader plan node reference is out of range",
                        })?;
                self.compile_node(reader_shape, reader_descriptor, node, output_offset, path)
            }
            PlanNode::List { element } => self.push_list(
                reader_shape,
                reader_descriptor,
                element,
                output_offset,
                path,
            ),
            PlanNode::Struct { fields } => {
                self.compile_struct(reader_shape, reader_descriptor, fields, output_offset, path)
            }
            PlanNode::Tuple { elements } => self.compile_tuple(
                reader_shape,
                reader_descriptor,
                elements,
                output_offset,
                path,
            ),
            PlanNode::Array {
                dimensions,
                element,
            } => self.compile_array(
                reader_shape,
                reader_descriptor,
                dimensions,
                element,
                output_offset,
                path,
            ),
            _ => self.push_fixed_copy(node, reader_shape, reader_descriptor, output_offset, path),
        };
        match result {
            Ok(()) => Ok(()),
            Err(StencilError::Unsupported { .. }) if self.allow_helpers => {
                self.push_decode_helper(node, reader_shape, output_offset, path)
            }
            Err(err) => Err(err),
        }
    }

    fn compile_tuple(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        elements: &[PlanNode],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        let descriptor_fields = local_struct_fields(reader_descriptor, path)?;
        if reader_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "cursor tuple element count differs from reader shape",
            });
        }
        for (index, element) in elements.iter().enumerate() {
            let reader_field = &reader_fields[index];
            let field_descriptor = &descriptor_fields[index];
            let element_offset = checked_offset(
                output_offset,
                local_direct_offset(&field_descriptor.access, path)?,
                path,
            )?;
            self.compile_node(
                reader_field.shape.get(),
                &field_descriptor.descriptor,
                element,
                element_offset,
                &format!("{path}.{index}"),
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.array]
    fn compile_array(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        dimensions: &[u64],
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (element_shape, count, stride) =
            fixed_array_parts(reader_shape, dimensions, path, "reader array")?;
        let (element_descriptor, descriptor_count, descriptor_stride) =
            local_inline_fixed_array(reader_descriptor, path)?;
        if descriptor_count != count || descriptor_stride != stride {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader array descriptor differs from Facet array layout",
            });
        }
        for index in 0..count {
            let element_offset =
                checked_offset(output_offset, checked_mul(index, stride, path)?, path)?;
            self.compile_node(
                element_shape,
                element_descriptor,
                element,
                element_offset,
                &format!("{path}[{index}]"),
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.list]
    fn push_list(
        &mut self,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let Def::List(list) = reader_shape.def else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader list stencil requires Facet list shape",
            });
        };
        if list.init_in_place_with_capacity().is_none()
            || list.as_mut_ptr_typed().is_none()
            || list.set_len().is_none()
        {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader list shape does not expose direct-fill operations",
            });
        }

        let element_shape = list.t();
        let (element_descriptor, element_stride) = local_sequence_element(reader_descriptor, path)?;
        let element_ops =
            self.compile_list_element(element_shape, element_descriptor, element, path)?;
        let failure_index = self.push_helper_failure(path)?;
        self.ops.push(HybridStencilOp::List {
            shape: reader_shape,
            output_offset,
            element_ops,
            element_stride,
            failure_index,
        });
        Ok(())
    }

    fn compile_list_element(
        &mut self,
        element_shape: &'static Shape,
        element_descriptor: &LocalTypeDescriptor,
        element: &PlanNode,
        path: &str,
    ) -> Result<Vec<HybridStencilOp>, StencilError> {
        let root_ops = std::mem::take(&mut self.ops);
        let helpers_len = self.helpers.len();
        let failures_len = self.failures.len();
        let element_result = self.compile_node(
            element_shape,
            element_descriptor,
            element,
            0,
            &format!("{path}[]"),
        );
        let element_ops = std::mem::replace(&mut self.ops, root_ops);
        if let Err(err) = element_result {
            self.helpers.truncate(helpers_len);
            self.failures.truncate(failures_len);
            return Err(err);
        }
        if element_ops
            .iter()
            .any(|op| matches!(op, HybridStencilOp::List { .. }))
        {
            self.helpers.truncate(helpers_len);
            self.failures.truncate(failures_len);
            return Err(StencilError::Unsupported {
                path: format!("{path}[]"),
                reason: "cursor list decode does not support nested native list loops yet",
            });
        }
        Ok(element_ops)
    }

    fn push_fixed_copy(
        &mut self,
        node: &PlanNode,
        reader_shape: &'static Shape,
        reader_descriptor: &LocalTypeDescriptor,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (ops, input_len) = fixed_copy_ops(
            self.writer_registry,
            self.plan_nodes,
            node,
            reader_shape,
            reader_descriptor,
            output_offset,
            path,
        )?;
        if input_len == 0 && ops.is_empty() {
            return Ok(());
        }
        let failure_index = self.push_helper_failure(path)?;
        self.ops.push(HybridStencilOp::Copy {
            ops,
            input_len,
            failure_index,
        });
        Ok(())
    }

    fn push_fixed_skip(&mut self, writer_type: &TypeRef, path: &str) -> Result<(), StencilError> {
        let input_len = match fixed_skip_len(self.writer_registry, writer_type, path) {
            Ok(input_len) => input_len,
            Err(StencilError::Unsupported { .. }) if self.allow_helpers => {
                return self.push_skip_helper(writer_type, path);
            }
            Err(err) => return Err(err),
        };
        if input_len == 0 {
            return Ok(());
        }
        let failure_index = self.push_helper_failure(path)?;
        self.ops.push(HybridStencilOp::Copy {
            ops: Vec::new(),
            input_len,
            failure_index,
        });
        Ok(())
    }

    fn push_decode_helper(
        &mut self,
        plan: &PlanNode,
        reader_shape: &'static Shape,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::Decode {
            plan: plan.clone(),
            plan_nodes: self.plan_nodes.to_vec(),
            reader_shape,
            output_offset,
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_skip_helper(&mut self, writer_type: &TypeRef, path: &str) -> Result<(), StencilError> {
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::Skip {
            writer_type: writer_type.clone(),
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_helper_failure(&mut self, path: &str) -> Result<usize, StencilError> {
        let failure_index = self.failures.len();
        let _ = status_for_failure(failure_index)?;
        self.failures.push(StencilFailure::Helper {
            path: path.to_owned(),
        });
        Ok(failure_index)
    }
}

pub(super) struct StencilEncodeCompiler {
    pub(super) ops: Vec<EncodeStencilOp>,
    pub(super) helpers: Vec<StencilEncodeHelper>,
    pub(super) failures: Vec<StencilFailure>,
}

#[derive(Clone, Copy)]
struct EncodeLocal<'a> {
    shape: &'static Shape,
    descriptor: &'a LocalTypeDescriptor,
}

#[derive(Clone, Copy)]
struct VariantElementLocal<'a> {
    descriptor: &'a LocalTypeDescriptor,
    access: Option<&'a LocalAccess>,
}

impl StencilEncodeCompiler {
    pub(super) fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &WriterNode,
    ) -> Result<(), StencilError> {
        let writer = rust_facet_descriptor(T::SHAPE, "$")?;
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        self.compile_node(T::SHAPE, &writer, root, 0, "$", &mut pending)?;
        self.flush_direct_segment(&mut pending);
        Ok(())
    }

    fn compile_node(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        node: &WriterNode,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        match fixed_encode_segment(
            shape,
            descriptor,
            node,
            input_offset,
            pending.output_len,
            path,
        ) {
            Ok(segment) if copy_ops_fit_direct_code(&segment.ops) => {
                pending.ops.extend(segment.ops);
                pending.output_len = checked_offset(pending.output_len, segment.output_len, path)?;
                return Ok(());
            }
            Ok(_) => {
                self.flush_direct_segment(pending);
                match fixed_encode_segment(shape, descriptor, node, input_offset, 0, path) {
                    Ok(segment) if copy_ops_fit_direct_code(&segment.ops) => {
                        self.push_direct_segment(segment);
                        return Ok(());
                    }
                    Ok(_) => {
                        self.push_node_helper(shape, node, input_offset, path)?;
                        return Ok(());
                    }
                    Err(StencilError::Unsupported { .. }) => {}
                    Err(err) => return Err(err),
                }
            }
            Err(StencilError::Unsupported { .. }) => {}
            Err(err) => return Err(err),
        }

        match node {
            WriterNode::Ref { .. } => {
                self.flush_direct_segment(pending);
                self.push_node_helper(shape, node, input_offset, path)
            }
            WriterNode::Primitive(Primitive::String) => {
                self.flush_direct_segment(pending);
                self.push_bytes(shape, input_offset, EncodeBytesKind::String);
                Ok(())
            }
            WriterNode::Primitive(Primitive::Bytes | Primitive::Payload) => {
                self.flush_direct_segment(pending);
                self.push_bytes(shape, input_offset, EncodeBytesKind::Bytes);
                Ok(())
            }
            WriterNode::Struct { fields } => {
                self.compile_struct_root(shape, descriptor, fields, input_offset, path, pending)
            }
            WriterNode::Tuple { elements } => {
                self.compile_tuple_root(shape, descriptor, elements, input_offset, path, pending)
            }
            WriterNode::Array {
                dimensions,
                element,
            } => self.compile_array_root(
                EncodeLocal { shape, descriptor },
                dimensions,
                element,
                input_offset,
                path,
                pending,
            ),
            WriterNode::Option { element } => {
                self.flush_direct_segment(pending);
                if self.push_option(shape, descriptor, input_offset, element, path)? {
                    Ok(())
                } else {
                    self.push_node_helper(shape, node, input_offset, path)
                }
            }
            WriterNode::List { element } => {
                self.flush_direct_segment(pending);
                if self.push_list(shape, descriptor, input_offset, element, path)? {
                    Ok(())
                } else {
                    self.push_node_helper(shape, node, input_offset, path)
                }
            }
            WriterNode::Enum { variants } => {
                self.flush_direct_segment(pending);
                self.push_enum(shape, descriptor, input_offset, variants, path)
            }
            _ => {
                self.flush_direct_segment(pending);
                self.push_node_helper(shape, node, input_offset, path)
            }
        }
    }

    // r[impl binette.aggregate.struct.compact]
    fn compile_struct_root(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        fields: &[WriterFieldPlan],
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        for field in fields {
            let field_path = format!("{path}.{}", field.name);
            let Some(facet_field) = facet_fields.get(field.facet_index) else {
                return Err(StencilError::Unsupported {
                    path: field_path,
                    reason: "writer field index is out of range",
                });
            };
            let field_descriptor = descriptor_fields.get(field.facet_index).ok_or_else(|| {
                StencilError::Unsupported {
                    path: field_path.clone(),
                    reason: "writer descriptor field index is out of range",
                }
            })?;
            let field_offset = checked_offset(
                input_offset,
                local_direct_offset(&field_descriptor.access, path)?,
                path,
            )?;
            self.compile_node(
                facet_field.shape.get(),
                &field_descriptor.descriptor,
                &field.node,
                field_offset,
                &field_path,
                pending,
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.tuple]
    fn compile_tuple_root(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        elements: &[WriterTupleElementPlan],
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        if facet_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer tuple arity differs from Facet shape",
            });
        }
        for element in elements {
            let element_path = format!("{path}.{}", element.facet_index);
            let Some(facet_field) = facet_fields.get(element.facet_index) else {
                return Err(StencilError::Unsupported {
                    path: element_path,
                    reason: "writer tuple field index is out of range",
                });
            };
            let element_descriptor =
                descriptor_fields.get(element.facet_index).ok_or_else(|| {
                    StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "writer tuple descriptor field index is out of range",
                    }
                })?;
            let element_offset = checked_offset(
                input_offset,
                local_direct_offset(&element_descriptor.access, path)?,
                path,
            )?;
            self.compile_node(
                facet_field.shape.get(),
                &element_descriptor.descriptor,
                &element.node,
                element_offset,
                &element_path,
                pending,
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.array]
    fn compile_array_root(
        &mut self,
        local: EncodeLocal<'_>,
        dimensions: &[u64],
        element: &WriterNode,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let (element_shape, count, stride) =
            fixed_array_parts(local.shape, dimensions, path, "writer array")?;
        let (element_descriptor, descriptor_count, descriptor_stride) =
            local_inline_fixed_array(local.descriptor, path)?;
        if descriptor_count != count || descriptor_stride != stride {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer array descriptor differs from Facet array layout",
            });
        }
        for index in 0..count {
            let element_input_offset =
                checked_offset(input_offset, checked_mul(index, stride, path)?, path)?;
            self.compile_node(
                element_shape,
                element_descriptor,
                element,
                element_input_offset,
                &format!("{path}[{index}]"),
                pending,
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.option]
    fn push_option(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        input_offset: usize,
        element: &WriterNode,
        path: &str,
    ) -> Result<bool, StencilError> {
        let Def::Option(option) = shape.def else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer option stencil requires Facet option shape",
            });
        };

        let (some_descriptor, representation) = local_option_descriptor(descriptor, path)?;
        let mut compiler = StencilEncodeCompiler {
            ops: Vec::new(),
            helpers: Vec::new(),
            failures: Vec::new(),
        };
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        compiler.compile_node(
            option.t(),
            some_descriptor,
            element,
            0,
            &format!("{path}.some"),
            &mut pending,
        )?;
        compiler.flush_direct_segment(&mut pending);
        if !compiler.helpers.is_empty() {
            return Ok(false);
        }

        let layout = if matches!(element, WriterNode::Primitive(Primitive::String))
            && matches!(
                representation,
                LocalOptionRepresentation::NicheString { .. }
            ) {
            EncodeOptionLayout::NicheString
        } else {
            EncodeOptionLayout::Facet
        };

        self.ops.push(EncodeStencilOp::Option {
            shape,
            input_offset,
            layout,
            some_ops: compiler.ops,
        });
        Ok(true)
    }

    // r[impl binette.aggregate.list]
    fn push_list(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        input_offset: usize,
        element: &WriterNode,
        path: &str,
    ) -> Result<bool, StencilError> {
        let Def::List(list) = shape.def else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer list stencil requires Facet list shape",
            });
        };

        let (element_descriptor, element_stride) = local_sequence_element(descriptor, path)?;
        let mut compiler = StencilEncodeCompiler {
            ops: Vec::new(),
            helpers: Vec::new(),
            failures: Vec::new(),
        };
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        compiler.compile_node(
            list.t(),
            element_descriptor,
            element,
            0,
            &format!("{path}[]"),
            &mut pending,
        )?;
        compiler.flush_direct_segment(&mut pending);
        if !compiler.helpers.is_empty() {
            return Ok(false);
        }

        let layout = match local_sequence_storage(descriptor, path)? {
            LocalSequenceStorage::DirectContiguous {
                pointer: LocalAccess::Direct { offset: ptr_offset },
                length: LocalAccess::Direct { offset: len_offset },
                ..
            } => EncodeListLayout::Vec {
                ptr_offset: *ptr_offset,
                len_offset: *len_offset,
                element_stride,
            },
            _ => EncodeListLayout::Facet,
        };

        self.ops.push(EncodeStencilOp::List {
            shape,
            input_offset,
            layout,
            element_ops: compiler.ops,
        });
        Ok(true)
    }

    fn push_node_helper(
        &mut self,
        shape: &'static Shape,
        node: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilEncodeHelper::Node {
            node: node.clone(),
            shape,
            input_offset,
            failure_index,
        });
        self.ops.push(EncodeStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_bytes(&mut self, shape: &'static Shape, input_offset: usize, kind: EncodeBytesKind) {
        self.ops.push(EncodeStencilOp::Bytes {
            shape,
            input_offset,
            kind,
        });
    }

    fn push_enum(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        input_offset: usize,
        variants: &[WriterVariantPlan],
        path: &str,
    ) -> Result<(), StencilError> {
        let Type::User(UserType::Enum(enum_type)) = shape.ty else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer enum stencil requires Facet enum shape",
            });
        };

        let mut cases = Vec::with_capacity(variants.len());
        for variant in variants {
            let Some(facet_variant) = enum_type.variants.get(variant.facet_index) else {
                return Err(StencilError::Unsupported {
                    path: path.to_owned(),
                    reason: "writer enum variant index is out of range",
                });
            };
            let variant_path = format!("{path}.{}", facet_variant.effective_name());
            let local_variant = local_enum_variant(descriptor, variant.facet_index, path)?;
            let ops = self.compile_variant_payload_ops(
                &variant.payload,
                facet_variant.data,
                local_variant.payload.as_deref(),
                input_offset,
                &variant_path,
            )?;
            cases.push(EncodeEnumCase {
                facet_index: variant.facet_index,
                wire_index: variant.wire_index,
                ops,
            });
        }

        self.ops.push(EncodeStencilOp::Enum {
            shape,
            input_offset,
            cases,
        });
        Ok(())
    }

    fn compile_variant_payload_ops(
        &mut self,
        payload: &WriterVariantPayloadPlan,
        data: facet_core::StructType,
        payload_descriptor: Option<&LocalTypeDescriptor>,
        input_offset: usize,
        path: &str,
    ) -> Result<Vec<EncodeStencilOp>, StencilError> {
        let outer_ops = std::mem::take(&mut self.ops);
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        let result = (|| {
            match payload {
                WriterVariantPayloadPlan::Unit => {}
                WriterVariantPayloadPlan::Newtype(element) => {
                    let payload_descriptor =
                        payload_descriptor.ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "writer enum newtype payload is missing local descriptor",
                        })?;
                    self.compile_variant_tuple_element(
                        data.fields,
                        VariantElementLocal {
                            descriptor: payload_descriptor,
                            access: None,
                        },
                        element,
                        input_offset,
                        path,
                        &mut pending,
                    )?;
                }
                WriterVariantPayloadPlan::Tuple(elements) => {
                    let descriptor_fields = local_struct_fields(
                        payload_descriptor.ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "writer enum tuple payload is missing local descriptor",
                        })?,
                        path,
                    )?;
                    for element in elements {
                        let field_descriptor = descriptor_fields
                            .get(element.facet_index)
                            .ok_or_else(|| StencilError::Unsupported {
                                path: path.to_owned(),
                                reason: "writer enum tuple descriptor field index is out of range",
                            })?;
                        self.compile_variant_tuple_element(
                            data.fields,
                            VariantElementLocal {
                                descriptor: &field_descriptor.descriptor,
                                access: Some(&field_descriptor.access),
                            },
                            element,
                            input_offset,
                            path,
                            &mut pending,
                        )?;
                    }
                }
                WriterVariantPayloadPlan::Struct(fields) => {
                    let descriptor_fields = local_struct_fields(
                        payload_descriptor.ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "writer enum struct payload is missing local descriptor",
                        })?,
                        path,
                    )?;
                    for field in fields {
                        self.compile_variant_struct_field(
                            data.fields,
                            descriptor_fields,
                            field,
                            input_offset,
                            path,
                            &mut pending,
                        )?;
                    }
                }
            }
            self.flush_direct_segment(&mut pending);
            Ok(())
        })();
        let payload_ops = std::mem::replace(&mut self.ops, outer_ops);
        result.map(|()| payload_ops)
    }

    fn compile_variant_tuple_element(
        &mut self,
        facet_fields: &'static [facet_core::Field],
        local: VariantElementLocal<'_>,
        element: &WriterTupleElementPlan,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let element_path = format!("{path}.{}", element.facet_index);
        let Some(facet_field) = facet_fields.get(element.facet_index) else {
            return Err(StencilError::Unsupported {
                path: element_path,
                reason: "writer enum tuple field index is out of range",
            });
        };
        let field_offset = checked_offset(
            input_offset,
            match local.access {
                Some(access) => local_direct_offset(access, path)?,
                None => facet_field.offset,
            },
            path,
        )?;
        self.compile_node(
            facet_field.shape(),
            local.descriptor,
            &element.node,
            field_offset,
            &element_path,
            pending,
        )
    }

    fn compile_variant_struct_field(
        &mut self,
        facet_fields: &'static [facet_core::Field],
        descriptor_fields: &[LocalFieldDescriptor],
        field: &WriterFieldPlan,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let field_path = format!("{path}.{}", field.name);
        let Some(facet_field) = facet_fields.get(field.facet_index) else {
            return Err(StencilError::Unsupported {
                path: field_path,
                reason: "writer enum struct field index is out of range",
            });
        };
        let field_descriptor =
            descriptor_fields
                .get(field.facet_index)
                .ok_or_else(|| StencilError::Unsupported {
                    path: field_path.clone(),
                    reason: "writer enum struct descriptor field index is out of range",
                })?;
        let field_offset = checked_offset(
            input_offset,
            local_direct_offset(&field_descriptor.access, path)?,
            path,
        )?;
        self.compile_node(
            facet_field.shape(),
            &field_descriptor.descriptor,
            &field.node,
            field_offset,
            &field_path,
            pending,
        )
    }

    fn push_helper_failure(&mut self, path: &str) -> Result<usize, StencilError> {
        let failure_index = self.failures.len();
        let _ = status_for_failure(failure_index)?;
        self.failures.push(StencilFailure::Helper {
            path: path.to_owned(),
        });
        Ok(failure_index)
    }

    fn push_direct_segment(&mut self, segment: FixedEncodeSegment) {
        if segment.output_len == 0 {
            return;
        }
        self.ops.push(EncodeStencilOp::Direct {
            ops: segment.ops,
            output_len: segment.output_len,
        });
    }

    fn flush_direct_segment(&mut self, segment: &mut FixedEncodeSegment) {
        if segment.output_len == 0 {
            segment.ops.clear();
            return;
        }
        let segment = std::mem::replace(
            segment,
            FixedEncodeSegment {
                ops: Vec::new(),
                output_len: 0,
            },
        );
        self.push_direct_segment(segment);
    }
}

impl FixedEncodeCompiler {
    pub(super) fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &WriterNode,
    ) -> Result<usize, StencilError> {
        let writer = rust_facet_descriptor(T::SHAPE, "$")?;
        self.compile_node(T::SHAPE, &writer, root, 0, "$")?;
        Ok(self.output_offset)
    }

    // r[impl binette.local-access.descriptor]
    pub(super) fn compile_descriptor_root(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        root: &WriterNode,
    ) -> Result<usize, StencilError> {
        self.compile_descriptor_node(descriptor, root, 0, "$")?;
        Ok(self.output_offset)
    }

    fn compile_node(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        node: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        match node {
            WriterNode::Ref { .. } => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct encode stencil does not support recursive writer refs",
            }),
            WriterNode::Primitive(primitive) => {
                self.compile_primitive(*primitive, input_offset, path)
            }
            WriterNode::Struct { fields } => {
                self.compile_struct(shape, descriptor, fields, input_offset, path)
            }
            WriterNode::Tuple { elements } => {
                self.compile_tuple(shape, descriptor, elements, input_offset, path)
            }
            WriterNode::Array {
                dimensions,
                element,
            } => self.compile_array(shape, descriptor, dimensions, element, input_offset, path),
            WriterNode::External => Ok(()),
            WriterNode::Enum { .. }
            | WriterNode::Result { .. }
            | WriterNode::List { .. }
            | WriterNode::Set { .. }
            | WriterNode::Map { .. }
            | WriterNode::Option { .. }
            | WriterNode::Dynamic => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct encode stencil only supports fixed-width roots",
            }),
        }
    }

    fn compile_descriptor_node(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        node: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        match node {
            WriterNode::Ref { .. } => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct local encode stencil does not support recursive writer refs",
            }),
            WriterNode::Primitive(primitive) => {
                validate_descriptor_primitive(descriptor, *primitive, path)?;
                self.compile_primitive(*primitive, input_offset, path)
            }
            WriterNode::Struct { fields } => {
                self.compile_descriptor_struct(descriptor, fields, input_offset, path)
            }
            WriterNode::Tuple { elements } => {
                self.compile_descriptor_tuple(descriptor, elements, input_offset, path)
            }
            WriterNode::Array {
                dimensions,
                element,
            } => self.compile_descriptor_array(descriptor, dimensions, element, input_offset, path),
            WriterNode::External => Ok(()),
            WriterNode::Enum { .. }
            | WriterNode::Result { .. }
            | WriterNode::List { .. }
            | WriterNode::Set { .. }
            | WriterNode::Map { .. }
            | WriterNode::Option { .. }
            | WriterNode::Dynamic => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct local encode stencil only supports fixed-width roots",
            }),
        }
    }

    fn compile_descriptor_struct(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        fields: &[WriterFieldPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        for field in fields {
            let field_descriptor = descriptor_fields.get(field.facet_index).ok_or_else(|| {
                StencilError::Unsupported {
                    path: format!("{path}.{}", field.name),
                    reason: "writer descriptor field index is out of range",
                }
            })?;
            let field_offset = checked_offset(
                input_offset,
                local_direct_offset(&field_descriptor.access, path)?,
                path,
            )?;
            self.compile_descriptor_node(
                &field_descriptor.descriptor,
                &field.node,
                field_offset,
                &format!("{path}.{}", field.name),
            )?;
        }
        Ok(())
    }

    fn compile_descriptor_tuple(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        elements: &[WriterTupleElementPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        if descriptor_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer tuple arity differs from local descriptor",
            });
        }
        for element in elements {
            let element_descriptor =
                descriptor_fields.get(element.facet_index).ok_or_else(|| {
                    StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "writer tuple descriptor field index is out of range",
                    }
                })?;
            let field_offset = checked_offset(
                input_offset,
                local_direct_offset(&element_descriptor.access, path)?,
                path,
            )?;
            self.compile_descriptor_node(
                &element_descriptor.descriptor,
                &element.node,
                field_offset,
                &format!("{path}.{}", element.facet_index),
            )?;
        }
        Ok(())
    }

    fn compile_descriptor_array(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        dimensions: &[u64],
        element: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let expected_count = dimensions_element_count(dimensions, path)?;
        let (element_descriptor, element_count, element_stride) =
            local_inline_fixed_array(descriptor, path)?;
        if element_count != expected_count {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer array dimensions differ from local descriptor",
            });
        }
        for index in 0..element_count {
            let element_input_offset = checked_offset(
                input_offset,
                checked_mul(index, element_stride, path)?,
                path,
            )?;
            self.compile_descriptor_node(
                element_descriptor,
                element,
                element_input_offset,
                &format!("{path}[{index}]"),
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.struct.compact]
    fn compile_struct(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        fields: &[WriterFieldPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        for field in fields {
            let facet_field =
                facet_fields
                    .get(field.facet_index)
                    .ok_or_else(|| StencilError::Unsupported {
                        path: format!("{path}.{}", field.name),
                        reason: "writer field index is out of range",
                    })?;
            let field_descriptor = descriptor_fields.get(field.facet_index).ok_or_else(|| {
                StencilError::Unsupported {
                    path: format!("{path}.{}", field.name),
                    reason: "writer descriptor field index is out of range",
                }
            })?;
            let field_offset = checked_offset(
                input_offset,
                local_direct_offset(&field_descriptor.access, path)?,
                path,
            )?;
            self.compile_node(
                facet_field.shape.get(),
                &field_descriptor.descriptor,
                &field.node,
                field_offset,
                &format!("{path}.{}", field.name),
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.tuple]
    fn compile_tuple(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        elements: &[WriterTupleElementPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        if facet_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer tuple arity differs from Facet shape",
            });
        }
        for element in elements {
            let facet_field =
                facet_fields
                    .get(element.facet_index)
                    .ok_or_else(|| StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "writer tuple field index is out of range",
                    })?;
            let element_descriptor =
                descriptor_fields.get(element.facet_index).ok_or_else(|| {
                    StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "writer tuple descriptor field index is out of range",
                    }
                })?;
            let field_offset = checked_offset(
                input_offset,
                local_direct_offset(&element_descriptor.access, path)?,
                path,
            )?;
            self.compile_node(
                facet_field.shape.get(),
                &element_descriptor.descriptor,
                &element.node,
                field_offset,
                &format!("{path}.{}", element.facet_index),
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.array]
    fn compile_array(
        &mut self,
        shape: &'static Shape,
        descriptor: &LocalTypeDescriptor,
        dimensions: &[u64],
        element: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (element_shape, count, stride) =
            fixed_array_parts(shape, dimensions, path, "writer array")?;
        let (element_descriptor, descriptor_count, descriptor_stride) =
            local_inline_fixed_array(descriptor, path)?;
        if descriptor_count != count || descriptor_stride != stride {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer array descriptor differs from Facet array layout",
            });
        }
        for index in 0..count {
            let element_input_offset =
                checked_offset(input_offset, checked_mul(index, stride, path)?, path)?;
            self.compile_node(
                element_shape,
                element_descriptor,
                element,
                element_input_offset,
                &format!("{path}[{index}]"),
            )?;
        }
        Ok(())
    }

    // r[impl binette.scalar.bool]
    // r[impl binette.scalar.unsigned]
    // r[impl binette.scalar.signed]
    // r[impl binette.scalar.float]
    // r[impl binette.scalar.char]
    fn compile_primitive(
        &mut self,
        primitive: Primitive,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let Some(widths) = encode_primitive_widths(primitive) else {
            return Err(unsupported_encode_primitive(path, primitive));
        };
        let mut input_offset = input_offset;
        for width in widths {
            self.ops.push(CopyOp {
                input_offset,
                output_offset: self.output_offset,
                width: *width,
            });
            input_offset = checked_offset(input_offset, width.bytes(), path)?;
            self.output_offset = checked_offset(self.output_offset, width.bytes(), path)?;
        }
        Ok(())
    }
}

fn fixed_encode_segment(
    shape: &'static Shape,
    descriptor: &LocalTypeDescriptor,
    node: &WriterNode,
    input_offset: usize,
    output_offset: usize,
    path: &str,
) -> Result<FixedEncodeSegment, StencilError> {
    let mut compiler = FixedEncodeCompiler {
        ops: Vec::new(),
        output_offset,
    };
    compiler.compile_node(shape, descriptor, node, input_offset, path)?;
    let output_len = compiler
        .output_offset
        .checked_sub(output_offset)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "direct encode output offset underflow",
        })?;
    Ok(FixedEncodeSegment {
        ops: compiler.ops,
        output_len,
    })
}

fn copy_ops_fit_direct_code(ops: &[CopyOp]) -> bool {
    ops.iter()
        .all(|op| op.input_offset <= 255 && op.output_offset <= 255)
}

impl CopyWidth {
    fn bytes(self) -> usize {
        match self {
            CopyWidth::One => 1,
            CopyWidth::Two => 2,
            CopyWidth::Four => 4,
            CopyWidth::Eight => 8,
        }
    }
}

const WIDTH_0: &[CopyWidth] = &[];
const WIDTH_1: &[CopyWidth] = &[CopyWidth::One];
const WIDTH_2: &[CopyWidth] = &[CopyWidth::Two];
const WIDTH_4: &[CopyWidth] = &[CopyWidth::Four];
const WIDTH_8: &[CopyWidth] = &[CopyWidth::Eight];
const WIDTH_16: &[CopyWidth] = &[CopyWidth::Eight, CopyWidth::Eight];

fn primitive_widths(primitive: Primitive) -> Option<&'static [CopyWidth]> {
    match primitive {
        Primitive::Unit => Some(WIDTH_0),
        Primitive::U8 | Primitive::I8 => Some(WIDTH_1),
        Primitive::U16 | Primitive::I16 => Some(WIDTH_2),
        Primitive::U32 | Primitive::I32 | Primitive::F32 => Some(WIDTH_4),
        Primitive::U64 | Primitive::I64 | Primitive::F64 => Some(WIDTH_8),
        Primitive::U128 | Primitive::I128 => Some(WIDTH_16),
        Primitive::Bool
        | Primitive::Never
        | Primitive::Char
        | Primitive::String
        | Primitive::Bytes
        | Primitive::Payload => None,
    }
}

fn encode_primitive_widths(primitive: Primitive) -> Option<&'static [CopyWidth]> {
    match primitive {
        Primitive::Unit => Some(WIDTH_0),
        Primitive::Bool | Primitive::U8 | Primitive::I8 => Some(WIDTH_1),
        Primitive::U16 | Primitive::I16 => Some(WIDTH_2),
        Primitive::U32 | Primitive::I32 | Primitive::F32 | Primitive::Char => Some(WIDTH_4),
        Primitive::U64 | Primitive::I64 | Primitive::F64 => Some(WIDTH_8),
        Primitive::U128 | Primitive::I128 => Some(WIDTH_16),
        Primitive::Never | Primitive::String | Primitive::Bytes | Primitive::Payload => None,
    }
}

fn unsupported_primitive(path: &str, primitive: Primitive) -> StencilError {
    StencilError::Unsupported {
        path: path.to_owned(),
        reason: match primitive {
            Primitive::Bool => "bool stencil decode still needs validation",
            Primitive::Never => "never has no compact value",
            Primitive::Char => "char stencil decode still needs scalar validation",
            Primitive::String | Primitive::Bytes | Primitive::Payload => {
                "variable-width stencil decode is not implemented yet"
            }
            _ => "unsupported primitive stencil decode",
        },
    }
}

fn unsupported_encode_primitive(path: &str, primitive: Primitive) -> StencilError {
    StencilError::Unsupported {
        path: path.to_owned(),
        reason: match primitive {
            Primitive::Never => "never has no compact value",
            Primitive::String | Primitive::Bytes | Primitive::Payload => {
                "variable-width direct encode stencil is not implemented yet"
            }
            _ => "unsupported primitive direct encode stencil",
        },
    }
}

fn primitive_for_plain_type_ref(type_ref: &TypeRef) -> Option<Primitive> {
    match type_ref {
        TypeRef::Concrete { type_id, args } if args.is_empty() => primitive_for_type_id(*type_id),
        TypeRef::Concrete { .. } | TypeRef::Var { .. } => None,
    }
}

fn rust_facet_descriptor(
    shape: &'static Shape,
    path: &str,
) -> Result<LocalTypeDescriptor, StencilError> {
    rust_facet_descriptor_for_shape(shape).map_err(|_| StencilError::Unsupported {
        path: path.to_owned(),
        reason: "failed to build Rust local access descriptor for stencil compilation",
    })
}

fn validate_descriptor_primitive(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    path: &str,
) -> Result<(), StencilError> {
    let LocalTypeKind::Scalar(LocalScalarAccess::Plain) = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not a plain scalar layout",
        });
    };

    let expected = TypeRef::concrete(primitive_type_id(primitive));
    match &descriptor.schema {
        LocalSchemaRef::Type(type_ref) if type_ref == &expected => Ok(()),
        _ => Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor primitive schema differs from writer primitive",
        }),
    }
}

fn local_struct_fields<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<&'a [LocalFieldDescriptor], StencilError> {
    let LocalTypeKind::Struct { fields } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not a struct layout",
        });
    };
    Ok(fields)
}

fn local_direct_offset(access: &LocalAccess, path: &str) -> Result<usize, StencilError> {
    let LocalAccess::Direct { offset } = access else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor field requires an accessor thunk",
        });
    };
    Ok(*offset)
}

fn local_sequence_element<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<(&'a LocalTypeDescriptor, usize), StencilError> {
    let LocalTypeKind::Sequence { element, storage } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not a sequence layout",
        });
    };
    let element_stride = match storage {
        LocalSequenceStorage::InlineFixed { element_stride, .. }
        | LocalSequenceStorage::DirectContiguous { element_stride, .. } => *element_stride,
        LocalSequenceStorage::Thunk { .. } => {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local sequence descriptor requires an accessor thunk",
            });
        }
    };
    Ok((element, element_stride))
}

fn local_sequence_storage<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<&'a LocalSequenceStorage, StencilError> {
    let LocalTypeKind::Sequence { storage, .. } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not a sequence layout",
        });
    };
    Ok(storage)
}

fn local_option_descriptor<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<(&'a LocalTypeDescriptor, &'a LocalOptionRepresentation), StencilError> {
    let LocalTypeKind::Option {
        some,
        representation,
    } = &descriptor.kind
    else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not an option layout",
        });
    };
    Ok((some, representation))
}

fn local_inline_fixed_array<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<(&'a LocalTypeDescriptor, usize, usize), StencilError> {
    let LocalTypeKind::Sequence { element, storage } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not an array sequence layout",
        });
    };
    let LocalSequenceStorage::InlineFixed {
        element_count,
        element_stride,
        ..
    } = storage
    else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not an inline fixed array",
        });
    };
    Ok((element, *element_count, *element_stride))
}

fn local_enum_variant<'a>(
    descriptor: &'a LocalTypeDescriptor,
    index: usize,
    path: &str,
) -> Result<&'a crate::local_access::LocalVariantDescriptor, StencilError> {
    let LocalTypeKind::Enum { variants, .. } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not an enum layout",
        });
    };
    variants
        .get(index)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor enum variant index is out of range",
        })
}

fn shape_struct_fields(
    shape: &'static Shape,
    path: &str,
) -> Result<&'static [facet_core::Field], StencilError> {
    let Type::User(user) = shape.ty else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader shape is not a struct",
        });
    };
    let UserType::Struct(struct_type) = user else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader shape is not a struct",
        });
    };
    match struct_type.kind {
        StructKind::Struct | StructKind::TupleStruct | StructKind::Tuple => Ok(struct_type.fields),
        StructKind::Unit => Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "unit struct stencil decode is not implemented yet",
        }),
    }
}

fn shape_enum_type(shape: &'static Shape, path: &str) -> Result<EnumType, StencilError> {
    let Type::User(UserType::Enum(enum_type)) = shape.ty else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader shape is not an enum",
        });
    };
    Ok(enum_type)
}

fn fixed_array_parts(
    shape: &'static Shape,
    dimensions: &[u64],
    path: &str,
    context: &'static str,
) -> Result<(&'static Shape, usize, usize), StencilError> {
    let Def::Array(array) = shape.def else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil array requires Facet array shape",
        });
    };
    let expected = dimensions_element_count(dimensions, path)?;
    if array.n != expected {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: match context {
                "reader array" => "reader array length differs from schema dimensions",
                "writer array" => "writer array length differs from schema dimensions",
                _ => "array length differs from schema dimensions",
            },
        });
    }
    let stride = array
        .t()
        .layout
        .sized_layout()
        .map_err(|_| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil array element shape is unsized",
        })?
        .size();
    Ok((array.t(), expected, stride))
}

fn dimensions_element_count(dimensions: &[u64], path: &str) -> Result<usize, StencilError> {
    dimensions.iter().try_fold(1usize, |count, dimension| {
        let dimension = usize::try_from(*dimension).map_err(|_| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "array dimension exceeds usize",
        })?;
        checked_mul(count, dimension, path)
    })
}

fn enum_discriminant_u8(variant: &facet_core::Variant, path: &str) -> Result<u8, StencilError> {
    let discriminant = variant
        .discriminant
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "reader enum variant is missing a discriminant",
        })?;
    u8::try_from(discriminant).map_err(|_| StencilError::Unsupported {
        path: path.to_owned(),
        reason: "reader enum discriminant does not fit repr(u8)",
    })
}

fn checked_offset(offset: usize, width: usize, path: &str) -> Result<usize, StencilError> {
    offset
        .checked_add(width)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset overflow",
        })
}

fn copy_ops_from_stencil_ops(ops: &[StencilOp]) -> Option<Vec<CopyOp>> {
    ops.iter()
        .map(|op| match op {
            StencilOp::Copy(op) => Some(*op),
            StencilOp::Bool { .. } | StencilOp::RootEnum { .. } | StencilOp::RootList { .. } => {
                None
            }
        })
        .collect()
}

fn fixed_copy_ops(
    writer_registry: &SchemaRegistry,
    plan_nodes: &[PlanNode],
    node: &PlanNode,
    reader_shape: &'static Shape,
    reader_descriptor: &LocalTypeDescriptor,
    output_offset: usize,
    path: &str,
) -> Result<(Vec<CopyOp>, usize), StencilError> {
    let mut compiler = StencilCompiler {
        writer_registry,
        plan_nodes,
        ops: Vec::new(),
        failures: Vec::new(),
        input_offset: 0,
    };
    compiler.compile_node(reader_shape, reader_descriptor, node, output_offset, path)?;
    if !compiler.failures.is_empty() {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "cursor decode currently supports only infallible fixed-copy stencils",
        });
    }
    let Some(ops) = copy_ops_from_stencil_ops(&compiler.ops) else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "cursor decode currently supports only fixed-copy stencils",
        });
    };
    Ok((ops, compiler.input_offset))
}

fn fixed_skip_len(
    writer_registry: &SchemaRegistry,
    writer_type: &TypeRef,
    path: &str,
) -> Result<usize, StencilError> {
    let mut compiler = StencilCompiler {
        writer_registry,
        plan_nodes: &[],
        ops: Vec::new(),
        failures: Vec::new(),
        input_offset: 0,
    };
    compiler.compile_skip_type(writer_type, path)?;
    if !compiler.failures.is_empty() || copy_ops_from_stencil_ops(&compiler.ops).is_none() {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "cursor decode currently supports only infallible fixed-copy skips",
        });
    }
    Ok(compiler.input_offset)
}

fn checked_mul(lhs: usize, rhs: usize, path: &str) -> Result<usize, StencilError> {
    lhs.checked_mul(rhs)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset overflow",
        })
}
