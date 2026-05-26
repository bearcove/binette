use super::*;

pub(super) struct StencilCompiler<'registry> {
    pub(super) writer_registry: &'registry SchemaRegistry,
    pub(super) ops: Vec<StencilOp>,
    pub(super) failures: Vec<StencilFailure>,
    pub(super) input_offset: usize,
}

impl StencilCompiler<'_> {
    pub(super) fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &PlanNode,
    ) -> Result<LengthCheck, StencilError> {
        if let PlanNode::Enum { variants } = root {
            return self.compile_enum_root(T::SHAPE, variants, "$");
        }

        self.compile_node(T::SHAPE, root, 0, "$")?;
        Ok(LengthCheck::Exact(self.input_offset))
    }

    fn compile_node(
        &mut self,
        reader_shape: &'static Shape,
        node: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        match node {
            PlanNode::Primitive { primitive } => {
                self.compile_primitive_read(*primitive, output_offset, path)
            }
            PlanNode::Struct { fields } => {
                self.compile_struct_plan(reader_shape, fields, output_offset, path)
            }
            PlanNode::Tuple { elements } => {
                self.compile_tuple_plan(reader_shape, elements, output_offset, path)
            }
            PlanNode::Array {
                dimensions,
                element,
            } => self.compile_array_plan(reader_shape, dimensions, element, output_offset, path),
            PlanNode::Enum { variants } if output_offset == 0 => self
                .compile_enum_root(reader_shape, variants, path)
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
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        self.compile_struct_fields_plan(reader_fields, fields, output_offset, path)
    }

    fn compile_struct_fields_plan(
        &mut self,
        reader_fields: &'static [facet_core::Field],
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
                    let field_offset = checked_offset(output_offset, reader_field.offset, path)?;
                    self.compile_read_plan(
                        plan,
                        reader_field.shape.get(),
                        field_offset,
                        &format!("{path}.{name}"),
                    )?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.compile_skip(writer_type, &format!("{path}.{name}"))?,
            }
        }
        Ok(())
    }

    fn compile_tuple_plan(
        &mut self,
        reader_shape: &'static Shape,
        elements: &[PlanNode],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
        if reader_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "stencil tuple element count differs from reader shape",
            });
        }
        for (index, element) in elements.iter().enumerate() {
            let reader_field = &reader_fields[index];
            let field_offset = checked_offset(output_offset, reader_field.offset, path)?;
            self.compile_read_plan(
                element,
                reader_field.shape.get(),
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
        dimensions: &[u64],
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (element_shape, count, stride) =
            fixed_array_parts(reader_shape, dimensions, path, "reader array")?;
        for index in 0..count {
            let element_output_offset =
                checked_offset(output_offset, checked_mul(index, stride, path)?, path)?;
            self.compile_read_plan(
                element,
                element_shape,
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
                    let (body, expected) =
                        self.compile_branch_body(input_offset + 4, |compiler| {
                            compiler.compile_enum_payload_plan(
                                reader_variant.data.fields,
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
                self.compile_read_plan(element, reader_field.shape.get(), reader_field.offset, path)
            }
            EnumPayloadPlan::Tuple(elements) => {
                if reader_fields.len() != elements.len() {
                    return Err(StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "tuple enum payload arity differs from reader shape",
                    });
                }
                for (index, element) in elements.iter().enumerate() {
                    let reader_field = &reader_fields[index];
                    self.compile_read_plan(
                        element,
                        reader_field.shape.get(),
                        reader_field.offset,
                        &format!("{path}.{index}"),
                    )?;
                }
                Ok(())
            }
            EnumPayloadPlan::Struct(fields) => {
                self.compile_struct_fields_plan(reader_fields, fields, 0, path)
            }
        }
    }

    fn compile_read_plan(
        &mut self,
        node: &PlanNode,
        reader_shape: &'static Shape,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        self.compile_node(reader_shape, node, output_offset, path)
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

pub(super) struct HybridStencilCompiler {
    pub(super) ops: Vec<HybridStencilOp>,
    pub(super) helpers: Vec<StencilHelper>,
    pub(super) failures: Vec<StencilFailure>,
}

impl HybridStencilCompiler {
    pub(super) fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &PlanNode,
    ) -> Result<(), StencilError> {
        match root {
            PlanNode::Struct { fields } => self.compile_struct_root(T::SHAPE, fields, 0, "$"),
            _ => self.push_decode_helper(root, T::SHAPE, 0, "$"),
        }
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct_root(
        &mut self,
        reader_shape: &'static Shape,
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let reader_fields = shape_struct_fields(reader_shape, path)?;
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
                    let field_offset = checked_offset(output_offset, reader_field.offset, path)?;
                    self.push_decode_helper(
                        plan,
                        reader_field.shape.get(),
                        field_offset,
                        &format!("{path}.{name}"),
                    )?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.push_skip_helper(writer_type, &format!("{path}.{name}"))?,
            }
        }
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

impl StencilEncodeCompiler {
    pub(super) fn compile_root<T: Facet<'static>>(
        &mut self,
        root: &WriterNode,
    ) -> Result<(), StencilError> {
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        self.compile_node(T::SHAPE, root, 0, "$", &mut pending)?;
        self.flush_direct_segment(&mut pending);
        Ok(())
    }

    fn compile_node(
        &mut self,
        shape: &'static Shape,
        node: &WriterNode,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        match fixed_encode_segment(shape, node, input_offset, pending.output_len, path) {
            Ok(segment) if copy_ops_fit_direct_code(&segment.ops) => {
                pending.ops.extend(segment.ops);
                pending.output_len = checked_offset(pending.output_len, segment.output_len, path)?;
                return Ok(());
            }
            Ok(_) => {
                self.flush_direct_segment(pending);
                match fixed_encode_segment(shape, node, input_offset, 0, path) {
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
                self.compile_struct_root(shape, fields, input_offset, path, pending)
            }
            WriterNode::Tuple { elements } => {
                self.compile_tuple_root(shape, elements, input_offset, path, pending)
            }
            WriterNode::Array {
                dimensions,
                element,
            } => self.compile_array_root(shape, dimensions, element, input_offset, path, pending),
            WriterNode::Option { element } => {
                self.flush_direct_segment(pending);
                if self.push_option(shape, input_offset, element, path)? {
                    Ok(())
                } else {
                    self.push_node_helper(shape, node, input_offset, path)
                }
            }
            WriterNode::List { element } => {
                self.flush_direct_segment(pending);
                if self.push_list(shape, input_offset, element, path)? {
                    Ok(())
                } else {
                    self.push_node_helper(shape, node, input_offset, path)
                }
            }
            WriterNode::Enum { variants } => {
                self.flush_direct_segment(pending);
                self.push_enum(shape, input_offset, variants, path)
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
        fields: &[WriterFieldPlan],
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
        for field in fields {
            let field_path = format!("{path}.{}", field.name);
            let Some(facet_field) = facet_fields.get(field.facet_index) else {
                return Err(StencilError::Unsupported {
                    path: field_path,
                    reason: "writer field index is out of range",
                });
            };
            let field_offset = checked_offset(input_offset, facet_field.offset, path)?;
            self.compile_node(
                facet_field.shape.get(),
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
        elements: &[WriterTupleElementPlan],
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
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
            let element_offset = checked_offset(input_offset, facet_field.offset, path)?;
            self.compile_node(
                facet_field.shape.get(),
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
        shape: &'static Shape,
        dimensions: &[u64],
        element: &WriterNode,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let (element_shape, count, stride) =
            fixed_array_parts(shape, dimensions, path, "writer array")?;
        for index in 0..count {
            let element_input_offset =
                checked_offset(input_offset, checked_mul(index, stride, path)?, path)?;
            self.compile_node(
                element_shape,
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
            element,
            0,
            &format!("{path}.some"),
            &mut pending,
        )?;
        compiler.flush_direct_segment(&mut pending);
        if !compiler.helpers.is_empty() {
            return Ok(false);
        }

        self.ops.push(EncodeStencilOp::Option {
            shape,
            input_offset,
            some_ops: compiler.ops,
        });
        Ok(true)
    }

    // r[impl binette.aggregate.list]
    fn push_list(
        &mut self,
        shape: &'static Shape,
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

        let mut compiler = StencilEncodeCompiler {
            ops: Vec::new(),
            helpers: Vec::new(),
            failures: Vec::new(),
        };
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        compiler.compile_node(list.t(), element, 0, &format!("{path}[]"), &mut pending)?;
        compiler.flush_direct_segment(&mut pending);
        if !compiler.helpers.is_empty() {
            return Ok(false);
        }

        self.ops.push(EncodeStencilOp::List {
            shape,
            input_offset,
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
            let ops = self.compile_variant_payload_ops(
                &variant.payload,
                facet_variant.data,
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
                    self.compile_variant_tuple_element(
                        data.fields,
                        element,
                        input_offset,
                        path,
                        &mut pending,
                    )?;
                }
                WriterVariantPayloadPlan::Tuple(elements) => {
                    for element in elements {
                        self.compile_variant_tuple_element(
                            data.fields,
                            element,
                            input_offset,
                            path,
                            &mut pending,
                        )?;
                    }
                }
                WriterVariantPayloadPlan::Struct(fields) => {
                    for field in fields {
                        self.compile_variant_struct_field(
                            data.fields,
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
        let field_offset = checked_offset(input_offset, facet_field.offset, path)?;
        self.compile_node(
            facet_field.shape(),
            &element.node,
            field_offset,
            &element_path,
            pending,
        )
    }

    fn compile_variant_struct_field(
        &mut self,
        facet_fields: &'static [facet_core::Field],
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
        let field_offset = checked_offset(input_offset, facet_field.offset, path)?;
        self.compile_node(
            facet_field.shape(),
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
        self.compile_node(T::SHAPE, root, 0, "$")?;
        Ok(self.output_offset)
    }

    fn compile_node(
        &mut self,
        shape: &'static Shape,
        node: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        match node {
            WriterNode::Primitive(primitive) => {
                self.compile_primitive(*primitive, input_offset, path)
            }
            WriterNode::Struct { fields } => self.compile_struct(shape, fields, input_offset, path),
            WriterNode::Tuple { elements } => {
                self.compile_tuple(shape, elements, input_offset, path)
            }
            WriterNode::Array {
                dimensions,
                element,
            } => self.compile_array(shape, dimensions, element, input_offset, path),
            WriterNode::External => Ok(()),
            WriterNode::Enum { .. }
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

    // r[impl binette.aggregate.struct.compact]
    fn compile_struct(
        &mut self,
        shape: &'static Shape,
        fields: &[WriterFieldPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
        for field in fields {
            let facet_field =
                facet_fields
                    .get(field.facet_index)
                    .ok_or_else(|| StencilError::Unsupported {
                        path: format!("{path}.{}", field.name),
                        reason: "writer field index is out of range",
                    })?;
            let field_offset = checked_offset(input_offset, facet_field.offset, path)?;
            self.compile_node(
                facet_field.shape.get(),
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
        elements: &[WriterTupleElementPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let facet_fields = shape_struct_fields(shape, path)?;
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
            let field_offset = checked_offset(input_offset, facet_field.offset, path)?;
            self.compile_node(
                facet_field.shape.get(),
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
        dimensions: &[u64],
        element: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (element_shape, count, stride) =
            fixed_array_parts(shape, dimensions, path, "writer array")?;
        for index in 0..count {
            let element_input_offset =
                checked_offset(input_offset, checked_mul(index, stride, path)?, path)?;
            self.compile_node(
                element_shape,
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
    node: &WriterNode,
    input_offset: usize,
    output_offset: usize,
    path: &str,
) -> Result<FixedEncodeSegment, StencilError> {
    let mut compiler = FixedEncodeCompiler {
        ops: Vec::new(),
        output_offset,
    };
    compiler.compile_node(shape, node, input_offset, path)?;
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

fn checked_mul(lhs: usize, rhs: usize, path: &str) -> Result<usize, StencilError> {
    lhs.checked_mul(rhs)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset overflow",
        })
}
