use binette::{
    ReaderPlan, SchemaBundle, SchemaRegistry, decode_from_slice_with_plan, encode_to_vec_with_plan,
    reader_plan_for, writer_plan_for,
};
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
use binette::{
    StencilDecoder, StencilEncoder, encode_to_vec_with_stencil, hybrid_stencil_decoder_for,
    hybrid_stencil_encoder_from_plan, strict_stencil_decoder_for, strict_stencil_encoder_from_plan,
};
use divan::{Bencher, black_box};
use facet::Facet;

fn main() {
    divan::main();
}

fn registry_for(bundle: &SchemaBundle) -> SchemaRegistry {
    let mut registry = SchemaRegistry::new();
    registry.install_bundle(bundle).unwrap();
    registry
}

mod writer {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Nested {
        pub count: u32,
        pub label: String,
        pub enabled: bool,
    }

    #[derive(Facet)]
    pub struct Message {
        pub id: u64,
        pub title: String,
        pub active: bool,
        pub counts: Vec<u32>,
        pub maybe: Option<String>,
        pub nested: Nested,
        pub pair: (u16, String),
        pub writer_only: String,
    }

    pub fn sample() -> Message {
        Message {
            id: 0x0102_0304_0506_0708,
            title: "binette baseline".to_owned(),
            active: true,
            counts: vec![1, 2, 3, 5, 8, 13, 21, 34],
            maybe: Some("present".to_owned()),
            nested: Nested {
                count: 42,
                label: "nested".to_owned(),
                enabled: true,
            },
            pair: (7, "seven".to_owned()),
            writer_only: "skipped by reader".to_owned(),
        }
    }
}

mod reader {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Nested {
        pub label: String,
        pub enabled: bool,
        pub count: u32,
    }

    #[derive(Facet)]
    pub struct Message {
        pub pair: (u16, String),
        pub nested: Nested,
        pub maybe: Option<String>,
        pub counts: Vec<u32>,
        pub active: bool,
        pub title: String,
        pub id: u64,
    }
}

mod fixed_writer {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Message {
        pub id: u64,
        pub enabled: bool,
        pub code: u16,
        pub writer_only: u32,
        pub writer_only_flag: bool,
        pub seq: u8,
    }

    pub fn sample() -> Message {
        Message {
            id: 0x0102_0304_0506_0708,
            enabled: true,
            code: 0x1122,
            writer_only: 0xaabb_ccdd,
            writer_only_flag: false,
            seq: 7,
        }
    }
}

mod fixed_reader {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Message {
        pub seq: u8,
        pub enabled: bool,
        pub id: u64,
        pub code: u16,
    }
}

mod list_struct_writer {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Message {
        pub prefix: u16,
        pub counts: Vec<(u16, u32)>,
        pub tail: u32,
        pub writer_only: u16,
    }

    pub fn sample() -> Message {
        Message {
            prefix: 0x1122,
            counts: vec![(1, 10), (2, 20), (3, 30), (5, 50), (8, 80)],
            tail: 0xaabb_ccdd,
            writer_only: 0xeeff,
        }
    }
}

mod list_struct_reader {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Message {
        pub tail: u32,
        pub counts: Vec<(u16, u32)>,
        pub prefix: u16,
    }
}

mod nested_writer {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Header {
        pub trace: u64,
        pub flags: bool,
    }

    #[derive(Facet)]
    pub struct Extra {
        pub code: u16,
        pub enabled: bool,
    }

    #[derive(Facet)]
    pub struct Message {
        pub id: u32,
        pub header: Header,
        pub pair: (u16, bool),
        pub writer_only: Extra,
        pub tail: u8,
    }

    pub fn sample() -> Message {
        Message {
            id: 0x1122_3344,
            header: Header {
                trace: 0x0102_0304_0506_0708,
                flags: true,
            },
            pair: (0x5566, false),
            writer_only: Extra {
                code: 0x7788,
                enabled: true,
            },
            tail: 9,
        }
    }
}

mod nested_reader {
    use facet::Facet;

    #[derive(Facet)]
    pub struct Header {
        pub flags: bool,
        pub trace: u64,
    }

    #[derive(Facet)]
    pub struct Message {
        pub tail: u8,
        pub pair: (u16, bool),
        pub header: Header,
        pub id: u32,
    }
}

mod enum_writer {
    use facet::Facet;

    #[derive(Facet)]
    #[allow(dead_code)]
    #[repr(u8)]
    pub enum Event {
        Started,
        Moved(u32, u16),
        Failed { code: u16, flag: bool },
        WriterOnly,
    }

    pub fn sample() -> Event {
        Event::Failed {
            code: 0x1122,
            flag: true,
        }
    }
}

mod enum_reader {
    use facet::Facet;

    #[derive(Facet)]
    #[allow(dead_code)]
    #[repr(u8)]
    pub enum Event {
        Failed { flag: bool, code: u16 },
        Started,
        Moved(u32, u16),
    }
}

mod aggregate {
    use std::collections::{HashMap, HashSet};

    pub type Tuple = (u16, String, Vec<u32>, Option<bool>);
    pub type List = Vec<(u16, String)>;
    pub type FixedList = Vec<(u16, u32)>;
    pub type Set = HashSet<u16>;
    pub type Map = HashMap<u16, u8>;
    pub type OptionValue = Option<(u16, String)>;
    pub type Array = [u16; 4];
    pub type NestedList = Vec<Vec<u16>>;
    pub type Dynamic = facet_value::Value;

    pub fn tuple_sample() -> Tuple {
        (7, "seven".to_owned(), vec![1, 2, 3, 5, 8], Some(true))
    }

    pub fn list_sample() -> List {
        vec![
            (1, "one".to_owned()),
            (2, "two".to_owned()),
            (3, "three".to_owned()),
        ]
    }

    pub fn fixed_list_sample() -> FixedList {
        vec![(1, 10), (2, 20), (3, 30), (5, 50), (8, 80)]
    }

    pub fn set_sample() -> Set {
        HashSet::from([3, 1, 2, 5, 8, 13])
    }

    pub fn map_sample() -> Map {
        HashMap::from([(2, 20), (1, 10), (3, 30), (5, 50)])
    }

    pub fn option_sample() -> OptionValue {
        Some((9, "nine".to_owned()))
    }

    pub fn array_sample() -> Array {
        [5, 8, 13, 21]
    }

    pub fn nested_list_sample() -> NestedList {
        vec![vec![1, 2, 3], vec![5, 8], vec![13, 21, 34]]
    }

    pub fn dynamic_sample() -> Dynamic {
        let mut object = facet_value::VObject::new();
        object.insert("name", facet_value::Value::from("binette"));
        object.insert("count", facet_value::Value::from(3u64));
        let mut items = facet_value::VArray::new();
        items.push(facet_value::Value::from(true));
        items.push(facet_value::Value::NULL);
        object.insert("items", facet_value::Value::from(items));
        facet_value::Value::from(object)
    }
}

struct Fixture {
    writer_plan: binette::WriterPlan,
    writer_registry: SchemaRegistry,
    bytes: Vec<u8>,
    reader_plan: ReaderPlan,
}

fn fixture() -> Fixture {
    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&writer::sample(), &writer_plan).unwrap();
    let reader_plan =
        reader_plan_for::<reader::Message>(writer_plan.root(), &writer_registry).unwrap();

    Fixture {
        writer_plan,
        writer_registry,
        bytes,
        reader_plan,
    }
}

struct FixedFixture {
    writer_plan: binette::WriterPlan,
    writer_registry: SchemaRegistry,
    bytes: Vec<u8>,
    reader_plan: ReaderPlan,
}

fn fixed_fixture() -> FixedFixture {
    let writer_plan = writer_plan_for::<fixed_writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&fixed_writer::sample(), &writer_plan).unwrap();
    let reader_plan =
        reader_plan_for::<fixed_reader::Message>(writer_plan.root(), &writer_registry).unwrap();

    FixedFixture {
        writer_plan,
        writer_registry,
        bytes,
        reader_plan,
    }
}

struct ListStructFixture {
    writer_plan: binette::WriterPlan,
    writer_registry: SchemaRegistry,
    bytes: Vec<u8>,
    reader_plan: ReaderPlan,
}

fn list_struct_fixture() -> ListStructFixture {
    let writer_plan = writer_plan_for::<list_struct_writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&list_struct_writer::sample(), &writer_plan).unwrap();
    let reader_plan =
        reader_plan_for::<list_struct_reader::Message>(writer_plan.root(), &writer_registry)
            .unwrap();

    ListStructFixture {
        writer_plan,
        writer_registry,
        bytes,
        reader_plan,
    }
}

struct NestedFixture {
    writer_plan: binette::WriterPlan,
    writer_registry: SchemaRegistry,
    bytes: Vec<u8>,
    reader_plan: ReaderPlan,
}

fn nested_fixture() -> NestedFixture {
    let writer_plan = writer_plan_for::<nested_writer::Message>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&nested_writer::sample(), &writer_plan).unwrap();
    let reader_plan =
        reader_plan_for::<nested_reader::Message>(writer_plan.root(), &writer_registry).unwrap();

    NestedFixture {
        writer_plan,
        writer_registry,
        bytes,
        reader_plan,
    }
}

struct EnumFixture {
    writer_plan: binette::WriterPlan,
    writer_registry: SchemaRegistry,
    bytes: Vec<u8>,
    reader_plan: ReaderPlan,
}

fn enum_fixture() -> EnumFixture {
    let writer_plan = writer_plan_for::<enum_writer::Event>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&enum_writer::sample(), &writer_plan).unwrap();
    let reader_plan =
        reader_plan_for::<enum_reader::Event>(writer_plan.root(), &writer_registry).unwrap();

    EnumFixture {
        writer_plan,
        writer_registry,
        bytes,
        reader_plan,
    }
}

struct SameFixture<T> {
    writer_plan: binette::WriterPlan,
    writer_registry: SchemaRegistry,
    bytes: Vec<u8>,
    reader_plan: ReaderPlan,
    sample: T,
}

fn same_fixture<T: Facet<'static>>(sample: T) -> SameFixture<T> {
    let writer_plan = writer_plan_for::<T>().unwrap();
    let writer_registry = registry_for(writer_plan.schema_bundle());
    let bytes = encode_to_vec_with_plan(&sample, &writer_plan).unwrap();
    let reader_plan = reader_plan_for::<T>(writer_plan.root(), &writer_registry).unwrap();

    SameFixture {
        writer_plan,
        writer_registry,
        bytes,
        reader_plan,
        sample,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct DecodeStencilFixture<T> {
    bytes: Vec<u8>,
    stencil: StencilDecoder<T>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn fixed_hybrid_decode_fixture() -> DecodeStencilFixture<fixed_reader::Message> {
    let fixture = fixed_fixture();
    let stencil = hybrid_stencil_decoder_for::<fixed_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn fixed_jit_decode_fixture() -> DecodeStencilFixture<fixed_reader::Message> {
    let fixture = fixed_fixture();
    let stencil = strict_stencil_decoder_for::<fixed_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn list_struct_hybrid_decode_fixture() -> DecodeStencilFixture<list_struct_reader::Message> {
    let fixture = list_struct_fixture();
    let stencil = hybrid_stencil_decoder_for::<list_struct_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn list_struct_jit_decode_fixture() -> DecodeStencilFixture<list_struct_reader::Message> {
    let fixture = list_struct_fixture();
    let stencil = strict_stencil_decoder_for::<list_struct_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn nested_hybrid_decode_fixture() -> DecodeStencilFixture<nested_reader::Message> {
    let fixture = nested_fixture();
    let stencil = hybrid_stencil_decoder_for::<nested_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn nested_jit_decode_fixture() -> DecodeStencilFixture<nested_reader::Message> {
    let fixture = nested_fixture();
    let stencil = strict_stencil_decoder_for::<nested_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn enum_hybrid_decode_fixture() -> DecodeStencilFixture<enum_reader::Event> {
    let fixture = enum_fixture();
    let stencil = hybrid_stencil_decoder_for::<enum_reader::Event>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn enum_jit_decode_fixture() -> DecodeStencilFixture<enum_reader::Event> {
    let fixture = enum_fixture();
    let stencil = strict_stencil_decoder_for::<enum_reader::Event>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn array_jit_decode_fixture() -> DecodeStencilFixture<aggregate::Array> {
    let fixture = same_fixture::<aggregate::Array>(aggregate::array_sample());
    let stencil = strict_stencil_decoder_for::<aggregate::Array>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn fixed_list_jit_decode_fixture() -> DecodeStencilFixture<aggregate::FixedList> {
    let fixture = same_fixture::<aggregate::FixedList>(aggregate::fixed_list_sample());
    let stencil = strict_stencil_decoder_for::<aggregate::FixedList>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mixed_hybrid_decode_fixture() -> DecodeStencilFixture<reader::Message> {
    let fixture = fixture();
    let stencil = hybrid_stencil_decoder_for::<reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    DecodeStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct SameHybridFixture<T> {
    fixture: SameFixture<T>,
    decoder: StencilDecoder<T>,
    encoder: StencilEncoder<T>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn same_hybrid_fixture<T: Facet<'static>>(sample: T) -> SameHybridFixture<T> {
    let fixture = same_fixture(sample);
    let decoder =
        hybrid_stencil_decoder_for::<T>(fixture.writer_plan.root(), &fixture.writer_registry)
            .unwrap();
    let encoder = hybrid_stencil_encoder_from_plan::<T>(&fixture.writer_plan).unwrap();

    SameHybridFixture {
        fixture,
        decoder,
        encoder,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct EncodeStencilFixture<T> {
    sample: T,
    stencil: StencilEncoder<T>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn fixed_encode_hybrid_fixture() -> EncodeStencilFixture<fixed_writer::Message> {
    let writer_plan = writer_plan_for::<fixed_writer::Message>().unwrap();
    let stencil = hybrid_stencil_encoder_from_plan::<fixed_writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: fixed_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn fixed_encode_jit_fixture() -> EncodeStencilFixture<fixed_writer::Message> {
    let writer_plan = writer_plan_for::<fixed_writer::Message>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<fixed_writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: fixed_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn list_struct_encode_hybrid_fixture() -> EncodeStencilFixture<list_struct_writer::Message> {
    let writer_plan = writer_plan_for::<list_struct_writer::Message>().unwrap();
    let stencil =
        hybrid_stencil_encoder_from_plan::<list_struct_writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: list_struct_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn list_struct_encode_jit_fixture() -> EncodeStencilFixture<list_struct_writer::Message> {
    let writer_plan = writer_plan_for::<list_struct_writer::Message>().unwrap();
    let stencil =
        strict_stencil_encoder_from_plan::<list_struct_writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: list_struct_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn nested_encode_hybrid_fixture() -> EncodeStencilFixture<nested_writer::Message> {
    let writer_plan = writer_plan_for::<nested_writer::Message>().unwrap();
    let stencil = hybrid_stencil_encoder_from_plan::<nested_writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: nested_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn nested_encode_jit_fixture() -> EncodeStencilFixture<nested_writer::Message> {
    let writer_plan = writer_plan_for::<nested_writer::Message>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<nested_writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: nested_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mixed_encode_hybrid_fixture() -> EncodeStencilFixture<writer::Message> {
    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let stencil = hybrid_stencil_encoder_from_plan::<writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mixed_encode_jit_fixture() -> EncodeStencilFixture<writer::Message> {
    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn enum_encode_hybrid_fixture() -> EncodeStencilFixture<enum_writer::Event> {
    let writer_plan = writer_plan_for::<enum_writer::Event>().unwrap();
    let stencil = hybrid_stencil_encoder_from_plan::<enum_writer::Event>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: enum_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn enum_encode_jit_fixture() -> EncodeStencilFixture<enum_writer::Event> {
    let writer_plan = writer_plan_for::<enum_writer::Event>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<enum_writer::Event>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: enum_writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn array_encode_jit_fixture() -> EncodeStencilFixture<aggregate::Array> {
    let writer_plan = writer_plan_for::<aggregate::Array>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<aggregate::Array>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: aggregate::array_sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn option_encode_jit_fixture() -> EncodeStencilFixture<aggregate::OptionValue> {
    let writer_plan = writer_plan_for::<aggregate::OptionValue>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<aggregate::OptionValue>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: aggregate::option_sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn list_encode_jit_fixture() -> EncodeStencilFixture<aggregate::List> {
    let writer_plan = writer_plan_for::<aggregate::List>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<aggregate::List>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: aggregate::list_sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn fixed_list_encode_jit_fixture() -> EncodeStencilFixture<aggregate::FixedList> {
    let writer_plan = writer_plan_for::<aggregate::FixedList>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<aggregate::FixedList>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: aggregate::fixed_list_sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn tuple_encode_jit_fixture() -> EncodeStencilFixture<aggregate::Tuple> {
    let writer_plan = writer_plan_for::<aggregate::Tuple>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<aggregate::Tuple>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: aggregate::tuple_sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn nested_list_encode_jit_fixture() -> EncodeStencilFixture<aggregate::NestedList> {
    let writer_plan = writer_plan_for::<aggregate::NestedList>().unwrap();
    let stencil = strict_stencil_encoder_from_plan::<aggregate::NestedList>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: aggregate::nested_list_sample(),
        stencil,
    }
}

macro_rules! same_schema_encode_benches {
    ($module:ident, $ty:ty, $sample:expr) => {
        mod $module {
            use super::*;

            #[divan::bench]
            pub fn interp(bencher: Bencher) {
                let fixture = same_fixture::<$ty>($sample);

                bencher.bench(|| {
                    black_box(
                        encode_to_vec_with_plan(
                            black_box(&fixture.sample),
                            black_box(&fixture.writer_plan),
                        )
                        .unwrap(),
                    )
                });
            }

            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            #[divan::bench]
            pub fn hybrid(bencher: Bencher) {
                let fixture = same_hybrid_fixture::<$ty>($sample);

                bencher.bench(|| {
                    black_box(
                        encode_to_vec_with_stencil(
                            black_box(&fixture.fixture.sample),
                            &fixture.encoder,
                        )
                        .unwrap(),
                    )
                });
            }
        }
    };
}

macro_rules! same_schema_decode_benches {
    ($module:ident, $ty:ty, $sample:expr) => {
        mod $module {
            use super::*;

            #[divan::bench]
            pub fn interp(bencher: Bencher) {
                let fixture = same_fixture::<$ty>($sample);

                bencher.bench(|| {
                    black_box(
                        decode_from_slice_with_plan::<$ty>(
                            black_box(&fixture.bytes),
                            black_box(&fixture.reader_plan),
                            black_box(&fixture.writer_registry),
                        )
                        .unwrap(),
                    )
                });
            }

            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            #[divan::bench]
            pub fn hybrid(bencher: Bencher) {
                let fixture = same_hybrid_fixture::<$ty>($sample);

                bencher.bench(|| {
                    black_box(
                        fixture
                            .decoder
                            .decode(black_box(&fixture.fixture.bytes))
                            .unwrap(),
                    )
                });
            }
        }
    };
}

macro_rules! same_schema_plan_bench {
    ($name:ident, $ty:ty, $sample:expr) => {
        #[divan::bench]
        pub fn $name(bencher: Bencher) {
            let fixture = same_fixture::<$ty>($sample);

            bencher.bench(|| {
                black_box(
                    reader_plan_for::<$ty>(
                        black_box(fixture.writer_plan.root()),
                        black_box(&fixture.writer_registry),
                    )
                    .unwrap(),
                )
            });
        }
    };
}

mod encode {
    use super::*;

    mod fixed_struct {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let writer_plan = writer_plan_for::<fixed_writer::Message>().unwrap();
            let sample = fixed_writer::sample();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(black_box(&sample), black_box(&writer_plan)).unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = fixed_encode_hybrid_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = fixed_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }

    mod list_struct {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let writer_plan = writer_plan_for::<list_struct_writer::Message>().unwrap();
            let sample = list_struct_writer::sample();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(black_box(&sample), black_box(&writer_plan)).unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = list_struct_encode_hybrid_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = list_struct_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }

    mod nested_struct {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let writer_plan = writer_plan_for::<nested_writer::Message>().unwrap();
            let sample = nested_writer::sample();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(black_box(&sample), black_box(&writer_plan)).unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = nested_encode_hybrid_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = nested_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }

    mod r#enum {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = enum_fixture();
            let sample = enum_writer::sample();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(black_box(&sample), black_box(&fixture.writer_plan))
                        .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = enum_encode_hybrid_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = enum_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }

    mod mixed_struct {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = fixture();
            let sample = writer::sample();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(black_box(&sample), black_box(&fixture.writer_plan))
                        .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = mixed_encode_hybrid_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = mixed_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }

    mod tuple {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = same_fixture::<aggregate::Tuple>(aggregate::tuple_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(
                        black_box(&fixture.sample),
                        black_box(&fixture.writer_plan),
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = same_hybrid_fixture::<aggregate::Tuple>(aggregate::tuple_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(
                        black_box(&fixture.fixture.sample),
                        &fixture.encoder,
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = tuple_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }
    mod list {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = same_fixture::<aggregate::List>(aggregate::list_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(
                        black_box(&fixture.sample),
                        black_box(&fixture.writer_plan),
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = same_hybrid_fixture::<aggregate::List>(aggregate::list_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(
                        black_box(&fixture.fixture.sample),
                        &fixture.encoder,
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = list_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }
    mod fixed_list {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = same_fixture::<aggregate::FixedList>(aggregate::fixed_list_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(
                        black_box(&fixture.sample),
                        black_box(&fixture.writer_plan),
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture =
                same_hybrid_fixture::<aggregate::FixedList>(aggregate::fixed_list_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(
                        black_box(&fixture.fixture.sample),
                        &fixture.encoder,
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = fixed_list_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }
    same_schema_encode_benches!(set, aggregate::Set, aggregate::set_sample());
    same_schema_encode_benches!(map, aggregate::Map, aggregate::map_sample());
    mod nested_list {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = same_fixture::<aggregate::NestedList>(aggregate::nested_list_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(
                        black_box(&fixture.sample),
                        black_box(&fixture.writer_plan),
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture =
                same_hybrid_fixture::<aggregate::NestedList>(aggregate::nested_list_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(
                        black_box(&fixture.fixture.sample),
                        &fixture.encoder,
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = nested_list_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }
    mod option {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = same_fixture::<aggregate::OptionValue>(aggregate::option_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(
                        black_box(&fixture.sample),
                        black_box(&fixture.writer_plan),
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = same_hybrid_fixture::<aggregate::OptionValue>(aggregate::option_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(
                        black_box(&fixture.fixture.sample),
                        &fixture.encoder,
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = option_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }
    mod array {
        use super::*;

        #[divan::bench]
        pub fn interp(bencher: Bencher) {
            let fixture = same_fixture::<aggregate::Array>(aggregate::array_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_plan(
                        black_box(&fixture.sample),
                        black_box(&fixture.writer_plan),
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn hybrid(bencher: Bencher) {
            let fixture = same_hybrid_fixture::<aggregate::Array>(aggregate::array_sample());

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(
                        black_box(&fixture.fixture.sample),
                        &fixture.encoder,
                    )
                    .unwrap(),
                )
            });
        }

        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        #[divan::bench]
        pub fn jit(bencher: Bencher) {
            let fixture = array_encode_jit_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }
    same_schema_encode_benches!(dynamic, aggregate::Dynamic, aggregate::dynamic_sample());
}

mod plan {
    use super::*;

    #[divan::bench]
    pub fn mixed_struct(bencher: Bencher) {
        let fixture = fixture();

        bencher.bench(|| {
            black_box(
                reader_plan_for::<reader::Message>(
                    black_box(fixture.writer_plan.root()),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[divan::bench]
    pub fn fixed_struct(bencher: Bencher) {
        let fixture = fixed_fixture();

        bencher.bench(|| {
            black_box(
                reader_plan_for::<fixed_reader::Message>(
                    black_box(fixture.writer_plan.root()),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[divan::bench]
    pub fn list_struct(bencher: Bencher) {
        let fixture = list_struct_fixture();

        bencher.bench(|| {
            black_box(
                reader_plan_for::<list_struct_reader::Message>(
                    black_box(fixture.writer_plan.root()),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    same_schema_plan_bench!(tuple, aggregate::Tuple, aggregate::tuple_sample());
    same_schema_plan_bench!(list, aggregate::List, aggregate::list_sample());
    same_schema_plan_bench!(
        fixed_list,
        aggregate::FixedList,
        aggregate::fixed_list_sample()
    );
    same_schema_plan_bench!(set, aggregate::Set, aggregate::set_sample());
    same_schema_plan_bench!(map, aggregate::Map, aggregate::map_sample());
    same_schema_plan_bench!(option, aggregate::OptionValue, aggregate::option_sample());
    same_schema_plan_bench!(array, aggregate::Array, aggregate::array_sample());
    same_schema_plan_bench!(
        nested_list,
        aggregate::NestedList,
        aggregate::nested_list_sample()
    );
    same_schema_plan_bench!(dynamic, aggregate::Dynamic, aggregate::dynamic_sample());
}

mod fixed_struct {
    use super::*;

    #[divan::bench]
    pub fn interp(bencher: Bencher) {
        let fixture = fixed_fixture();

        bencher.bench(|| {
            black_box(
                decode_from_slice_with_plan::<fixed_reader::Message>(
                    black_box(&fixture.bytes),
                    black_box(&fixture.reader_plan),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn hybrid(bencher: Bencher) {
        let fixture = fixed_hybrid_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn jit(bencher: Bencher) {
        let fixture = fixed_jit_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}

mod list_struct {
    use super::*;

    #[divan::bench]
    pub fn interp(bencher: Bencher) {
        let fixture = list_struct_fixture();

        bencher.bench(|| {
            black_box(
                decode_from_slice_with_plan::<list_struct_reader::Message>(
                    black_box(&fixture.bytes),
                    black_box(&fixture.reader_plan),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn hybrid(bencher: Bencher) {
        let fixture = list_struct_hybrid_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn jit(bencher: Bencher) {
        let fixture = list_struct_jit_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}

mod nested_struct {
    use super::*;

    #[divan::bench]
    pub fn interp(bencher: Bencher) {
        let fixture = nested_fixture();

        bencher.bench(|| {
            black_box(
                decode_from_slice_with_plan::<nested_reader::Message>(
                    black_box(&fixture.bytes),
                    black_box(&fixture.reader_plan),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn hybrid(bencher: Bencher) {
        let fixture = nested_hybrid_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn jit(bencher: Bencher) {
        let fixture = nested_jit_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}

mod r#enum {
    use super::*;

    #[divan::bench]
    pub fn interp(bencher: Bencher) {
        let fixture = enum_fixture();

        bencher.bench(|| {
            black_box(
                decode_from_slice_with_plan::<enum_reader::Event>(
                    black_box(&fixture.bytes),
                    black_box(&fixture.reader_plan),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn hybrid(bencher: Bencher) {
        let fixture = enum_hybrid_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn jit(bencher: Bencher) {
        let fixture = enum_jit_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}

mod mixed_struct {
    use super::*;

    #[divan::bench]
    pub fn interp(bencher: Bencher) {
        let fixture = fixture();

        bencher.bench(|| {
            black_box(
                decode_from_slice_with_plan::<reader::Message>(
                    black_box(&fixture.bytes),
                    black_box(&fixture.reader_plan),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn hybrid(bencher: Bencher) {
        let fixture = mixed_hybrid_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}

same_schema_decode_benches!(tuple, aggregate::Tuple, aggregate::tuple_sample());
same_schema_decode_benches!(list, aggregate::List, aggregate::list_sample());
mod fixed_list {
    use super::*;

    #[divan::bench]
    pub fn interp(bencher: Bencher) {
        let fixture = same_fixture::<aggregate::FixedList>(aggregate::fixed_list_sample());

        bencher.bench(|| {
            black_box(
                decode_from_slice_with_plan::<aggregate::FixedList>(
                    black_box(&fixture.bytes),
                    black_box(&fixture.reader_plan),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn hybrid(bencher: Bencher) {
        let fixture = same_hybrid_fixture::<aggregate::FixedList>(aggregate::fixed_list_sample());

        bencher.bench(|| {
            black_box(
                fixture
                    .decoder
                    .decode(black_box(&fixture.fixture.bytes))
                    .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn jit(bencher: Bencher) {
        let fixture = fixed_list_jit_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}
same_schema_decode_benches!(set, aggregate::Set, aggregate::set_sample());
same_schema_decode_benches!(map, aggregate::Map, aggregate::map_sample());
same_schema_decode_benches!(option, aggregate::OptionValue, aggregate::option_sample());
mod array {
    use super::*;

    #[divan::bench]
    pub fn interp(bencher: Bencher) {
        let fixture = same_fixture::<aggregate::Array>(aggregate::array_sample());

        bencher.bench(|| {
            black_box(
                decode_from_slice_with_plan::<aggregate::Array>(
                    black_box(&fixture.bytes),
                    black_box(&fixture.reader_plan),
                    black_box(&fixture.writer_registry),
                )
                .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn hybrid(bencher: Bencher) {
        let fixture = same_hybrid_fixture::<aggregate::Array>(aggregate::array_sample());

        bencher.bench(|| {
            black_box(
                fixture
                    .decoder
                    .decode(black_box(&fixture.fixture.bytes))
                    .unwrap(),
            )
        });
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[divan::bench]
    pub fn jit(bencher: Bencher) {
        let fixture = array_jit_decode_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}
same_schema_decode_benches!(dynamic, aggregate::Dynamic, aggregate::dynamic_sample());
