use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TypeId(pub u64);

// r[impl binette.schema.type-ref]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeRef {
    Concrete { type_id: TypeId, args: Vec<TypeRef> },
    Var { name: String },
}

impl TypeRef {
    pub fn concrete(type_id: TypeId) -> Self {
        Self::Concrete {
            type_id,
            args: Vec::new(),
        }
    }

    pub fn generic(type_id: TypeId, args: Vec<TypeRef>) -> Self {
        Self::Concrete { type_id, args }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaBundle {
    pub schemas: Vec<Schema>,
    pub root: TypeRef,
    pub attachments: Vec<AttachmentDeclaration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentDeclaration {
    pub kind: String,
    pub metadata_schema: Option<TypeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub id: TypeId,
    pub type_params: Vec<String>,
    pub kind: SchemaKind,
}

// r[impl binette.schema.kinds]
// r[impl binette.schema.array]
// r[impl binette.schema.dynamic]
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaKind {
    Primitive(Primitive),
    Struct {
        name: String,
        fields: Vec<Field>,
    },
    Enum {
        name: String,
        variants: Vec<Variant>,
    },
    Tuple {
        elements: Vec<TypeRef>,
    },
    List {
        element: TypeRef,
    },
    Set {
        element: TypeRef,
    },
    Map {
        key: TypeRef,
        value: TypeRef,
    },
    Array {
        element: TypeRef,
        dimensions: Vec<u64>,
    },
    Option {
        element: TypeRef,
    },
    Dynamic,
    External {
        kind: String,
        metadata: Value,
    },
}

// r[impl binette.schema.fields]
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub type_ref: TypeRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: String,
    pub index: u32,
    pub payload: VariantPayload,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantPayload {
    Unit,
    Newtype { type_ref: TypeRef },
    Tuple { elements: Vec<TypeRef> },
    Struct { fields: Vec<Field> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    Char,
    String,
    Unit,
    Never,
    Bytes,
    Payload,
}
