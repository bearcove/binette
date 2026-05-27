import CBinette
import Foundation
import XCTest

private enum Message: Equatable {
    case Hi(String)
    case Bye(UInt32)
}

private typealias CSequenceLen = @convention(c) (UnsafePointer<UInt8>?, UnsafeMutableRawPointer?) -> Int
private typealias CSequenceU8 = @convention(c) (UnsafePointer<UInt8>?, Int, UnsafeMutableRawPointer?) -> UInt8
private typealias CSequenceWriteBytes = @convention(c) (UnsafeMutablePointer<UInt8>?, UnsafePointer<UInt8>?, Int, UnsafeMutableRawPointer?) -> Bool
private typealias CEnumTag = @convention(c) (UnsafePointer<UInt8>?, UnsafeMutableRawPointer?) -> UInt32
private typealias CVariantProject = @convention(c) (UnsafePointer<UInt8>?, UnsafeMutableRawPointer?) -> UnsafePointer<UInt8>?
private typealias CVariantProjectInto = @convention(c) (UnsafePointer<UInt8>?, UnsafeMutablePointer<UInt8>?, Int, UnsafeMutableRawPointer?) -> Bool
private typealias CVariantDropProjected = @convention(c) (UnsafeMutablePointer<UInt8>?, UnsafeMutableRawPointer?) -> Void
private typealias CVariantConstruct = @convention(c) (UnsafeMutablePointer<UInt8>?, UnsafePointer<UInt8>?, Int, UnsafeMutableRawPointer?) -> Bool

final class BinetteCAbiCanaryTests: XCTestCase {
    // r[verify binette.local-access.boundary]
    // r[verify binette.local-access.descriptor+2]
    // r[verify binette.local-access.swift-probes+2]
    func testSwiftMessageCrossesRustCAbiThroughLocalDescriptor() throws {
        let handle = try importMessageDescriptor()
        defer { binette_local_descriptor_free(handle) }

        for value in [Message.Hi("hello from swift"), Message.Bye(0xCAFE_BABE)] {
            var message = value
            var encoded = BinetteByteBuffer()
            let encodeStatus = withUnsafePointer(to: &message) { pointer in
                binette_canary_message_encode(
                    handle,
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
}

private func importMessageDescriptor() throws -> OpaquePointer {
    var stringDescriptor = stringDescriptor()
    var u32Descriptor = plainDescriptor(
        typeID: binette_primitive_u32_type_id(),
        size: MemoryLayout<UInt32>.size,
        align: MemoryLayout<UInt32>.alignment,
        stride: MemoryLayout<UInt32>.stride
    )

    return try withUnsafePointer(to: &stringDescriptor) { stringPtr in
        try withUnsafePointer(to: &u32Descriptor) { u32Ptr in
            let variants = [
                BinetteLocalVariantAbi(
                    name: localStr("Hi"),
                    index: 0,
                    project: projectAccess(messageProjectHiBorrowed),
                    project_into: BinetteLocalVariantProjectIntoAbi(
                        call: cFunction(messageProjectHiInto),
                        context: nil
                    ),
                    drop_projected: BinetteLocalVariantDropAbi(
                        call: cFunction(dropProjectedString),
                        context: nil
                    ),
                    construct: BinetteLocalVariantConstructAbi(
                        call: cFunction(messageConstructHi),
                        context: nil
                    ),
                    payload: stringPtr
                ),
                BinetteLocalVariantAbi(
                    name: localStr("Bye"),
                    index: 1,
                    project: projectAccess(messageProjectByeBorrowed),
                    project_into: BinetteLocalVariantProjectIntoAbi(
                        call: cFunction(messageProjectByeInto),
                        context: nil
                    ),
                    drop_projected: BinetteLocalVariantDropAbi(
                        call: nil,
                        context: nil
                    ),
                    construct: BinetteLocalVariantConstructAbi(
                        call: cFunction(messageConstructBye),
                        context: nil
                    ),
                    payload: u32Ptr
                ),
            ]

            return try variants.withUnsafeBufferPointer { variantsPtr in
                var descriptor = BinetteLocalDescriptorAbi(
                    schema: typeSchema(binette_canary_message_type_id()),
                    backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                    layout: BinetteLocalLayoutAbi(
                        size: MemoryLayout<Message>.size,
                        align: MemoryLayout<Message>.alignment,
                        stride: MemoryLayout<Message>.stride
                    ),
                    kind: enumKind(variants: variantsPtr)
                )
                var handle: OpaquePointer?
                let status = binette_local_descriptor_import(&descriptor, &handle)
                XCTAssertEqual(status, BINETTE_STATUS_OK)
                return try XCTUnwrap(handle)
            }
        }
    }
}

private func decodeMessage(
    handle: OpaquePointer,
    bytes: BinetteByteBuffer
) throws -> Message {
    let out = UnsafeMutablePointer<Message>.allocate(capacity: 1)
    let status = binette_canary_message_decode(
        handle,
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

private func localStr(_ value: StaticString) -> BinetteLocalStrAbi {
    BinetteLocalStrAbi(ptr: value.utf8Start, len: value.utf8CodeUnitCount)
}

private func typeSchema(_ typeID: UInt64) -> BinetteLocalSchemaRefAbi {
    BinetteLocalSchemaRefAbi(
        tag: UInt32(BINETTE_LOCAL_SCHEMA_REF_TYPE),
        type_id: typeID,
        owner_type_id: 0,
        path: BinetteLocalStrAbi(ptr: nil, len: 0)
    )
}

private func plainDescriptor(
    typeID: UInt64,
    size: Int,
    align: Int,
    stride: Int
) -> BinetteLocalDescriptorAbi {
    BinetteLocalDescriptorAbi(
        schema: typeSchema(typeID),
        backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
        layout: BinetteLocalLayoutAbi(size: size, align: align, stride: stride),
        kind: scalarKind(tag: UInt32(BINETTE_LOCAL_SCALAR_PLAIN), storage: emptySequenceStorage())
    )
}

private func stringDescriptor() -> BinetteLocalDescriptorAbi {
    BinetteLocalDescriptorAbi(
        schema: typeSchema(binette_primitive_string_type_id()),
        backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
        layout: BinetteLocalLayoutAbi(
            size: MemoryLayout<String>.size,
            align: MemoryLayout<String>.alignment,
            stride: MemoryLayout<String>.stride
        ),
        kind: scalarKind(
            tag: UInt32(BINETTE_LOCAL_SCALAR_STRING),
            storage: BinetteLocalSequenceStorageAbi(
                tag: UInt32(BINETTE_LOCAL_SEQUENCE_THUNK),
                offset: 0,
                element_count: 0,
                pointer_offset: 0,
                length_offset: 0,
                has_capacity: 0,
                capacity_offset: 0,
                element_stride: 1,
                thunks: BinetteLocalSequenceThunksAbi(
                    len: cFunction(swiftStringLen),
                    element_u8: cFunction(swiftStringElement),
                    element_ptr: nil,
                    write_bytes: cFunction(swiftStringWrite),
                    write_fixed_elements: nil,
                    context: nil
                )
            )
        )
    )
}

private func scalarKind(
    tag: UInt32,
    storage: BinetteLocalSequenceStorageAbi
) -> BinetteLocalKindAbi {
    var kind = emptyKind()
    kind.tag = UInt32(BINETTE_LOCAL_KIND_SCALAR)
    kind.scalar = BinetteLocalScalarAbi(tag: tag, storage: storage)
    return kind
}

private func enumKind(
    variants: UnsafeBufferPointer<BinetteLocalVariantAbi>
) -> BinetteLocalKindAbi {
    var kind = emptyKind()
    kind.tag = UInt32(BINETTE_LOCAL_KIND_ENUM)
    kind.enumeration = BinetteLocalEnumAbi(
        tag: BinetteLocalEnumTagAccessAbi(
            tag: UInt32(BINETTE_LOCAL_ACCESS_THUNK),
            direct_offset: 0,
            thunk: BinetteLocalEnumTagThunkAbi(call: cFunction(messageTag), context: nil)
        ),
        variants: variants.baseAddress,
        variant_count: variants.count
    )
    return kind
}

private func emptyKind() -> BinetteLocalKindAbi {
    BinetteLocalKindAbi(
        tag: UInt32(BINETTE_LOCAL_KIND_SCALAR),
        scalar: BinetteLocalScalarAbi(
            tag: UInt32(BINETTE_LOCAL_SCALAR_PLAIN),
            storage: emptySequenceStorage()
        ),
        structure: BinetteLocalStructAbi(fields: nil, field_count: 0),
        enumeration: BinetteLocalEnumAbi(
            tag: BinetteLocalEnumTagAccessAbi(
                tag: UInt32(BINETTE_LOCAL_ACCESS_DIRECT),
                direct_offset: 0,
                thunk: BinetteLocalEnumTagThunkAbi(call: nil, context: nil)
            ),
            variants: nil,
            variant_count: 0
        ),
        sequence: BinetteLocalSequenceAbi(element: nil, storage: emptySequenceStorage()),
        option: BinetteLocalOptionAbi(
            some: nil,
            representation: BinetteLocalOptionRepresentationAbi(
                tag: UInt32(BINETTE_LOCAL_OPTION_DIRECT_TAG),
                tag_offset: 0,
                tag_width: 1,
                none_value: 0,
                some_value: 1,
                some_offset: 0,
                none_bytes: nil,
                none_bytes_len: 0,
                thunks: BinetteLocalOptionThunksAbi(
                    is_some: nil,
                    some: nil,
                    write_none: nil,
                    write_some_bytes: nil,
                    context: nil
                )
            )
        ),
        text: BinetteLocalStrAbi(ptr: nil, len: 0)
    )
}

private func emptySequenceStorage() -> BinetteLocalSequenceStorageAbi {
    BinetteLocalSequenceStorageAbi(
        tag: UInt32(BINETTE_LOCAL_SEQUENCE_INLINE_FIXED),
        offset: 0,
        element_count: 0,
        pointer_offset: 0,
        length_offset: 0,
        has_capacity: 0,
        capacity_offset: 0,
        element_stride: 0,
        thunks: BinetteLocalSequenceThunksAbi(
            len: nil,
            element_u8: nil,
            element_ptr: nil,
            write_bytes: nil,
            write_fixed_elements: nil,
            context: nil
        )
    )
}

private func projectAccess(_ thunk: @escaping CVariantProject) -> BinetteLocalVariantProjectAccessAbi {
    BinetteLocalVariantProjectAccessAbi(
        tag: UInt32(BINETTE_LOCAL_ACCESS_THUNK),
        direct_offset: 0,
        thunk: BinetteLocalVariantProjectThunkAbi(call: cFunction(thunk), context: nil)
    )
}

private func cFunction(_ function: CSequenceLen) -> UnsafeRawPointer {
    unsafeBitCast(function, to: UnsafeRawPointer.self)
}

private func cFunction(_ function: CSequenceU8) -> UnsafeRawPointer {
    unsafeBitCast(function, to: UnsafeRawPointer.self)
}

private func cFunction(_ function: CSequenceWriteBytes) -> UnsafeRawPointer {
    unsafeBitCast(function, to: UnsafeRawPointer.self)
}

private func cFunction(_ function: CEnumTag) -> UnsafeRawPointer {
    unsafeBitCast(function, to: UnsafeRawPointer.self)
}

private func cFunction(_ function: CVariantProject) -> UnsafeRawPointer {
    unsafeBitCast(function, to: UnsafeRawPointer.self)
}

private func cFunction(_ function: CVariantProjectInto) -> UnsafeRawPointer {
    unsafeBitCast(function, to: UnsafeRawPointer.self)
}

private func cFunction(_ function: CVariantDropProjected) -> UnsafeRawPointer {
    unsafeBitCast(function, to: UnsafeRawPointer.self)
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

private func swiftStringLen(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> Int {
    UnsafeRawPointer(value!).assumingMemoryBound(to: String.self).pointee.utf8.count
}

private func swiftStringElement(
    _ value: UnsafePointer<UInt8>?,
    _ index: Int,
    _ context: UnsafeMutableRawPointer?
) -> UInt8 {
    let text = UnsafeRawPointer(value!).assumingMemoryBound(to: String.self).pointee
    return Array(text.utf8)[index]
}

private func swiftStringWrite(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ ptr: UnsafePointer<UInt8>?,
    _ len: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    let bytes = UnsafeBufferPointer(start: ptr, count: len)
    guard let text = String(bytes: bytes, encoding: .utf8) else { return false }
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: String.self).initialize(to: text)
    return true
}
