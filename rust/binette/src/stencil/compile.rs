use super::*;
use crate::hash::primitive_type_id;
use crate::local_access::{
    LocalAccess, LocalEnumTagThunks, LocalFieldDescriptor, LocalOptionEncodeThunks,
    LocalOptionRepresentation, LocalOptionSequenceDecodeThunks, LocalScalarAccess, LocalSchemaRef,
    LocalSequenceDecodeThunks, LocalSequenceElementPtrEncodeThunks, LocalSequenceEncodeThunks,
    LocalSequenceFixedDecodeThunks, LocalSequenceStorage, LocalThunkBindings, LocalTypeDescriptor,
    LocalTypeKind, LocalVariantConstructThunks, LocalVariantProjectIntoThunks,
    LocalVariantProjectThunks,
};
use crate::schema::TypeRef;

pub(super) struct StencilCompiler<'registry> {
    pub(super) writer_registry: &'registry SchemaRegistry,
    pub(super) input_offset: usize,
}

impl StencilCompiler<'_> {
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

    fn compile_primitive_skip(
        &mut self,
        primitive: Primitive,
        path: &str,
    ) -> Result<(), StencilError> {
        let Some(widths) = primitive_widths(primitive) else {
            return Err(unsupported_primitive(path, primitive));
        };
        for width in widths {
            self.input_offset = checked_offset(self.input_offset, width.bytes(), path)?;
        }
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

pub(super) struct LocalDecodeStencilCompiler<'registry> {
    pub(super) writer_registry: &'registry SchemaRegistry,
    pub(super) plan_nodes: &'registry [PlanNode],
    pub(super) ops: Vec<StencilOp>,
    pub(super) failures: Vec<StencilFailure>,
    pub(super) input_offset: usize,
}

impl LocalDecodeStencilCompiler<'_> {
    // r[impl binette.local-access.descriptor+2]
    pub(super) fn compile_root(
        &mut self,
        reader: &LocalTypeDescriptor,
        root: &PlanNode,
    ) -> Result<LengthCheck, StencilError> {
        if let PlanNode::Enum { variants } = root {
            return self.compile_enum_root(reader, variants, "$");
        }
        if let PlanNode::Option { element } = root {
            return self.compile_option_root(reader, element, "$");
        }
        self.compile_node(reader, root, 0, "$")?;
        Ok(LengthCheck::Exact(self.input_offset))
    }

    fn compile_node(
        &mut self,
        reader: &LocalTypeDescriptor,
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
                self.compile_node(reader, node, output_offset, path)
            }
            PlanNode::Primitive { primitive } => {
                self.compile_primitive_read(reader, *primitive, output_offset, path)
            }
            PlanNode::Struct { fields } => self.compile_struct(reader, fields, output_offset, path),
            PlanNode::Tuple { elements } => {
                self.compile_tuple(reader, elements, output_offset, path)
            }
            PlanNode::Array {
                dimensions,
                element,
            } => self.compile_array(reader, dimensions, element, output_offset, path),
            PlanNode::External { .. } => {
                let LocalTypeKind::ExternalAttachment { .. } = &reader.kind else {
                    return Err(StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "local descriptor is not an external attachment",
                    });
                };
                Ok(())
            }
            PlanNode::Enum { .. }
            | PlanNode::List { .. }
            | PlanNode::Set { .. }
            | PlanNode::Map { .. }
            | PlanNode::Option { .. }
            | PlanNode::Dynamic => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct local decode stencil only supports fixed-width roots",
            }),
        }
    }

    fn compile_option_root(
        &mut self,
        reader: &LocalTypeDescriptor,
        element: &PlanNode,
        path: &str,
    ) -> Result<LengthCheck, StencilError> {
        if self.input_offset != 0 || !self.ops.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local direct option decode only supports options at the root",
            });
        }

        let option_parts = local_direct_option_parts(reader, path)?;
        let input_offset = self.input_offset;
        self.input_offset = checked_offset(self.input_offset, 1, path)?;
        let invalid_failure_index = self.failures.len();
        self.failures.push(StencilFailure::InvalidOptionTag {
            path: path.to_owned(),
            position: input_offset,
        });

        let (body, expected_some) = self.compile_branch_body(input_offset + 1, |compiler| {
            compiler.compile_node(
                option_parts.some,
                element,
                option_parts.some_offset,
                &format!("{path}.some"),
            )
        })?;

        self.ops.push(StencilOp::RootOption {
            input_offset,
            tag_output_offset: option_parts.tag_offset,
            tag_output_width: option_parts.tag_width,
            none_value: option_parts.none_value,
            some_value: option_parts.some_value,
            body,
            invalid_failure_index,
        });

        Ok(LengthCheck::RootU8Tag {
            position: input_offset,
            cases: vec![
                ByteTaggedLength {
                    tag: STENCIL_OPTION_NONE as u8,
                    expected: input_offset + 1,
                },
                ByteTaggedLength {
                    tag: STENCIL_OPTION_SOME as u8,
                    expected: expected_some,
                },
            ],
        })
    }

    // r[impl binette.compat.enum]
    // r[impl binette.compat.enum.payload]
    fn compile_enum_root(
        &mut self,
        reader: &LocalTypeDescriptor,
        variants: &[EnumVariantPlan],
        path: &str,
    ) -> Result<LengthCheck, StencilError> {
        if self.input_offset != 0 || !self.ops.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local direct enum decode only supports enums at the root",
            });
        }

        let (tag_output_offset, local_variants) = local_enum_direct_tag_variants(reader, path)?;

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
                    let local_variant = local_reader_variant_descriptor(
                        local_variants,
                        *reader_index,
                        name,
                        path,
                        "reader enum variant index is out of range",
                    )?;
                    let reader_discriminant = u8::try_from(local_variant.index).map_err(|_| {
                        StencilError::Unsupported {
                            path: format!("{path}.{name}"),
                            reason: "local enum variant index does not fit u8 tag",
                        }
                    })?;
                    let (body, expected) =
                        self.compile_branch_body(input_offset + 4, |compiler| {
                            compiler.compile_enum_payload(
                                local_variant,
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
            tag_output_offset,
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

    fn compile_enum_payload(
        &mut self,
        local_variant: &crate::local_access::LocalVariantDescriptor,
        payload: &EnumPayloadPlan,
        path: &str,
    ) -> Result<(), StencilError> {
        match payload {
            EnumPayloadPlan::Unit => Ok(()),
            EnumPayloadPlan::Newtype(element) => {
                let payload_descriptor =
                    local_variant
                        .payload
                        .as_deref()
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "newtype enum payload is missing local descriptor",
                        })?;
                let payload_offset = local_direct_offset(&local_variant.access, path)?;
                self.compile_node(payload_descriptor, element, payload_offset, path)
            }
            EnumPayloadPlan::Tuple(elements) => {
                let descriptor_fields = local_struct_fields(
                    local_variant
                        .payload
                        .as_deref()
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "tuple enum payload is missing local descriptor",
                        })?,
                    path,
                )?;
                if descriptor_fields.len() != elements.len() {
                    return Err(StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "tuple enum payload arity differs from local descriptor",
                    });
                }
                for (index, element) in elements.iter().enumerate() {
                    let field_descriptor = &descriptor_fields[index];
                    self.compile_node(
                        &field_descriptor.descriptor,
                        element,
                        local_direct_offset(&field_descriptor.access, path)?,
                        &format!("{path}.{index}"),
                    )?;
                }
                Ok(())
            }
            EnumPayloadPlan::Struct(fields) => {
                let descriptor_fields = local_struct_fields(
                    local_variant
                        .payload
                        .as_deref()
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "struct enum payload is missing local descriptor",
                        })?,
                    path,
                )?;
                for field in fields {
                    match field {
                        StructFieldPlan::Read {
                            reader_index,
                            name,
                            plan,
                            ..
                        } => {
                            let field_descriptor = local_reader_field_descriptor(
                                descriptor_fields,
                                *reader_index,
                                name,
                                path,
                                "reader enum struct field index is out of range",
                            )?;
                            self.compile_node(
                                &field_descriptor.descriptor,
                                plan,
                                local_direct_offset(&field_descriptor.access, path)?,
                                &format!("{path}.{name}"),
                            )?;
                        }
                        StructFieldPlan::Skip {
                            writer_type, name, ..
                        } => self.compile_skip(writer_type, &format!("{path}.{name}"))?,
                        StructFieldPlan::Default { name, .. } => {
                            return Err(StencilError::Unsupported {
                                path: format!("{path}.{name}"),
                                reason: "default-filled enum fields require backend construction support",
                            });
                        }
                    }
                }
                Ok(())
            }
        }
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct(
        &mut self,
        reader: &LocalTypeDescriptor,
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(reader, path)?;
        for field in fields {
            match field {
                StructFieldPlan::Read {
                    reader_index,
                    name,
                    plan,
                    ..
                } => {
                    let field_descriptor = local_reader_field_descriptor(
                        descriptor_fields,
                        *reader_index,
                        name,
                        path,
                        "reader descriptor field index is out of range",
                    )?;
                    let field_offset = checked_offset(
                        output_offset,
                        local_direct_offset(&field_descriptor.access, path)?,
                        path,
                    )?;
                    self.compile_node(
                        &field_descriptor.descriptor,
                        plan,
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
                        reason: "default-filled reader fields require backend construction support",
                    });
                }
            }
        }
        Ok(())
    }

    fn compile_tuple(
        &mut self,
        reader: &LocalTypeDescriptor,
        elements: &[PlanNode],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(reader, path)?;
        if descriptor_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader tuple arity differs from local descriptor",
            });
        }
        for (index, element) in elements.iter().enumerate() {
            let element_descriptor = &descriptor_fields[index];
            let element_offset = checked_offset(
                output_offset,
                local_direct_offset(&element_descriptor.access, path)?,
                path,
            )?;
            self.compile_node(
                &element_descriptor.descriptor,
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
        reader: &LocalTypeDescriptor,
        dimensions: &[u64],
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let expected_count = dimensions_element_count(dimensions, path)?;
        let (element_descriptor, element_count, element_stride) =
            local_inline_fixed_array(reader, path)?;
        if element_count != expected_count {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader array dimensions differ from local descriptor",
            });
        }
        for index in 0..element_count {
            let element_output_offset = checked_offset(
                output_offset,
                checked_mul(index, element_stride, path)?,
                path,
            )?;
            self.compile_node(
                element_descriptor,
                element,
                element_output_offset,
                &format!("{path}[{index}]"),
            )?;
        }
        Ok(())
    }

    fn compile_primitive_read(
        &mut self,
        reader: &LocalTypeDescriptor,
        primitive: Primitive,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        validate_descriptor_primitive(reader, primitive, path)?;
        if primitive == Primitive::Bool {
            return self.emit_bool(path, Some(output_offset));
        }

        let Some(widths) = primitive_widths(primitive) else {
            return Err(unsupported_primitive(path, primitive));
        };
        self.emit_primitive_copies(path, output_offset, widths)
    }

    fn compile_skip(&mut self, writer_type: &TypeRef, path: &str) -> Result<(), StencilError> {
        self.compile_skip_type(writer_type, path)
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
                reason: "local decode stencil only supports fixed-width skipped values",
            }),
        }
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
                reason: "local decode stencil does not support generic skipped type refs",
            }),
        }
    }

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

    fn emit_bool(&mut self, path: &str, output_offset: Option<usize>) -> Result<(), StencilError> {
        let input_offset = self.input_offset;
        let failure_index = self.failures.len();
        let _ = status_for_failure(failure_index)?;
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
}

pub(super) struct LocalHybridDecodeStencilCompiler<'registry, 'thunks> {
    pub(super) writer_registry: &'registry SchemaRegistry,
    pub(super) plan_nodes: &'registry [PlanNode],
    pub(super) ops: Vec<HybridStencilOp>,
    pub(super) helpers: Vec<StencilHelper>,
    pub(super) failures: Vec<StencilFailure>,
    pub(super) thunks: &'thunks LocalThunkBindings,
}

impl LocalHybridDecodeStencilCompiler<'_, '_> {
    // r[impl binette.local-access.descriptor+2]
    // r[impl binette.local-access.strict-hybrid]
    pub(super) fn compile_root(
        &mut self,
        reader: &LocalTypeDescriptor,
        root: &PlanNode,
    ) -> Result<(), StencilError> {
        self.compile_node(reader, root, 0, "$")
    }

    fn compile_node(
        &mut self,
        reader: &LocalTypeDescriptor,
        node: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        match fixed_local_copy_ops(
            self.writer_registry,
            self.plan_nodes,
            node,
            reader,
            output_offset,
            path,
        ) {
            Ok((ops, input_len)) => {
                if input_len == 0 && ops.is_empty() {
                    return Ok(());
                }
                let failure_index = self.push_helper_failure(path)?;
                self.ops.push(HybridStencilOp::Copy {
                    ops,
                    input_len,
                    failure_index,
                });
                return Ok(());
            }
            Err(StencilError::Unsupported { .. }) => {}
            Err(err) => return Err(err),
        }

        match node {
            PlanNode::Ref { node_index } => {
                let node =
                    self.plan_nodes
                        .get(*node_index)
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "recursive reader plan node reference is out of range",
                        })?;
                self.compile_node(reader, node, output_offset, path)
            }
            PlanNode::Primitive {
                primitive: primitive @ (Primitive::String | Primitive::Bytes | Primitive::Payload),
            } => self.push_sequence_bytes(reader, *primitive, output_offset, path),
            PlanNode::Primitive {
                primitive: Primitive::Bool,
            } => self.push_bool(reader, output_offset, path),
            PlanNode::Struct { fields } => self.compile_struct(reader, fields, output_offset, path),
            PlanNode::Tuple { elements } => {
                self.compile_tuple(reader, elements, output_offset, path)
            }
            PlanNode::Array {
                dimensions,
                element,
            } => self.compile_array(reader, dimensions, element, output_offset, path),
            PlanNode::Option { element } => {
                match self.push_direct_option_fixed(reader, element, output_offset, path) {
                    Ok(()) => Ok(()),
                    Err(StencilError::Unsupported { .. }) => {
                        self.push_option_sequence_bytes(reader, element, output_offset, path)
                    }
                    Err(err) => Err(err),
                }
            }
            PlanNode::External { .. } => Ok(()),
            PlanNode::List { element } => {
                self.push_sequence_fixed_elements(reader, element, output_offset, path)
            }
            PlanNode::Set { .. }
            | PlanNode::Map { .. }
            | PlanNode::Primitive { .. }
            | PlanNode::Dynamic => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local hybrid decode has no backend thunk for this subtree",
            }),
            PlanNode::Enum { variants } => {
                self.push_constructed_enum(reader, variants, output_offset, path)
            }
        }
    }

    fn push_bool(
        &mut self,
        reader: &LocalTypeDescriptor,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        validate_descriptor_primitive(reader, Primitive::Bool, path)?;
        let failure_index = self.push_helper_failure(path)?;
        self.ops.push(HybridStencilOp::Bool {
            output_offset,
            failure_index,
        });
        Ok(())
    }

    // r[impl binette.compat.field-matching]
    // r[impl binette.compat.skip-unknown]
    fn compile_struct(
        &mut self,
        reader: &LocalTypeDescriptor,
        fields: &[StructFieldPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(reader, path)?;
        for field in fields {
            match field {
                StructFieldPlan::Read {
                    reader_index,
                    name,
                    plan,
                    ..
                } => {
                    let field_path = format!("{path}.{name}");
                    let field_descriptor = local_reader_field_descriptor(
                        descriptor_fields,
                        *reader_index,
                        name,
                        path,
                        "reader descriptor field index is out of range",
                    )?;
                    let field_offset = checked_offset(
                        output_offset,
                        local_direct_offset(&field_descriptor.access, &field_path)?,
                        &field_path,
                    )?;
                    self.compile_node(
                        &field_descriptor.descriptor,
                        plan,
                        field_offset,
                        &field_path,
                    )?;
                }
                StructFieldPlan::Skip {
                    writer_type, name, ..
                } => self.push_fixed_skip(writer_type, &format!("{path}.{name}"))?,
                StructFieldPlan::Default { name, .. } => {
                    return Err(StencilError::Unsupported {
                        path: format!("{path}.{name}"),
                        reason: "default-filled reader fields require backend construction support",
                    });
                }
            }
        }
        Ok(())
    }

    fn compile_tuple(
        &mut self,
        reader: &LocalTypeDescriptor,
        elements: &[PlanNode],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(reader, path)?;
        if descriptor_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader tuple arity differs from local descriptor",
            });
        }
        for (index, element) in elements.iter().enumerate() {
            let element_path = format!("{path}.{index}");
            let element_descriptor = &descriptor_fields[index];
            let element_offset = checked_offset(
                output_offset,
                local_direct_offset(&element_descriptor.access, &element_path)?,
                &element_path,
            )?;
            self.compile_node(
                &element_descriptor.descriptor,
                element,
                element_offset,
                &element_path,
            )?;
        }
        Ok(())
    }

    // r[impl binette.aggregate.array]
    fn compile_array(
        &mut self,
        reader: &LocalTypeDescriptor,
        dimensions: &[u64],
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let expected_count = dimensions_element_count(dimensions, path)?;
        let (element_descriptor, element_count, element_stride) =
            local_inline_fixed_array(reader, path)?;
        if element_count != expected_count {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "reader array dimensions differ from local descriptor",
            });
        }
        for index in 0..element_count {
            let element_output_offset = checked_offset(
                output_offset,
                checked_mul(index, element_stride, path)?,
                path,
            )?;
            self.compile_node(
                element_descriptor,
                element,
                element_output_offset,
                &format!("{path}[{index}]"),
            )?;
        }
        Ok(())
    }

    fn push_sequence_bytes(
        &mut self,
        reader: &LocalTypeDescriptor,
        primitive: Primitive,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        if let Some(layout) = local_direct_byte_sequence_decode_layout(reader, primitive, path)? {
            let failure_index = self.push_helper_failure(path)?;
            let helper_index = self.helpers.len();
            self.helpers.push(StencilHelper::DirectSequenceBytes {
                output_offset,
                layout,
                primitive,
                failure_index,
            });
            self.ops.push(HybridStencilOp::Helper { helper_index });
            return Ok(());
        }

        let thunks = local_sequence_decode_thunks(reader, primitive, self.thunks, path)?;
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::SequenceBytes {
            output_offset,
            thunks,
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_sequence_fixed_elements(
        &mut self,
        reader: &LocalTypeDescriptor,
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        if let Some((element_descriptor, layout)) =
            local_direct_sequence_decode_layout(reader, path)?
        {
            let (element_ops, element_input_len) = fixed_local_decode_ops(
                self.writer_registry,
                self.plan_nodes,
                element,
                element_descriptor,
                0,
                &format!("{path}[]"),
            )?;
            let failure_index = self.push_helper_failure(path)?;
            let helper_index = self.helpers.len();
            self.helpers
                .push(StencilHelper::DirectSequenceFixedElements {
                    output_offset,
                    layout,
                    element_ops,
                    element_input_len,
                    failure_index,
                });
            self.ops.push(HybridStencilOp::Helper { helper_index });
            return Ok(());
        }

        let (element_descriptor, element_stride, thunks) =
            local_sequence_fixed_decode_thunks(reader, self.thunks, path)?;
        let (element_ops, element_input_len) = fixed_local_decode_ops(
            self.writer_registry,
            self.plan_nodes,
            element,
            element_descriptor,
            0,
            &format!("{path}[]"),
        )?;
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::SequenceFixedElements {
            output_offset,
            thunks,
            element_ops,
            element_input_len,
            element_stride,
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_constructed_enum(
        &mut self,
        reader: &LocalTypeDescriptor,
        variants: &[EnumVariantPlan],
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let local_variants = local_enum_construct_variants(reader, path)?;
        let mut cases = Vec::with_capacity(variants.len());
        for variant in variants {
            let EnumVariantPlan::Read {
                writer_index,
                reader_index,
                name,
                payload,
            } = variant
            else {
                continue;
            };
            let local_variant = local_reader_variant_descriptor(
                local_variants,
                *reader_index,
                name,
                path,
                "reader enum variant index is out of range",
            )?;
            let construct_thunks = local_variant_construct_thunks(
                local_variant,
                self.thunks,
                &format!("{path}.{name}"),
            )?;
            let payload = self.compile_constructed_enum_payload(
                local_variant.payload.as_deref(),
                payload,
                &format!("{path}.{name}"),
            )?;
            cases.push(LocalEnumDecodeCase {
                wire_index: *writer_index,
                construct_thunks,
                payload,
            });
        }
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::Enum {
            output_offset,
            cases,
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn compile_constructed_enum_payload(
        &self,
        local_payload: Option<&LocalTypeDescriptor>,
        payload: &EnumPayloadPlan,
        path: &str,
    ) -> Result<LocalEnumDecodePayload, StencilError> {
        match payload {
            EnumPayloadPlan::Unit => Ok(LocalEnumDecodePayload::Unit),
            EnumPayloadPlan::Newtype(element) => {
                if let PlanNode::Primitive {
                    primitive:
                        primitive @ (Primitive::String | Primitive::Bytes | Primitive::Payload),
                } = &**element
                {
                    let local_payload = local_payload.ok_or_else(|| StencilError::Unsupported {
                        path: path.to_owned(),
                        reason: "reader enum byte-sequence payload is missing local descriptor",
                    })?;
                    local_byte_sequence_descriptor(local_payload, *primitive, path)?;
                    return Ok(LocalEnumDecodePayload::SequenceBytes);
                }
                let local_payload = local_payload.ok_or_else(|| StencilError::Unsupported {
                    path: path.to_owned(),
                    reason: "reader enum payload is missing local descriptor",
                })?;
                let (ops, input_len) = fixed_local_decode_ops(
                    self.writer_registry,
                    self.plan_nodes,
                    element,
                    local_payload,
                    0,
                    path,
                )?;
                Ok(LocalEnumDecodePayload::Fixed {
                    ops,
                    input_len,
                    local_size: local_payload.layout.size,
                })
            }
            EnumPayloadPlan::Tuple(_) | EnumPayloadPlan::Struct(_) => {
                Err(StencilError::Unsupported {
                    path: path.to_owned(),
                    reason: "local constructed enum helper currently supports unit and newtype payloads",
                })
            }
        }
    }

    fn push_fixed_skip(&mut self, writer_type: &TypeRef, path: &str) -> Result<(), StencilError> {
        let input_len = match fixed_skip_len(self.writer_registry, writer_type, path) {
            Ok(input_len) => input_len,
            Err(StencilError::Unsupported { .. }) => {
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

    fn push_option_sequence_bytes(
        &mut self,
        reader: &LocalTypeDescriptor,
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let PlanNode::Primitive {
            primitive: primitive @ (Primitive::String | Primitive::Bytes | Primitive::Payload),
        } = element
        else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local option thunk decode currently supports byte-sequence payloads",
            });
        };
        if let Some((option, sequence)) =
            local_direct_option_sequence_bytes_decode_layout(reader, *primitive, path)?
        {
            let failure_index = self.push_helper_failure(path)?;
            let helper_index = self.helpers.len();
            self.helpers.push(StencilHelper::DirectOptionSequenceBytes {
                output_offset,
                option,
                sequence,
                primitive: *primitive,
                failure_index,
            });
            self.ops.push(HybridStencilOp::Helper { helper_index });
            return Ok(());
        }

        let thunks =
            local_option_sequence_bytes_decode_thunks(reader, *primitive, self.thunks, path)?;
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::OptionSequenceBytes {
            output_offset,
            thunks,
            failure_index,
        });
        self.ops.push(HybridStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_direct_option_fixed(
        &mut self,
        reader: &LocalTypeDescriptor,
        element: &PlanNode,
        output_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let option = local_direct_option_parts(reader, path)?;
        let (element_ops, element_input_len) = fixed_local_decode_ops(
            self.writer_registry,
            self.plan_nodes,
            element,
            option.some,
            0,
            &format!("{path}.some"),
        )?;
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilHelper::DirectOptionFixed {
            output_offset,
            option: DirectOptionDecodeLayout {
                tag_offset: option.tag_offset,
                tag_width: option.tag_width,
                none_value: option.none_value,
                none_bytes: option.none_bytes.map(Vec::from),
                some_value: option.some_value,
                some_offset: option.some_offset,
                option_size: option.option_size,
            },
            element_ops,
            element_input_len,
            element_output_len: option.some.layout.size,
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

pub(super) struct LocalEncodeStencilCompiler<'a> {
    pub(super) ops: Vec<EncodeStencilOp>,
    pub(super) helpers: Vec<StencilEncodeHelper>,
    pub(super) failures: Vec<StencilFailure>,
    pub(super) thunks: &'a LocalThunkBindings,
}

impl LocalEncodeStencilCompiler<'_> {
    // r[impl binette.local-access.descriptor+2]
    // r[impl binette.local-access.strict-hybrid]
    pub(super) fn compile_root(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        root: &WriterNode,
    ) -> Result<(), StencilError> {
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        self.compile_node(descriptor, root, 0, "$", &mut pending)?;
        self.flush_direct_segment(&mut pending);
        Ok(())
    }

    fn compile_node(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        node: &WriterNode,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        match fixed_descriptor_encode_segment(
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
                match fixed_descriptor_encode_segment(descriptor, node, input_offset, 0, path) {
                    Ok(segment) if copy_ops_fit_direct_code(&segment.ops) => {
                        self.push_direct_segment(segment);
                        return Ok(());
                    }
                    Ok(_) => {
                        return Err(StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "local direct encode segment exceeds first stencil immediate range",
                        });
                    }
                    Err(StencilError::Unsupported { .. }) => {}
                    Err(err) => return Err(err),
                }
            }
            Err(StencilError::Unsupported { .. }) => {}
            Err(err) => return Err(err),
        }

        match node {
            WriterNode::Primitive(
                primitive @ (Primitive::String | Primitive::Bytes | Primitive::Payload),
            ) => {
                self.flush_direct_segment(pending);
                self.push_sequence_bytes(descriptor, *primitive, input_offset, path)
            }
            WriterNode::Struct { fields } => {
                self.compile_struct(descriptor, fields, input_offset, path, pending)
            }
            WriterNode::Tuple { elements } => {
                self.compile_tuple(descriptor, elements, input_offset, path, pending)
            }
            WriterNode::Array {
                dimensions,
                element,
            } => self.compile_array(descriptor, dimensions, element, input_offset, path, pending),
            WriterNode::Option { element } => {
                self.flush_direct_segment(pending);
                match self.push_direct_option(descriptor, element, input_offset, path) {
                    Ok(()) => Ok(()),
                    Err(StencilError::Unsupported { .. }) => {
                        self.push_option_sequence_bytes(descriptor, element, input_offset, path)
                    }
                    Err(err) => Err(err),
                }
            }
            WriterNode::External => Ok(()),
            WriterNode::Ref { .. }
            | WriterNode::Result { .. }
            | WriterNode::Set { .. }
            | WriterNode::Map { .. }
            | WriterNode::Primitive(_)
            | WriterNode::Dynamic => Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local hybrid encode has no backend thunk for this subtree",
            }),
            WriterNode::Enum { variants } => {
                self.flush_direct_segment(pending);
                match self.push_direct_enum(descriptor, variants, input_offset, path) {
                    Ok(()) => Ok(()),
                    Err(StencilError::Unsupported { .. }) => {
                        self.push_projected_enum(descriptor, variants, input_offset, path)
                    }
                    Err(err) => Err(err),
                }
            }
            WriterNode::List { element } => {
                self.flush_direct_segment(pending);
                match self.push_direct_list(descriptor, element, input_offset, path) {
                    Ok(()) => Ok(()),
                    Err(StencilError::Unsupported { .. }) => {
                        self.push_sequence_fixed_elements(descriptor, element, input_offset, path)
                    }
                    Err(err) => Err(err),
                }
            }
        }
    }

    fn compile_struct(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        fields: &[WriterFieldPlan],
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        for field in fields {
            let field_path = format!("{path}.{}", field.name);
            let field_descriptor = local_writer_field_descriptor(
                descriptor_fields,
                field,
                path,
                "writer descriptor field index is out of range",
            )?;
            let field_offset = checked_offset(
                input_offset,
                local_direct_offset(&field_descriptor.access, &field_path)?,
                &field_path,
            )?;
            self.compile_node(
                &field_descriptor.descriptor,
                &field.node,
                field_offset,
                &field_path,
                pending,
            )?;
        }
        Ok(())
    }

    fn compile_tuple(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        elements: &[WriterTupleElementPlan],
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
    ) -> Result<(), StencilError> {
        let descriptor_fields = local_struct_fields(descriptor, path)?;
        if descriptor_fields.len() != elements.len() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "writer tuple arity differs from local descriptor",
            });
        }
        for element in elements {
            let element_path = format!("{path}.{}", element.local_index);
            let element_descriptor =
                descriptor_fields.get(element.local_index).ok_or_else(|| {
                    StencilError::Unsupported {
                        path: element_path.clone(),
                        reason: "writer tuple descriptor field index is out of range",
                    }
                })?;
            let element_offset = checked_offset(
                input_offset,
                local_direct_offset(&element_descriptor.access, &element_path)?,
                &element_path,
            )?;
            self.compile_node(
                &element_descriptor.descriptor,
                &element.node,
                element_offset,
                &element_path,
                pending,
            )?;
        }
        Ok(())
    }

    fn compile_array(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        dimensions: &[u64],
        element: &WriterNode,
        input_offset: usize,
        path: &str,
        pending: &mut FixedEncodeSegment,
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
            self.compile_node(
                element_descriptor,
                element,
                element_input_offset,
                &format!("{path}[{index}]"),
                pending,
            )?;
        }
        Ok(())
    }

    fn push_sequence_bytes(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        primitive: Primitive,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        if let Some((kind, ptr_offset, len_offset)) =
            local_direct_byte_sequence(descriptor, primitive, path)?
        {
            self.ops.push(EncodeStencilOp::Bytes {
                input_offset,
                kind,
                layout: EncodeBytesLayout::Direct {
                    ptr_offset,
                    len_offset,
                },
            });
            return Ok(());
        }

        let thunks = local_sequence_bytes_thunks(descriptor, primitive, self.thunks, path)?;
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilEncodeHelper::SequenceBytes {
            input_offset,
            thunks,
            failure_index,
        });
        self.ops.push(EncodeStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_sequence_fixed_elements(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        element: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (element_descriptor, thunks) =
            local_sequence_element_ptr_thunks(descriptor, self.thunks, path)?;
        let segment = fixed_descriptor_encode_segment(
            element_descriptor,
            element,
            0,
            0,
            &format!("{path}[]"),
        )?;
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers
            .push(StencilEncodeHelper::SequenceFixedElements {
                input_offset,
                thunks,
                element_ops: segment.ops,
                element_output_len: segment.output_len,
                failure_index,
            });
        self.ops.push(EncodeStencilOp::Helper { helper_index });
        Ok(())
    }

    fn push_direct_option(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        element: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let option_parts = local_direct_option_parts(descriptor, path)?;
        let mut compiler = LocalEncodeStencilCompiler {
            ops: Vec::new(),
            helpers: Vec::new(),
            failures: Vec::new(),
            thunks: self.thunks,
        };
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        compiler.compile_node(
            option_parts.some,
            element,
            0,
            &format!("{path}.some"),
            &mut pending,
        )?;
        compiler.flush_direct_segment(&mut pending);
        if !compiler.helpers.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct local option payload requires helper fallback",
            });
        }

        self.ops.push(EncodeStencilOp::Option {
            input_offset,
            layout: EncodeOptionLayout::DirectTag {
                tag_offset: option_parts.tag_offset,
                tag_width: option_parts.tag_width,
                none_value: option_parts.none_value,
                some_offset: option_parts.some_offset,
            },
            some_ops: compiler.ops,
        });
        Ok(())
    }

    fn push_direct_list(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        element: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (element_descriptor, element_stride) = local_sequence_element(descriptor, path)?;
        let LocalSequenceStorage::DirectContiguous {
            pointer: LocalAccess::Direct { offset: ptr_offset },
            length: LocalAccess::Direct { offset: len_offset },
            ..
        } = local_sequence_storage(descriptor, path)?
        else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local sequence descriptor does not use direct contiguous storage",
            });
        };

        let mut compiler = LocalEncodeStencilCompiler {
            ops: Vec::new(),
            helpers: Vec::new(),
            failures: Vec::new(),
            thunks: self.thunks,
        };
        let mut pending = FixedEncodeSegment {
            ops: Vec::new(),
            output_len: 0,
        };
        compiler.compile_node(
            element_descriptor,
            element,
            0,
            &format!("{path}[]"),
            &mut pending,
        )?;
        compiler.flush_direct_segment(&mut pending);
        if !compiler.helpers.is_empty() {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "direct local list element requires helper fallback",
            });
        }

        self.ops.push(EncodeStencilOp::List {
            input_offset,
            layout: EncodeListLayout::Vec {
                ptr_offset: *ptr_offset,
                len_offset: *len_offset,
                element_stride,
            },
            element_ops: compiler.ops,
        });
        Ok(())
    }

    fn push_direct_enum(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        variants: &[WriterVariantPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (tag_offset, local_variants) = local_enum_direct_tag_variants(descriptor, path)?;
        let mut cases = Vec::with_capacity(variants.len());
        for variant in variants {
            let local_variant = local_variants.get(variant.local_index).ok_or_else(|| {
                StencilError::Unsupported {
                    path: path.to_owned(),
                    reason: "writer enum variant index is out of range",
                }
            })?;
            let variant_path = format!("{path}.{}", local_variant.name);
            let ops = self.compile_direct_enum_payload(
                local_variant,
                &variant.payload,
                input_offset,
                &variant_path,
            )?;
            cases.push(EncodeEnumCase {
                local_index: local_variant.index,
                wire_index: variant.wire_index,
                ops,
            });
        }

        self.ops.push(EncodeStencilOp::Enum {
            input_offset,
            selector: EncodeEnumSelector::DirectTag { offset: tag_offset },
            cases,
        });
        Ok(())
    }

    fn compile_direct_enum_payload(
        &mut self,
        local_variant: &crate::local_access::LocalVariantDescriptor,
        payload: &WriterVariantPayloadPlan,
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
                    let payload_descriptor = local_variant.payload.as_deref().ok_or_else(|| {
                        StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "writer enum newtype payload is missing local descriptor",
                        }
                    })?;
                    let payload_offset = checked_offset(
                        input_offset,
                        local_direct_offset(&local_variant.access, path)?,
                        path,
                    )?;
                    self.compile_node(
                        payload_descriptor,
                        &element.node,
                        payload_offset,
                        path,
                        &mut pending,
                    )?;
                }
                WriterVariantPayloadPlan::Tuple(elements) => {
                    let descriptor_fields = local_struct_fields(
                        local_variant.payload.as_deref().ok_or_else(|| {
                            StencilError::Unsupported {
                                path: path.to_owned(),
                                reason: "writer enum tuple payload is missing local descriptor",
                            }
                        })?,
                        path,
                    )?;
                    for element in elements {
                        let field_descriptor = descriptor_fields
                            .get(element.local_index)
                            .ok_or_else(|| StencilError::Unsupported {
                                path: path.to_owned(),
                                reason: "writer enum tuple descriptor field index is out of range",
                            })?;
                        let field_offset = checked_offset(
                            input_offset,
                            local_direct_offset(&field_descriptor.access, path)?,
                            path,
                        )?;
                        self.compile_node(
                            &field_descriptor.descriptor,
                            &element.node,
                            field_offset,
                            &format!("{path}.{}", element.local_index),
                            &mut pending,
                        )?;
                    }
                }
                WriterVariantPayloadPlan::Struct(fields) => {
                    let descriptor_fields = local_struct_fields(
                        local_variant.payload.as_deref().ok_or_else(|| {
                            StencilError::Unsupported {
                                path: path.to_owned(),
                                reason: "writer enum struct payload is missing local descriptor",
                            }
                        })?,
                        path,
                    )?;
                    for field in fields {
                        let field_path = format!("{path}.{}", field.name);
                        let field_descriptor = local_writer_field_descriptor(
                            descriptor_fields,
                            field,
                            path,
                            "writer enum struct descriptor field index is out of range",
                        )?;
                        let field_offset = checked_offset(
                            input_offset,
                            local_direct_offset(&field_descriptor.access, &field_path)?,
                            &field_path,
                        )?;
                        self.compile_node(
                            &field_descriptor.descriptor,
                            &field.node,
                            field_offset,
                            &field_path,
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

    fn push_projected_enum(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        variants: &[WriterVariantPlan],
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let (tag_thunks, local_variants) = local_enum_tag_thunks(descriptor, self.thunks, path)?;
        let mut cases = Vec::with_capacity(variants.len());
        for variant in variants {
            let local_variant = local_variants.get(variant.local_index).ok_or_else(|| {
                StencilError::Unsupported {
                    path: path.to_owned(),
                    reason: "writer enum variant index is out of range",
                }
            })?;
            let variant_path = format!("{path}.{}", local_variant.name);
            let payload = self.compile_projected_enum_payload(
                local_variant,
                &variant.payload,
                &variant_path,
            )?;
            cases.push(LocalEnumEncodeCase {
                local_index: local_variant.index,
                wire_index: variant.wire_index,
                payload,
            });
        }
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilEncodeHelper::Enum {
            input_offset,
            tag_thunks,
            cases,
            failure_index,
        });
        self.ops.push(EncodeStencilOp::Helper { helper_index });
        Ok(())
    }

    fn compile_projected_enum_payload(
        &self,
        local_variant: &crate::local_access::LocalVariantDescriptor,
        payload: &WriterVariantPayloadPlan,
        path: &str,
    ) -> Result<LocalEnumEncodePayload, StencilError> {
        match payload {
            WriterVariantPayloadPlan::Unit => Ok(LocalEnumEncodePayload::Unit),
            WriterVariantPayloadPlan::Newtype(element) => {
                let payload_descriptor =
                    local_variant
                        .payload
                        .as_deref()
                        .ok_or_else(|| StencilError::Unsupported {
                            path: path.to_owned(),
                            reason: "writer enum payload is missing local descriptor",
                        })?;
                if let WriterNode::Primitive(
                    primitive @ (Primitive::String | Primitive::Bytes | Primitive::Payload),
                ) = element.node
                {
                    let thunks = local_sequence_bytes_thunks(
                        payload_descriptor,
                        primitive,
                        self.thunks,
                        path,
                    )?;
                    if let Some(project_into_thunks) =
                        local_variant_project_into_thunks(local_variant, self.thunks, path)?
                    {
                        return Ok(LocalEnumEncodePayload::OwnedSequenceBytes {
                            project_into_thunks,
                            payload_layout: payload_descriptor.layout,
                            thunks,
                        });
                    }
                    let project_thunks =
                        local_variant_project_thunks(&local_variant.access, self.thunks, path)?;
                    return Ok(LocalEnumEncodePayload::SequenceBytes {
                        project_thunks,
                        thunks,
                    });
                }
                let segment =
                    fixed_descriptor_encode_segment(payload_descriptor, &element.node, 0, 0, path)?;
                if let Some(project_into_thunks) =
                    local_variant_project_into_thunks(local_variant, self.thunks, path)?
                {
                    return Ok(LocalEnumEncodePayload::OwnedFixed {
                        project_into_thunks,
                        payload_layout: payload_descriptor.layout,
                        ops: segment.ops,
                        output_len: segment.output_len,
                    });
                }
                let project_thunks =
                    local_variant_project_thunks(&local_variant.access, self.thunks, path)?;
                Ok(LocalEnumEncodePayload::Fixed {
                    project_thunks,
                    ops: segment.ops,
                    output_len: segment.output_len,
                })
            }
            WriterVariantPayloadPlan::Tuple(_) | WriterVariantPayloadPlan::Struct(_) => {
                Err(StencilError::Unsupported {
                    path: path.to_owned(),
                    reason: "local projected enum helper currently supports unit and newtype payloads",
                })
            }
        }
    }

    fn push_option_sequence_bytes(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        element: &WriterNode,
        input_offset: usize,
        path: &str,
    ) -> Result<(), StencilError> {
        let WriterNode::Primitive(
            primitive @ (Primitive::String | Primitive::Bytes | Primitive::Payload),
        ) = element
        else {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local option thunk encode currently supports byte-sequence payloads",
            });
        };
        let (option_thunks, sequence_thunks) =
            local_option_sequence_bytes_encode_thunks(descriptor, *primitive, self.thunks, path)?;
        let failure_index = self.push_helper_failure(path)?;
        let helper_index = self.helpers.len();
        self.helpers.push(StencilEncodeHelper::OptionSequenceBytes {
            input_offset,
            option_thunks,
            sequence_thunks,
            failure_index,
        });
        self.ops.push(EncodeStencilOp::Helper { helper_index });
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
    // r[impl binette.local-access.descriptor+2]
    pub(super) fn compile_descriptor_root(
        &mut self,
        descriptor: &LocalTypeDescriptor,
        root: &WriterNode,
    ) -> Result<usize, StencilError> {
        self.compile_descriptor_node(descriptor, root, 0, "$")?;
        Ok(self.output_offset)
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
            let field_descriptor = local_writer_field_descriptor(
                descriptor_fields,
                field,
                path,
                "writer descriptor field index is out of range",
            )?;
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
                descriptor_fields.get(element.local_index).ok_or_else(|| {
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
                &format!("{path}.{}", element.local_index),
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

fn fixed_descriptor_encode_segment(
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
    compiler.compile_descriptor_node(descriptor, node, input_offset, path)?;
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
    validate_descriptor_schema_type(
        descriptor,
        &expected,
        path,
        "local descriptor primitive schema differs from writer primitive",
    )
}

fn validate_descriptor_schema_type(
    descriptor: &LocalTypeDescriptor,
    expected: &TypeRef,
    path: &str,
    reason: &'static str,
) -> Result<(), StencilError> {
    match &descriptor.schema {
        LocalSchemaRef::Type(type_ref) if type_ref == expected => Ok(()),
        _ => Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason,
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

fn local_writer_field_descriptor<'a>(
    descriptor_fields: &'a [LocalFieldDescriptor],
    field: &WriterFieldPlan,
    path: &str,
    missing_reason: &'static str,
) -> Result<&'a LocalFieldDescriptor, StencilError> {
    let field_path = format!("{path}.{}", field.name);
    let descriptor =
        descriptor_fields
            .get(field.local_index)
            .ok_or_else(|| StencilError::Unsupported {
                path: field_path.clone(),
                reason: missing_reason,
            })?;
    if descriptor.name != field.name {
        return Err(StencilError::Unsupported {
            path: field_path,
            reason: "local descriptor field name differs from writer plan",
        });
    }
    Ok(descriptor)
}

fn local_reader_field_descriptor<'a>(
    descriptor_fields: &'a [LocalFieldDescriptor],
    reader_index: usize,
    name: &str,
    path: &str,
    missing_reason: &'static str,
) -> Result<&'a LocalFieldDescriptor, StencilError> {
    let field_path = format!("{path}.{name}");
    let descriptor =
        descriptor_fields
            .get(reader_index)
            .ok_or_else(|| StencilError::Unsupported {
                path: field_path.clone(),
                reason: missing_reason,
            })?;
    if descriptor.name != name {
        return Err(StencilError::Unsupported {
            path: field_path,
            reason: "local descriptor field name differs from reader plan",
        });
    }
    Ok(descriptor)
}

fn local_reader_variant_descriptor<'a>(
    variants: &'a [crate::local_access::LocalVariantDescriptor],
    reader_index: usize,
    name: &str,
    path: &str,
    missing_reason: &'static str,
) -> Result<&'a crate::local_access::LocalVariantDescriptor, StencilError> {
    let variant_path = format!("{path}.{name}");
    let variant = variants
        .get(reader_index)
        .ok_or_else(|| StencilError::Unsupported {
            path: variant_path.clone(),
            reason: missing_reason,
        })?;
    if variant.name != name {
        return Err(StencilError::Unsupported {
            path: variant_path,
            reason: "local descriptor enum variant name differs from reader plan",
        });
    }
    Ok(variant)
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

struct LocalDirectOptionParts<'a> {
    some: &'a LocalTypeDescriptor,
    tag_offset: usize,
    tag_width: usize,
    none_value: usize,
    none_bytes: Option<&'a [u8]>,
    some_value: Option<usize>,
    some_offset: usize,
    option_size: usize,
}

fn local_direct_option_parts<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<LocalDirectOptionParts<'a>, StencilError> {
    let (some, representation) = local_option_descriptor(descriptor, path)?;
    let (tag, tag_width, none_value, none_bytes, some_value, some_access) = match representation {
        LocalOptionRepresentation::Tag {
            tag,
            tag_width,
            none_value,
            some_value,
            some,
        } => (tag, *tag_width, *none_value, None, Some(*some_value), some),
        LocalOptionRepresentation::Niche {
            tag,
            tag_width,
            none_value,
            none_bytes,
            some,
            ..
        } => (
            tag,
            *tag_width,
            *none_value,
            none_bytes.as_deref(),
            None,
            some,
        ),
        _ => {
            return Err(StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local option descriptor does not use a direct tag",
            });
        }
    };
    Ok(LocalDirectOptionParts {
        some,
        tag_offset: local_direct_offset(tag, path)?,
        tag_width,
        none_value,
        none_bytes,
        some_value,
        some_offset: local_direct_offset(some_access, &format!("{path}.some"))?,
        option_size: descriptor.layout.size,
    })
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

fn local_sequence_bytes_thunks(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<LocalSequenceEncodeThunks, StencilError> {
    let storage = local_byte_sequence_storage(
        descriptor,
        primitive,
        path,
        "local descriptor is not a thunk-backed byte sequence",
    )?;
    let LocalSequenceStorage::Thunk { len, element, .. } = storage else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local byte sequence descriptor does not use backend thunks",
        });
    };
    bindings
        .sequence_u8(len, element)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local byte sequence backend thunks are not bound",
        })
}

fn local_direct_byte_sequence(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    path: &str,
) -> Result<Option<(EncodeBytesKind, usize, usize)>, StencilError> {
    let kind = encode_bytes_kind_for_primitive(primitive, path)?;
    let storage = local_byte_sequence_storage(
        descriptor,
        primitive,
        path,
        "local descriptor is not a byte sequence",
    )?;
    let LocalSequenceStorage::DirectContiguous {
        pointer: LocalAccess::Direct { offset: ptr_offset },
        length: LocalAccess::Direct { offset: len_offset },
        ..
    } = storage
    else {
        return Ok(None);
    };
    Ok(Some((kind, *ptr_offset, *len_offset)))
}

fn local_sequence_element_ptr_thunks<'a>(
    descriptor: &'a LocalTypeDescriptor,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<(&'a LocalTypeDescriptor, LocalSequenceElementPtrEncodeThunks), StencilError> {
    let LocalTypeKind::Sequence { element, storage } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not a thunk-backed sequence",
        });
    };
    let LocalSequenceStorage::Thunk {
        len,
        element: element_thunk,
        ..
    } = storage
    else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local sequence descriptor does not use backend thunks",
        });
    };
    let thunks = bindings
        .sequence_element_ptr(len, element_thunk)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local sequence element pointer thunks are not bound",
        })?;
    Ok((element, thunks))
}

fn local_sequence_decode_thunks(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<LocalSequenceDecodeThunks, StencilError> {
    let storage = local_byte_sequence_storage(
        descriptor,
        primitive,
        path,
        "local descriptor is not a thunk-constructible byte sequence",
    )?;
    let LocalSequenceStorage::Thunk {
        write: Some(write), ..
    } = storage
    else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local byte sequence descriptor has no backend decode thunk",
        });
    };
    bindings
        .sequence_decode(write)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local byte sequence decode thunk is not bound",
        })
}

fn local_direct_byte_sequence_decode_layout(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    path: &str,
) -> Result<Option<DirectSequenceDecodeLayout>, StencilError> {
    let storage = local_byte_sequence_storage(
        descriptor,
        primitive,
        path,
        "local descriptor is not a byte sequence",
    )?;
    direct_contiguous_sequence_decode_layout(storage, 1, path)
}

fn local_direct_sequence_decode_layout<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<Option<(&'a LocalTypeDescriptor, DirectSequenceDecodeLayout)>, StencilError> {
    let LocalTypeKind::Sequence { element, storage } = &descriptor.kind else {
        return Ok(None);
    };
    let Some(layout) =
        direct_contiguous_sequence_decode_layout(storage, element.layout.align, path)?
    else {
        return Ok(None);
    };
    Ok(Some((element, layout)))
}

fn direct_contiguous_sequence_decode_layout(
    storage: &LocalSequenceStorage,
    element_align: usize,
    path: &str,
) -> Result<Option<DirectSequenceDecodeLayout>, StencilError> {
    let LocalSequenceStorage::DirectContiguous {
        pointer: LocalAccess::Direct { offset: ptr_offset },
        length: LocalAccess::Direct { offset: len_offset },
        capacity: Some(LocalAccess::Direct { offset: cap_offset }),
        element_stride,
    } = storage
    else {
        return Ok(None);
    };
    if element_align == 0 || !element_align.is_power_of_two() {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "direct sequence element alignment is not valid",
        });
    }
    Ok(Some(DirectSequenceDecodeLayout {
        ptr_offset: *ptr_offset,
        len_offset: *len_offset,
        cap_offset: *cap_offset,
        element_stride: *element_stride,
        element_align,
    }))
}

fn local_sequence_fixed_decode_thunks<'a>(
    descriptor: &'a LocalTypeDescriptor,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<
    (
        &'a LocalTypeDescriptor,
        usize,
        LocalSequenceFixedDecodeThunks,
    ),
    StencilError,
> {
    let LocalTypeKind::Sequence { element, storage } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not a thunk-constructible sequence",
        });
    };
    let LocalSequenceStorage::Thunk {
        write: Some(write), ..
    } = storage
    else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local sequence descriptor has no backend decode thunk",
        });
    };
    let thunks =
        bindings
            .sequence_fixed_decode(write)
            .ok_or_else(|| StencilError::Unsupported {
                path: path.to_owned(),
                reason: "local fixed-element sequence decode thunk is not bound",
            })?;
    Ok((element, element.layout.stride, thunks))
}

fn local_direct_option_sequence_bytes_decode_layout(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    path: &str,
) -> Result<Option<(DirectOptionDecodeLayout, DirectSequenceDecodeLayout)>, StencilError> {
    let (some, representation) = local_option_descriptor(descriptor, path)?;
    if !matches!(
        representation,
        LocalOptionRepresentation::Tag { .. } | LocalOptionRepresentation::Niche { .. }
    ) {
        return Ok(None);
    }
    let Some(sequence) =
        local_direct_byte_sequence_decode_layout(some, primitive, &format!("{path}.some"))?
    else {
        return Ok(None);
    };
    let option_parts = local_direct_option_parts(descriptor, path)?;
    if option_parts.some_value.is_none() && option_parts.none_bytes.is_none() {
        return Ok(None);
    }
    Ok(Some((
        DirectOptionDecodeLayout {
            tag_offset: option_parts.tag_offset,
            tag_width: option_parts.tag_width,
            none_value: option_parts.none_value,
            none_bytes: option_parts.none_bytes.map(<[u8]>::to_vec),
            some_value: option_parts.some_value,
            some_offset: option_parts.some_offset,
            option_size: option_parts.option_size,
        },
        sequence,
    )))
}

fn local_option_sequence_bytes_encode_thunks(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<(LocalOptionEncodeThunks, LocalSequenceEncodeThunks), StencilError> {
    let (some, representation) = local_option_descriptor(descriptor, path)?;
    let LocalOptionRepresentation::Thunk {
        is_some,
        some: some_thunk,
        ..
    } = representation
    else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local option descriptor does not use backend thunks",
        });
    };
    let option = bindings
        .option(is_some, some_thunk)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local option backend thunks are not bound",
        })?;
    let sequence = local_sequence_bytes_thunks(some, primitive, bindings, &format!("{path}.some"))?;
    Ok((option, sequence))
}

fn local_option_sequence_bytes_decode_thunks(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<LocalOptionSequenceDecodeThunks, StencilError> {
    let (some, representation) = local_option_descriptor(descriptor, path)?;
    local_byte_sequence_descriptor(some, primitive, &format!("{path}.some"))?;
    let LocalOptionRepresentation::Thunk {
        write_none: Some(write_none),
        write_some_bytes: Some(write_some_bytes),
        ..
    } = representation
    else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local option descriptor has no backend decode thunks",
        });
    };
    bindings
        .option_sequence_decode(write_none, write_some_bytes)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local option decode thunks are not bound",
        })
}

fn local_byte_sequence_descriptor(
    descriptor: &LocalTypeDescriptor,
    primitive: Primitive,
    path: &str,
) -> Result<(), StencilError> {
    let storage = local_byte_sequence_storage(
        descriptor,
        primitive,
        path,
        "local option byte-sequence payload is not a byte sequence",
    )?;
    if matches!(storage, LocalSequenceStorage::Thunk { .. }) {
        Ok(())
    } else {
        Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local option byte-sequence payload is not thunk-backed",
        })
    }
}

fn local_byte_sequence_storage<'a>(
    descriptor: &'a LocalTypeDescriptor,
    primitive: Primitive,
    path: &str,
    kind_mismatch_reason: &'static str,
) -> Result<&'a LocalSequenceStorage, StencilError> {
    validate_descriptor_schema_type(
        descriptor,
        &TypeRef::concrete(primitive_type_id(primitive)),
        path,
        "local descriptor byte-sequence schema differs from plan primitive",
    )?;

    match (primitive, &descriptor.kind) {
        (Primitive::String, LocalTypeKind::Scalar(LocalScalarAccess::String(storage))) => {
            Ok(storage)
        }
        (
            Primitive::Bytes | Primitive::Payload,
            LocalTypeKind::Scalar(LocalScalarAccess::Bytes(storage)),
        ) => Ok(storage),
        (
            Primitive::String | Primitive::Bytes | Primitive::Payload,
            LocalTypeKind::Scalar(LocalScalarAccess::String(_) | LocalScalarAccess::Bytes(_)),
        ) => Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor byte-sequence kind differs from plan primitive",
        }),
        _ => Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: kind_mismatch_reason,
        }),
    }
}

fn encode_bytes_kind_for_primitive(
    primitive: Primitive,
    path: &str,
) -> Result<EncodeBytesKind, StencilError> {
    match primitive {
        Primitive::String => Ok(EncodeBytesKind::String),
        Primitive::Bytes | Primitive::Payload => Ok(EncodeBytesKind::Bytes),
        _ => Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "plan primitive is not a byte sequence",
        }),
    }
}

fn local_enum_tag_thunks<'a>(
    descriptor: &'a LocalTypeDescriptor,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<
    (
        LocalEnumTagThunks,
        &'a [crate::local_access::LocalVariantDescriptor],
    ),
    StencilError,
> {
    let LocalTypeKind::Enum { tag, variants } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not an enum layout",
        });
    };
    let LocalAccess::Thunk(tag) = tag else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum descriptor does not use a backend tag thunk",
        });
    };
    let tag_thunks = bindings
        .enum_tag(tag)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum tag thunk is not bound",
        })?;
    Ok((tag_thunks, variants))
}

fn local_enum_direct_tag_variants<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<(usize, &'a [crate::local_access::LocalVariantDescriptor]), StencilError> {
    let LocalTypeKind::Enum { tag, variants } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not an enum layout",
        });
    };
    let LocalAccess::Direct { offset } = tag else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "strict local enum decode requires a direct tag access",
        });
    };
    Ok((*offset, variants))
}

fn local_enum_construct_variants<'a>(
    descriptor: &'a LocalTypeDescriptor,
    path: &str,
) -> Result<&'a [crate::local_access::LocalVariantDescriptor], StencilError> {
    let LocalTypeKind::Enum { variants, .. } = &descriptor.kind else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local descriptor is not an enum layout",
        });
    };
    Ok(variants)
}

fn local_variant_project_thunks(
    access: &LocalAccess,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<LocalVariantProjectThunks, StencilError> {
    let LocalAccess::Thunk(project) = access else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum variant descriptor does not use a backend projector thunk",
        });
    };
    bindings
        .variant_project(project)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum variant projector thunk is not bound",
        })
}

fn local_variant_project_into_thunks(
    variant: &crate::local_access::LocalVariantDescriptor,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<Option<LocalVariantProjectIntoThunks>, StencilError> {
    let Some(project_into) = &variant.project_into else {
        return Ok(None);
    };
    bindings
        .variant_project_into(project_into, variant.drop_projected.as_ref())
        .map(Some)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum variant owned projector thunk is not bound",
        })
}

fn local_variant_construct_thunks(
    variant: &crate::local_access::LocalVariantDescriptor,
    bindings: &LocalThunkBindings,
    path: &str,
) -> Result<LocalVariantConstructThunks, StencilError> {
    let construct = variant
        .construct
        .as_ref()
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum variant descriptor has no backend constructor thunk",
        })?;
    bindings
        .variant_construct(construct)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum variant constructor thunk is not bound",
        })
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
            StencilOp::Bool { .. } | StencilOp::RootEnum { .. } | StencilOp::RootOption { .. } => {
                None
            }
        })
        .collect()
}

fn fixed_local_copy_ops(
    writer_registry: &SchemaRegistry,
    plan_nodes: &[PlanNode],
    node: &PlanNode,
    reader_descriptor: &LocalTypeDescriptor,
    output_offset: usize,
    path: &str,
) -> Result<(Vec<CopyOp>, usize), StencilError> {
    let mut compiler = LocalDecodeStencilCompiler {
        writer_registry,
        plan_nodes,
        ops: Vec::new(),
        failures: Vec::new(),
        input_offset: 0,
    };
    compiler.compile_node(reader_descriptor, node, output_offset, path)?;
    if !compiler.failures.is_empty() {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local hybrid decode currently supports only infallible fixed-copy stencils",
        });
    }
    let Some(ops) = copy_ops_from_stencil_ops(&compiler.ops) else {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local hybrid decode currently supports only fixed-copy stencils",
        });
    };
    Ok((ops, compiler.input_offset))
}

fn fixed_local_decode_ops(
    writer_registry: &SchemaRegistry,
    plan_nodes: &[PlanNode],
    node: &PlanNode,
    reader_descriptor: &LocalTypeDescriptor,
    output_offset: usize,
    path: &str,
) -> Result<(Vec<StencilOp>, usize), StencilError> {
    let mut compiler = LocalDecodeStencilCompiler {
        writer_registry,
        plan_nodes,
        ops: Vec::new(),
        failures: Vec::new(),
        input_offset: 0,
    };
    compiler.compile_node(reader_descriptor, node, output_offset, path)?;
    if compiler
        .ops
        .iter()
        .any(|op| matches!(op, StencilOp::RootEnum { .. }))
    {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "local enum decode helper supports only fixed payload stencils",
        });
    }
    Ok((compiler.ops, compiler.input_offset))
}

fn fixed_skip_len(
    writer_registry: &SchemaRegistry,
    writer_type: &TypeRef,
    path: &str,
) -> Result<usize, StencilError> {
    let mut compiler = StencilCompiler {
        writer_registry,
        input_offset: 0,
    };
    compiler.compile_skip_type(writer_type, path)?;
    Ok(compiler.input_offset)
}

fn checked_mul(lhs: usize, rhs: usize, path: &str) -> Result<usize, StencilError> {
    lhs.checked_mul(rhs)
        .ok_or_else(|| StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset overflow",
        })
}
