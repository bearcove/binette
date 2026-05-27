import BinetteSwiftProbes
import Foundation
import XCTest

final class BinetteSwiftProbesTests: XCTestCase {
    func testRepresentativeDescriptorsAreProduced() {
        let descriptors = makeProbeDescriptors()

        XCTAssertTrue(validateProbeDescriptors(descriptors))
        XCTAssertTrue(descriptors.allSatisfy { $0.backend == .swiftProbe })
    }

    func testProbeDescriptorsExportAsCodableHandoff() throws {
        let exports = exportProbeDescriptors()
        let data = try JSONEncoder().encode(exports)
        let decoded = try JSONDecoder().decode([BinetteDescriptorExport].self, from: data)

        XCTAssertEqual(decoded, exports)
        XCTAssertTrue(exports.allSatisfy { $0.backend == "swift-probe" })

        let nested = try XCTUnwrap(exports.first { $0.schemaName == "ProbeNested" })
        XCTAssertEqual(nested.kind.tag, "struct")
        XCTAssertEqual(nested.kind.fields?.map(\.access.tag), ["direct", "direct", "direct"])

        let enumDescriptor = try XCTUnwrap(exports.first { $0.schemaName == "ProbeEnum" })
        XCTAssertEqual(enumDescriptor.kind.tag, "enum")
        XCTAssertEqual(enumDescriptor.kind.variants?.map(\.name), ["empty", "titled", "nested"])
        XCTAssertEqual(enumDescriptor.kind.variants?[1].payload?.schemaName, "string")

        let string = try XCTUnwrap(exports.first { $0.schemaName == "string" })
        XCTAssertEqual(string.kind.tag, "string")
        XCTAssertEqual(string.kind.storage?.tag, "thunk")
        XCTAssertEqual(string.kind.storage?.count, "Swift.String.utf8.count")
        XCTAssertEqual(string.kind.storage?.write, "Swift.String.init.utf8")
    }

    func testStoredStructFieldsUseDirectOffsets() throws {
        let descriptor = try XCTUnwrap(
            makeProbeDescriptors().first { $0.schemaName == "ProbeNested" }
        )

        guard case let .storedStruct(fields) = descriptor.kind else {
            return XCTFail("expected stored struct descriptor")
        }

        XCTAssertEqual(fields.map(\.name), ["title", "leaf", "values"])
        XCTAssertEqual(fields.map(\.access), [
            .direct(offset: MemoryLayout<ProbeNested>.offset(of: \ProbeNested.title)!),
            .direct(offset: MemoryLayout<ProbeNested>.offset(of: \ProbeNested.leaf)!),
            .direct(offset: MemoryLayout<ProbeNested>.offset(of: \ProbeNested.values)!),
        ])
    }

    func testSwiftStandardLibraryShapesStartAsExplicitThunkFallbacks() throws {
        let descriptors = makeProbeDescriptors()
        let string = try XCTUnwrap(descriptors.first { $0.schemaName == "string" })
        let array = try XCTUnwrap(descriptors.first { $0.schemaName == "array<i64>" })
        let optional = try XCTUnwrap(descriptors.first { $0.schemaName == "option<string>" })
        let enumDescriptor = try XCTUnwrap(descriptors.first { $0.schemaName == "ProbeEnum" })

        guard case let .scalar(.string(stringStorage)) = string.kind else {
            return XCTFail("expected string scalar descriptor")
        }
        guard case let .sequence(_, arrayStorage) = array.kind else {
            return XCTFail("expected array sequence descriptor")
        }
        guard case let .optional(_, optionalStorage) = optional.kind else {
            return XCTFail("expected optional descriptor")
        }

        XCTAssertEqual(
            stringStorage,
            .thunk(
                count: "Swift.String.utf8.count",
                element: "Swift.String.utf8.element",
                write: "Swift.String.init.utf8"
            )
        )
        XCTAssertEqual(
            arrayStorage,
            .thunk(
                count: "Swift.Array.count",
                element: "Swift.Array.element",
                write: "Swift.Array.init.elements"
            )
        )
        XCTAssertEqual(
            optionalStorage,
            .thunk(
                isSome: "Swift.Optional.isSome",
                some: "Swift.Optional.some",
                writeNone: "Swift.Optional.init.none",
                writeSomeBytes: "Swift.Optional<String>.init.some.utf8"
            )
        )
        guard case let .enumPayloads(tag, variants) = enumDescriptor.kind else {
            return XCTFail("expected enum descriptor")
        }
        XCTAssertEqual(tag, .thunk("ProbeEnum.discriminant"))
        XCTAssertEqual(variants.map(\.name), ["empty", "titled", "nested"])
        XCTAssertEqual(variants.map(\.index), [0, 1, 2])
        XCTAssertEqual(
            variants.map(\.construct),
            ["ProbeEnum.init.empty", "ProbeEnum.init.titled.utf8", "ProbeEnum.init.nested"]
        )
        XCTAssertNil(variants[0].payload)
        XCTAssertEqual(variants[1].payload?.schemaName, "string")
        XCTAssertEqual(variants[2].payload?.schemaName, "ProbeLeaf")
    }

    func testStringThunkNamesCoverEncodeProjectionAndDecodeConstruction() throws {
        let descriptor = try XCTUnwrap(makeProbeDescriptors().first { $0.schemaName == "string" })

        guard case let .scalar(.string(storage)) = descriptor.kind else {
            return XCTFail("expected string scalar descriptor")
        }
        XCTAssertEqual(
            storage,
            .thunk(
                count: "Swift.String.utf8.count",
                element: "Swift.String.utf8.element",
                write: "Swift.String.init.utf8"
            )
        )
    }

    func testOptionalThunkNamesCoverEncodeProjectionAndDecodeConstruction() throws {
        let descriptor = try XCTUnwrap(
            makeProbeDescriptors().first { $0.schemaName == "option<string>" }
        )

        guard case let .optional(some, storage) = descriptor.kind else {
            return XCTFail("expected optional descriptor")
        }
        XCTAssertEqual(some.schemaName, "string")
        XCTAssertEqual(
            storage,
            .thunk(
                isSome: "Swift.Optional.isSome",
                some: "Swift.Optional.some",
                writeNone: "Swift.Optional.init.none",
                writeSomeBytes: "Swift.Optional<String>.init.some.utf8"
            )
        )
    }
}
