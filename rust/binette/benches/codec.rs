use binette::{
    ReaderPlan, SchemaBundle, SchemaRegistry, decode_from_slice_with_plan, encode_to_vec_with_plan,
    reader_plan_for, writer_plan_for,
};
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
use binette::{StencilDecoder, stencil_decoder_for};
use divan::{Bencher, black_box};

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

#[divan::bench]
fn encode_compact_writer_plan(bencher: Bencher) {
    let fixture = fixture();
    let sample = writer::sample();

    bencher.bench(|| {
        encode_to_vec_with_plan(black_box(&sample), black_box(&fixture.writer_plan)).unwrap()
    });
}

#[divan::bench]
fn plan_reader_field_reorder_skip(bencher: Bencher) {
    let fixture = fixture();

    bencher.bench(|| {
        reader_plan_for::<reader::Message>(
            black_box(fixture.writer_plan.root()),
            black_box(&fixture.writer_registry),
        )
        .unwrap()
    });
}

#[divan::bench]
fn plan_fixed_reader_field_reorder_skip(bencher: Bencher) {
    let fixture = fixed_fixture();

    bencher.bench(|| {
        reader_plan_for::<fixed_reader::Message>(
            black_box(fixture.writer_plan.root()),
            black_box(&fixture.writer_registry),
        )
        .unwrap()
    });
}

#[divan::bench]
fn decode_interpreted_fixed_field_reorder_skip(bencher: Bencher) {
    let fixture = fixed_fixture();

    bencher.bench(|| {
        decode_from_slice_with_plan::<fixed_reader::Message>(
            black_box(&fixture.bytes),
            black_box(&fixture.reader_plan),
            black_box(&fixture.writer_registry),
        )
        .unwrap()
    });
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[divan::bench]
fn decode_stencil_fixed_field_reorder_skip(bencher: Bencher) {
    let fixture = fixed_stencil_fixture();

    bencher.bench(|| fixture.stencil.decode(black_box(&fixture.bytes)).unwrap());
}

#[divan::bench]
fn decode_interpreted_field_reorder_skip(bencher: Bencher) {
    let fixture = fixture();

    bencher.bench(|| {
        decode_from_slice_with_plan::<reader::Message>(
            black_box(&fixture.bytes),
            black_box(&fixture.reader_plan),
            black_box(&fixture.writer_registry),
        )
        .unwrap()
    });
}
