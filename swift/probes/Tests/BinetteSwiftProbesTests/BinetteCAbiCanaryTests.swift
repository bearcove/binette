import CBinette
import BinetteSwiftProbes
import Foundation
import XCTest

private enum Message: Equatable {
    case Hi(String)
    case Bye(UInt32)
}

private struct VoxLikeChannel: Equatable {}

private struct VoxLikeRequest: Equatable {
    var title: String
    var note: String?
    var payload: [UInt8]
    var retry: UInt16?
    var stream: VoxLikeChannel
}

private struct VoxLikeArgs: Equatable {
    var title: String
    var count: UInt32
}

private typealias CVariantProject = @convention(c) (UnsafePointer<UInt8>?, UnsafeMutableRawPointer?) -> UnsafePointer<UInt8>?

final class BinetteCAbiCanaryTests: XCTestCase {
    // r[verify binette.local-access.boundary]
    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.swift-probes+2]
    func testSwiftMessageCrossesRustCAbiThroughLocalDescriptor() throws {
        let handle = try importMessageDescriptor()
        defer { binette_local_descriptor_free(handle) }
        let schemaBundle = try canaryMessageSchemaBundle()
        defer { binette_byte_buffer_free(schemaBundle) }

        for value in [Message.Hi("hello from swift"), Message.Bye(0xCAFE_BABE)] {
            var message = value
            var encoded = BinetteByteBuffer()
            let encodeStatus = withUnsafePointer(to: &message) { pointer in
                binette_local_encode_with_schema_bundle(
                    handle,
                    UnsafePointer(schemaBundle.ptr),
                    schemaBundle.len,
                    UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
                    &encoded
                )
            }
            XCTAssertEqual(encodeStatus, BINETTE_STATUS_OK)
            defer { binette_byte_buffer_free(encoded) }

            let decoded = try decodeMessage(handle: handle, bytes: encoded)
            XCTAssertEqual(decoded, value)
        }

        var rustBytes = BinetteByteBuffer()
        XCTAssertEqual(binette_canary_message_rust_encode_hi(&rustBytes), BINETTE_STATUS_OK)
        defer { binette_byte_buffer_free(rustBytes) }

        let rustValue = try decodeMessage(handle: handle, bytes: rustBytes)
        XCTAssertEqual(rustValue, Message.Hi("hello from rust"))
    }

    // r[verify binette.local-access.boundary]
    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.swift-probes+2]
    func testVoxLikeSwiftDescriptorTreeImportsThroughCAbi() throws {
        let handle = try importVoxLikeRequestDescriptor()
        defer { binette_local_descriptor_free(handle) }
        let schemaBundle = try syntheticSchemaBundle(handle: handle)
        defer { binette_byte_buffer_free(schemaBundle) }

        for value in [
            VoxLikeRequest(
                title: "hello from vox-ish swift",
                note: "optional string branch",
                payload: [0, 1, 2, 3, 255],
                retry: 0xCAFE,
                stream: VoxLikeChannel()
            ),
            VoxLikeRequest(
                title: "none branch",
                note: nil,
                payload: [],
                retry: nil,
                stream: VoxLikeChannel()
            ),
        ] {
            let encoded = try encodeVoxLikeRequest(
                handle: handle,
                schemaBundle: schemaBundle,
                value
            )
            defer { binette_byte_buffer_free(encoded) }

            let decoded = try decodeVoxLikeRequest(
                handle: handle,
                schemaBundle: schemaBundle,
                bytes: encoded
            )
            XCTAssertEqual(decoded, value)
        }
    }

    // r[verify binette.local-access.boundary]
    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.swift-probes+2]
    func testSwiftTupleDescriptorUsesTupleSchemaThroughCAbi() throws {
        let handle = try importVoxLikeArgsDescriptor()
        defer { binette_local_descriptor_free(handle) }
        let schemaBundle = try syntheticSchemaBundle(handle: handle)
        defer { binette_byte_buffer_free(schemaBundle) }

        var args = VoxLikeArgs(title: "tuple-shaped method args", count: 0xCAFE_BABE)
        var encoded = BinetteByteBuffer()
        let encodeStatus = withUnsafePointer(to: &args) { pointer in
            binette_local_encode_with_schema_bundle(
                handle,
                UnsafePointer(schemaBundle.ptr),
                schemaBundle.len,
                UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
                &encoded
            )
        }
        XCTAssertEqual(encodeStatus, BINETTE_STATUS_OK)
        defer { binette_byte_buffer_free(encoded) }

        let out = UnsafeMutablePointer<VoxLikeArgs>.allocate(capacity: 1)
        let decodeStatus = binette_local_decode_with_schema_bundles(
            handle,
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            UnsafePointer(encoded.ptr),
            encoded.len,
            UnsafeMutableRawPointer(out).assumingMemoryBound(to: UInt8.self)
        )
        XCTAssertEqual(decodeStatus, BINETTE_STATUS_OK)
        if decodeStatus == BINETTE_STATUS_OK {
            let decoded = out.move()
            XCTAssertEqual(decoded, args)
        }
        out.deallocate()
    }

    // r[verify binette.local-access.boundary]
    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.swift-probes+2]
    func testSwiftRootStringDescriptorCrossesRustCAbi() throws {
        let arena = BinetteCAbiDescriptorArena()
        let handle = try importDescriptor(arena.string())
        defer { binette_local_descriptor_free(handle) }
        let schemaBundle = try syntheticSchemaBundle(handle: handle)
        defer { binette_byte_buffer_free(schemaBundle) }

        var value = "root string"
        var encoded = BinetteByteBuffer()
        let encodeStatus = withUnsafePointer(to: &value) { pointer in
            binette_local_encode_with_schema_bundle(
                handle,
                UnsafePointer(schemaBundle.ptr),
                schemaBundle.len,
                UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
                &encoded
            )
        }
        XCTAssertEqual(encodeStatus, BINETTE_STATUS_OK)
        defer { binette_byte_buffer_free(encoded) }

        let out = UnsafeMutablePointer<String>.allocate(capacity: 1)
        let decodeStatus = binette_local_decode_with_schema_bundles(
            handle,
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            UnsafePointer(encoded.ptr),
            encoded.len,
            UnsafeMutableRawPointer(out).assumingMemoryBound(to: UInt8.self)
        )
        XCTAssertEqual(decodeStatus, BINETTE_STATUS_OK)
        if decodeStatus == BINETTE_STATUS_OK {
            let decoded = out.move()
            XCTAssertEqual(decoded, value)
        }
        out.deallocate()
    }
}

private func importMessageDescriptor() throws -> OpaquePointer {
    let arena = BinetteCAbiDescriptorArena()
    let stringDescriptor = arena.string()
    let u32Descriptor = arena.plain(typeID: binette_primitive_u32_type_id(), UInt32.self)
    let descriptor = arena.enumeration(
        typeID: binette_canary_message_type_id(),
        layout: binetteLayout(of: Message.self),
        tag: BinetteLocalEnumTagAccessAbi(
            tag: UInt32(BINETTE_LOCAL_ACCESS_THUNK),
            direct_offset: 0,
            thunk: BinetteLocalEnumTagThunkAbi(call: messageTag, context: nil)
        ),
        variants: [
            BinetteLocalVariantAbi(
                name: binetteLocalStr("Hi"),
                index: 0,
                project: projectAccess(messageProjectHiBorrowed),
                project_into: BinetteLocalVariantProjectIntoAbi(
                    call: messageProjectHiInto,
                    context: nil
                ),
                drop_projected: BinetteLocalVariantDropAbi(
                    call: dropProjectedString,
                    context: nil
                ),
                construct: BinetteLocalVariantConstructAbi(
                    call: messageConstructHi,
                    context: nil
                ),
                payload_kind: UInt32(BINETTE_LOCAL_VARIANT_PAYLOAD_NEWTYPE),
                payload: stringDescriptor
            ),
            BinetteLocalVariantAbi(
                name: binetteLocalStr("Bye"),
                index: 1,
                project: projectAccess(messageProjectByeBorrowed),
                project_into: BinetteLocalVariantProjectIntoAbi(
                    call: messageProjectByeInto,
                    context: nil
                ),
                drop_projected: BinetteLocalVariantDropAbi(
                    call: nil,
                    context: nil
                ),
                construct: BinetteLocalVariantConstructAbi(
                    call: messageConstructBye,
                    context: nil
                ),
                payload_kind: UInt32(BINETTE_LOCAL_VARIANT_PAYLOAD_NEWTYPE),
                payload: u32Descriptor
            ),
        ]
    )
    return try importDescriptor(descriptor)
}

private func importVoxLikeRequestDescriptor() throws -> OpaquePointer {
    let arena = BinetteCAbiDescriptorArena()
    let stringDescriptor = arena.string()
    let bytesDescriptor = arena.bytes()
    let optionalStringDescriptor = arena.option(
        typeID: 0xB1_0000_0000_0003,
        layout: binetteLayout(of: String?.self),
        some: stringDescriptor,
        representation: binetteThunkOptionalStringRepresentation()
    )
    let u16Descriptor = arena.plain(typeID: binette_primitive_u16_type_id(), UInt16.self)
    let optionalU16Descriptor = arena.option(
        typeID: 0xB1_0000_0000_0002,
        layout: binetteLayout(of: UInt16?.self),
        some: u16Descriptor,
        representation: binetteDirectOptionalU16Representation()
    )
    let channelDescriptor = arena.externalAttachment(
        typeID: 0xB1_0000_0000_0001,
        kind: "vox.channel",
        layout: binetteLayout(of: VoxLikeChannel.self),
        metadataFields: [
            arena.externalMetadataField(
                "direction",
                arena.externalMetadataString("tx")
            ),
            arena.externalMetadataField(
                "element",
                arena.externalMetadataTypeRef(u16Descriptor)
            ),
        ]
    )
    let descriptor = arena.structure(
        typeID: 0xB1_0000_0000_1000,
        layout: binetteLayout(of: VoxLikeRequest.self),
        fields: [
            BinetteLocalFieldAbi(
                name: binetteLocalStr("title"),
                offset: MemoryLayout<VoxLikeRequest>.offset(of: \VoxLikeRequest.title)!,
                descriptor: stringDescriptor
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("note"),
                offset: MemoryLayout<VoxLikeRequest>.offset(of: \VoxLikeRequest.note)!,
                descriptor: optionalStringDescriptor
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("payload"),
                offset: MemoryLayout<VoxLikeRequest>.offset(of: \VoxLikeRequest.payload)!,
                descriptor: bytesDescriptor
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("retry"),
                offset: MemoryLayout<VoxLikeRequest>.offset(of: \VoxLikeRequest.retry)!,
                descriptor: optionalU16Descriptor
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("stream"),
                offset: MemoryLayout<VoxLikeRequest>.offset(of: \VoxLikeRequest.stream)!,
                descriptor: channelDescriptor
            ),
        ]
    )
    return try importDescriptor(descriptor)
}

private func importVoxLikeArgsDescriptor() throws -> OpaquePointer {
    let arena = BinetteCAbiDescriptorArena()
    let stringDescriptor = arena.string()
    let u32Descriptor = arena.plain(typeID: binette_primitive_u32_type_id(), UInt32.self)
    let descriptor = arena.tuple(
        typeID: 0xB1_0000_0000_2000,
        layout: binetteLayout(of: VoxLikeArgs.self),
        fields: [
            BinetteLocalFieldAbi(
                name: binetteLocalStr("0"),
                offset: MemoryLayout<VoxLikeArgs>.offset(of: \VoxLikeArgs.title)!,
                descriptor: stringDescriptor
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("1"),
                offset: MemoryLayout<VoxLikeArgs>.offset(of: \VoxLikeArgs.count)!,
                descriptor: u32Descriptor
            ),
        ]
    )
    return try importDescriptor(descriptor)
}

private func decodeMessage(
    handle: OpaquePointer,
    bytes: BinetteByteBuffer
) throws -> Message {
    let schemaBundle = try canaryMessageSchemaBundle()
    defer { binette_byte_buffer_free(schemaBundle) }
    let out = UnsafeMutablePointer<Message>.allocate(capacity: 1)
    let status = binette_local_decode_with_schema_bundles(
        handle,
        UnsafePointer(schemaBundle.ptr),
        schemaBundle.len,
        UnsafePointer(schemaBundle.ptr),
        schemaBundle.len,
        UnsafePointer(bytes.ptr),
        bytes.len,
        UnsafeMutableRawPointer(out).assumingMemoryBound(to: UInt8.self)
    )
    XCTAssertEqual(status, BINETTE_STATUS_OK)
    if status != BINETTE_STATUS_OK {
        out.deallocate()
        throw NSError(domain: "BinetteCAbiCanaryTests", code: Int(status))
    }
    let value = out.move()
    out.deallocate()
    return value
}

private func encodeVoxLikeRequest(
    handle: OpaquePointer,
    schemaBundle: BinetteByteBuffer,
    _ value: VoxLikeRequest
) throws -> BinetteByteBuffer {
    var request = value
    var encoded = BinetteByteBuffer()
    let status = withUnsafePointer(to: &request) { pointer in
        binette_local_encode_with_schema_bundle(
            handle,
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
            &encoded
        )
    }
    XCTAssertEqual(status, BINETTE_STATUS_OK)
    if status != BINETTE_STATUS_OK {
        throw NSError(domain: "BinetteCAbiCanaryTests", code: Int(status))
    }
    return encoded
}

private func decodeVoxLikeRequest(
    handle: OpaquePointer,
    schemaBundle: BinetteByteBuffer,
    bytes: BinetteByteBuffer
) throws -> VoxLikeRequest {
    let out = UnsafeMutablePointer<VoxLikeRequest>.allocate(capacity: 1)
    let status = binette_local_decode_with_schema_bundles(
        handle,
        UnsafePointer(schemaBundle.ptr),
        schemaBundle.len,
        UnsafePointer(schemaBundle.ptr),
        schemaBundle.len,
        UnsafePointer(bytes.ptr),
        bytes.len,
        UnsafeMutableRawPointer(out).assumingMemoryBound(to: UInt8.self)
    )
    XCTAssertEqual(status, BINETTE_STATUS_OK)
    if status != BINETTE_STATUS_OK {
        out.deallocate()
        throw NSError(domain: "BinetteCAbiCanaryTests", code: Int(status))
    }
    let value = out.move()
    out.deallocate()
    return value
}

private func canaryMessageSchemaBundle() throws -> BinetteByteBuffer {
    var schemaBundle = BinetteByteBuffer()
    let status = binette_canary_message_schema_bundle(&schemaBundle)
    XCTAssertEqual(status, BINETTE_STATUS_OK)
    if status != BINETTE_STATUS_OK {
        throw NSError(domain: "BinetteCAbiCanaryTests", code: Int(status))
    }
    return schemaBundle
}

private func syntheticSchemaBundle(handle: OpaquePointer) throws -> BinetteByteBuffer {
    var schemaBundle = BinetteByteBuffer()
    let status = binette_local_descriptor_synthetic_schema_bundle(handle, &schemaBundle)
    XCTAssertEqual(status, BINETTE_STATUS_OK)
    if status != BINETTE_STATUS_OK {
        throw NSError(domain: "BinetteCAbiCanaryTests", code: Int(status))
    }
    return schemaBundle
}

private func importDescriptor(
    _ descriptor: UnsafePointer<BinetteLocalDescriptorAbi>
) throws -> OpaquePointer {
    var handle: OpaquePointer?
    let status = binette_local_descriptor_import(descriptor, &handle)
    XCTAssertEqual(status, BINETTE_STATUS_OK)
    return try XCTUnwrap(handle)
}

private func projectAccess(_ thunk: @escaping CVariantProject) -> BinetteLocalVariantProjectAccessAbi {
    BinetteLocalVariantProjectAccessAbi(
        tag: UInt32(BINETTE_LOCAL_ACCESS_THUNK),
        direct_offset: 0,
        thunk: BinetteLocalVariantProjectThunkAbi(call: thunk, context: nil)
    )
}

private func messageTag(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> UInt32 {
    switch UnsafeRawPointer(value!).assumingMemoryBound(to: Message.self).pointee {
    case .Hi:
        return 0
    case .Bye:
        return 1
    }
}

private func messageProjectHiBorrowed(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> UnsafePointer<UInt8>? {
    nil
}

private func messageProjectByeBorrowed(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> UnsafePointer<UInt8>? {
    nil
}

private func messageProjectHiInto(
    _ value: UnsafePointer<UInt8>?,
    _ out: UnsafeMutablePointer<UInt8>?,
    _ outLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    guard outLen == MemoryLayout<String>.size else { return false }
    let message = UnsafeRawPointer(value!).assumingMemoryBound(to: Message.self).pointee
    guard case let .Hi(text) = message else { return false }
    UnsafeMutableRawPointer(out!).assumingMemoryBound(to: String.self).initialize(to: text)
    return true
}

private func dropProjectedString(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) {
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: String.self).deinitialize(count: 1)
}

private func messageProjectByeInto(
    _ value: UnsafePointer<UInt8>?,
    _ out: UnsafeMutablePointer<UInt8>?,
    _ outLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    guard outLen == MemoryLayout<UInt32>.size else { return false }
    let message = UnsafeRawPointer(value!).assumingMemoryBound(to: Message.self).pointee
    guard case let .Bye(code) = message else { return false }
    UnsafeMutableRawPointer(out!).assumingMemoryBound(to: UInt32.self).initialize(to: code)
    return true
}

private func messageConstructHi(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ payload: UnsafePointer<UInt8>?,
    _ payloadLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    let bytes = UnsafeBufferPointer(start: payload, count: payloadLen)
    guard let text = String(bytes: bytes, encoding: .utf8) else { return false }
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: Message.self).initialize(to: .Hi(text))
    return true
}

private func messageConstructBye(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ payload: UnsafePointer<UInt8>?,
    _ payloadLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    guard payloadLen == MemoryLayout<UInt32>.size else { return false }
    var code: UInt32 = 0
    withUnsafeMutableBytes(of: &code) { out in
        out.copyMemory(from: UnsafeRawBufferPointer(start: payload, count: payloadLen))
    }
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: Message.self).initialize(to: .Bye(code))
    return true
}
