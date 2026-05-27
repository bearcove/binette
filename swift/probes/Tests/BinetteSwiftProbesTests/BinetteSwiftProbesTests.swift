import BinetteSwiftProbes
import Foundation
import XCTest

final class BinetteSwiftProbesTests: XCTestCase {
    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.swift-probes+2]
    func testRepresentativeDescriptorsAreProduced() {
        let descriptors = makeProbeDescriptors()

        XCTAssertTrue(validateProbeDescriptors(descriptors))
        XCTAssertTrue(descriptors.allSatisfy { $0.backend == .swiftProbe })
    }

    // r[verify binette.local-access.runtime-facts]
    // r[verify binette.local-access.swift-probes+2]
    func testProbeDescriptorsValidateAgainstLiveRuntimeValues() {
        XCTAssertTrue(validateProbeRuntimeFacts())
    }

    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.runtime-facts]
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

    // r[verify binette.local-access.backends]
    // r[verify binette.local-access.descriptor+2]
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

    // r[verify binette.local-access.descriptor+2]
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

    // r[verify binette.local-access.descriptor+2]
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

    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.runtime-facts]
    // r[verify binette.local-access.swift-probes+2]
    func testSwiftOptionalUInt16DescriptorExportsProbedDirectTagLayout() throws {
        let descriptor = try XCTUnwrap(
            makeProbeDescriptors().first { $0.schemaName == "option<u16>" }
        )

        guard case let .optional(some, storage) = descriptor.kind else {
            return XCTFail("expected optional descriptor")
        }
        XCTAssertEqual(some.schemaName, "u16")
        guard case let .directTag(offset, width, noneValue, someValue, someOffset) = storage else {
            return XCTFail("expected direct optional descriptor")
        }

        let none: UInt16? = nil
        let someOptional: UInt16? = 0xCAFE
        XCTAssertEqual(width, MemoryLayout<UInt8>.size)
        XCTAssertNotEqual(noneValue, someValue)
        XCTAssertEqual(loadByte(from: none, offset: offset), UInt8(noneValue))
        XCTAssertEqual(loadByte(from: someOptional, offset: offset), UInt8(someValue))
        XCTAssertEqual(loadUInt16(from: someOptional, offset: someOffset), 0xCAFE)

        XCTAssertEqual(someOffset, 0)
    }

    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.runtime-facts]
    // r[verify binette.local-access.swift-probes+2]
    func testSwiftOptionalBoolDescriptorExportsProbedNicheLayout() throws {
        let descriptor = try XCTUnwrap(
            makeProbeDescriptors().first { $0.schemaName == "option<bool>" }
        )

        guard case let .optional(some, storage) = descriptor.kind else {
            return XCTFail("expected optional descriptor")
        }
        XCTAssertEqual(some.schemaName, "bool")
        guard case let .nicheTag(offset, width, noneValue, someOffset) = storage else {
            return XCTFail("expected niche optional descriptor")
        }

        let none: Bool? = nil
        let someFalse: Bool? = false
        let someTrue: Bool? = true
        XCTAssertEqual(width, MemoryLayout<UInt8>.size)
        XCTAssertEqual(loadByte(from: none, offset: offset), UInt8(noneValue))
        XCTAssertNotEqual(loadByte(from: someFalse, offset: offset), UInt8(noneValue))
        XCTAssertNotEqual(loadByte(from: someTrue, offset: offset), UInt8(noneValue))
        XCTAssertFalse(loadBool(from: someFalse, offset: someOffset))
        XCTAssertTrue(loadBool(from: someTrue, offset: someOffset))

        XCTAssertEqual(noneValue, 2)
    }
}

private func loadByte<T>(from value: T, offset: Int) -> UInt8 {
    var value = value
    return withUnsafeBytes(of: &value) { bytes in
        bytes.baseAddress!.advanced(by: offset).load(as: UInt8.self)
    }
}

private func loadUInt16<T>(from value: T, offset: Int) -> UInt16 {
    var value = value
    return withUnsafeBytes(of: &value) { bytes in
        bytes.baseAddress!.advanced(by: offset).load(as: UInt16.self)
    }
}

private func loadBool<T>(from value: T, offset: Int) -> Bool {
    var value = value
    return withUnsafeBytes(of: &value) { bytes in
        bytes.baseAddress!.advanced(by: offset).load(as: Bool.self)
    }
}
