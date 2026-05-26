use binette::{
    ReaderPlan, SchemaBundle, SchemaRegistry, decode_from_slice_with_plan, encode_to_vec_with_plan,
    reader_plan_for, writer_plan_for,
};
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
use binette::{
    StencilDecoder, StencilEncoder, encode_to_vec_with_stencil, stencil_decoder_for,
    stencil_encoder_from_plan,
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
    pub type Set = HashSet<u16>;
    pub type Map = HashMap<u16, u8>;
    pub type OptionValue = Option<(u16, String)>;
    pub type Array = [u16; 4];

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
struct FixedStencilFixture {
    bytes: Vec<u8>,
    stencil: StencilDecoder<fixed_reader::Message>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn fixed_stencil_fixture() -> FixedStencilFixture {
    let fixture = fixed_fixture();
    let stencil = stencil_decoder_for::<fixed_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    FixedStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct NestedStencilFixture {
    bytes: Vec<u8>,
    stencil: StencilDecoder<nested_reader::Message>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn nested_stencil_fixture() -> NestedStencilFixture {
    let fixture = nested_fixture();
    let stencil = stencil_decoder_for::<nested_reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    NestedStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct EnumStencilFixture {
    bytes: Vec<u8>,
    stencil: StencilDecoder<enum_reader::Event>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn enum_stencil_fixture() -> EnumStencilFixture {
    let fixture = enum_fixture();
    let stencil = stencil_decoder_for::<enum_reader::Event>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    EnumStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct MixedStencilFixture {
    bytes: Vec<u8>,
    stencil: StencilDecoder<reader::Message>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mixed_stencil_fixture() -> MixedStencilFixture {
    let fixture = fixture();
    let stencil = stencil_decoder_for::<reader::Message>(
        fixture.writer_plan.root(),
        &fixture.writer_registry,
    )
    .unwrap();

    MixedStencilFixture {
        bytes: fixture.bytes,
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct SameStencilFixture<T> {
    fixture: SameFixture<T>,
    decoder: StencilDecoder<T>,
    encoder: StencilEncoder<T>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn same_stencil_fixture<T: Facet<'static>>(sample: T) -> SameStencilFixture<T> {
    let fixture = same_fixture(sample);
    let decoder =
        stencil_decoder_for::<T>(fixture.writer_plan.root(), &fixture.writer_registry).unwrap();
    let encoder = stencil_encoder_from_plan::<T>(&fixture.writer_plan).unwrap();

    SameStencilFixture {
        fixture,
        decoder,
        encoder,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct EncodeStencilFixture {
    sample: writer::Message,
    stencil: StencilEncoder<writer::Message>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn encode_stencil_fixture() -> EncodeStencilFixture {
    let writer_plan = writer_plan_for::<writer::Message>().unwrap();
    let stencil = stencil_encoder_from_plan::<writer::Message>(&writer_plan).unwrap();

    EncodeStencilFixture {
        sample: writer::sample(),
        stencil,
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct EncodeEnumStencilFixture {
    sample: enum_writer::Event,
    stencil: StencilEncoder<enum_writer::Event>,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn encode_enum_stencil_fixture() -> EncodeEnumStencilFixture {
    let writer_plan = writer_plan_for::<enum_writer::Event>().unwrap();
    let stencil = stencil_encoder_from_plan::<enum_writer::Event>(&writer_plan).unwrap();

    EncodeEnumStencilFixture {
        sample: enum_writer::sample(),
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
            pub fn stencil(bencher: Bencher) {
                let fixture = same_stencil_fixture::<$ty>($sample);

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
            pub fn stencil(bencher: Bencher) {
                let fixture = same_stencil_fixture::<$ty>($sample);

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
        pub fn stencil(bencher: Bencher) {
            let fixture = encode_enum_stencil_fixture();

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
        pub fn stencil(bencher: Bencher) {
            let fixture = encode_stencil_fixture();

            bencher.bench(|| {
                black_box(
                    encode_to_vec_with_stencil(black_box(&fixture.sample), &fixture.stencil)
                        .unwrap(),
                )
            });
        }
    }

    same_schema_encode_benches!(tuple, aggregate::Tuple, aggregate::tuple_sample());
    same_schema_encode_benches!(list, aggregate::List, aggregate::list_sample());
    same_schema_encode_benches!(set, aggregate::Set, aggregate::set_sample());
    same_schema_encode_benches!(map, aggregate::Map, aggregate::map_sample());
    same_schema_encode_benches!(option, aggregate::OptionValue, aggregate::option_sample());
    same_schema_encode_benches!(array, aggregate::Array, aggregate::array_sample());
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

    same_schema_plan_bench!(tuple, aggregate::Tuple, aggregate::tuple_sample());
    same_schema_plan_bench!(list, aggregate::List, aggregate::list_sample());
    same_schema_plan_bench!(set, aggregate::Set, aggregate::set_sample());
    same_schema_plan_bench!(map, aggregate::Map, aggregate::map_sample());
    same_schema_plan_bench!(option, aggregate::OptionValue, aggregate::option_sample());
    same_schema_plan_bench!(array, aggregate::Array, aggregate::array_sample());
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
    pub fn stencil(bencher: Bencher) {
        let fixture = fixed_stencil_fixture();

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
    pub fn stencil(bencher: Bencher) {
        let fixture = nested_stencil_fixture();

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
    pub fn stencil(bencher: Bencher) {
        let fixture = enum_stencil_fixture();

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
    pub fn stencil(bencher: Bencher) {
        let fixture = mixed_stencil_fixture();

        bencher.bench(|| black_box(fixture.stencil.decode(black_box(&fixture.bytes)).unwrap()));
    }
}

same_schema_decode_benches!(tuple, aggregate::Tuple, aggregate::tuple_sample());
same_schema_decode_benches!(list, aggregate::List, aggregate::list_sample());
same_schema_decode_benches!(set, aggregate::Set, aggregate::set_sample());
same_schema_decode_benches!(map, aggregate::Map, aggregate::map_sample());
same_schema_decode_benches!(option, aggregate::OptionValue, aggregate::option_sample());
same_schema_decode_benches!(array, aggregate::Array, aggregate::array_sample());
