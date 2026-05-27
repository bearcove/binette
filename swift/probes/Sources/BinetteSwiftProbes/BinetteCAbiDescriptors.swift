import CBinette
import Foundation

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.descriptor+2]
public final class BinetteCAbiDescriptorArena {
    private var descriptors: [UnsafeMutablePointer<BinetteLocalDescriptorAbi>] = []
    private var fieldArrays: [(UnsafeMutablePointer<BinetteLocalFieldAbi>, Int)] = []
    private var variantArrays: [(UnsafeMutablePointer<BinetteLocalVariantAbi>, Int)] = []

    public init() {}

    deinit {
        for descriptor in descriptors {
            descriptor.deinitialize(count: 1)
            descriptor.deallocate()
        }
        for (fields, count) in fieldArrays {
            fields.deinitialize(count: count)
            fields.deallocate()
        }
        for (variants, count) in variantArrays {
            variants.deinitialize(count: count)
            variants.deallocate()
        }
    }

    public func plain<T>(
        typeID: UInt64,
        _: T.Type
    ) -> UnsafePointer<BinetteLocalDescriptorAbi> {
        store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(typeID),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: binetteLayout(of: T.self),
                kind: scalarKind(
                    tag: UInt32(BINETTE_LOCAL_SCALAR_PLAIN),
                    storage: emptySequenceStorage()
                )
            )
        )
    }

    public func string() -> UnsafePointer<BinetteLocalDescriptorAbi> {
        store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(binette_primitive_string_type_id()),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: binetteLayout(of: String.self),
                kind: scalarKind(
                    tag: UInt32(BINETTE_LOCAL_SCALAR_STRING),
                    storage: stringStorage()
                )
            )
        )
    }

    public func bytes() -> UnsafePointer<BinetteLocalDescriptorAbi> {
        store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(binette_primitive_bytes_type_id()),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: binetteLayout(of: [UInt8].self),
                kind: scalarKind(
                    tag: UInt32(BINETTE_LOCAL_SCALAR_BYTES),
                    storage: byteArrayStorage()
                )
            )
        )
    }

    public func option(
        typeID: UInt64,
        layout: BinetteLocalLayoutAbi,
        some: UnsafePointer<BinetteLocalDescriptorAbi>,
        representation: BinetteLocalOptionRepresentationAbi
    ) -> UnsafePointer<BinetteLocalDescriptorAbi> {
        store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(typeID),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: layout,
                kind: optionKind(some: some, representation: representation)
            )
        )
    }

    public func externalAttachment(
        typeID: UInt64,
        kind attachmentKind: StaticString,
        layout: BinetteLocalLayoutAbi
    ) -> UnsafePointer<BinetteLocalDescriptorAbi> {
        var kind = emptyKind()
        kind.tag = UInt32(BINETTE_LOCAL_KIND_EXTERNAL_ATTACHMENT)
        kind.text = binetteLocalStr(attachmentKind)
        return store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(typeID),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: layout,
                kind: kind
            )
        )
    }

    public func structure(
        typeID: UInt64,
        layout: BinetteLocalLayoutAbi,
        fields: [BinetteLocalFieldAbi]
    ) -> UnsafePointer<BinetteLocalDescriptorAbi> {
        let fieldPointer = storeFields(fields)
        var kind = emptyKind()
        kind.tag = UInt32(BINETTE_LOCAL_KIND_STRUCT)
        kind.structure = BinetteLocalStructAbi(
            fields: UnsafePointer(fieldPointer),
            field_count: fields.count
        )
        return store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(typeID),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: layout,
                kind: kind
            )
        )
    }

    public func tuple(
        typeID: UInt64,
        layout: BinetteLocalLayoutAbi,
        fields: [BinetteLocalFieldAbi]
    ) -> UnsafePointer<BinetteLocalDescriptorAbi> {
        let fieldPointer = storeFields(fields)
        var kind = emptyKind()
        kind.tag = UInt32(BINETTE_LOCAL_KIND_TUPLE)
        kind.tuple = BinetteLocalStructAbi(
            fields: UnsafePointer(fieldPointer),
            field_count: fields.count
        )
        return store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(typeID),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: layout,
                kind: kind
            )
        )
    }

    public func enumeration(
        typeID: UInt64,
        layout: BinetteLocalLayoutAbi,
        tag: BinetteLocalEnumTagAccessAbi,
        variants: [BinetteLocalVariantAbi]
    ) -> UnsafePointer<BinetteLocalDescriptorAbi> {
        let variantPointer = storeVariants(variants)
        var kind = emptyKind()
        kind.tag = UInt32(BINETTE_LOCAL_KIND_ENUM)
        kind.enumeration = BinetteLocalEnumAbi(
            tag: tag,
            variants: UnsafePointer(variantPointer),
            variant_count: variants.count
        )
        return store(
            BinetteLocalDescriptorAbi(
                schema: binetteTypeSchema(typeID),
                backend: UInt32(BINETTE_LOCAL_BACKEND_SWIFT),
                layout: layout,
                kind: kind
            )
        )
    }

    private func store(
        _ descriptor: BinetteLocalDescriptorAbi
    ) -> UnsafePointer<BinetteLocalDescriptorAbi> {
        let pointer = UnsafeMutablePointer<BinetteLocalDescriptorAbi>.allocate(capacity: 1)
        pointer.initialize(to: descriptor)
        descriptors.append(pointer)
        return UnsafePointer(pointer)
    }

    private func storeFields(
        _ fields: [BinetteLocalFieldAbi]
    ) -> UnsafeMutablePointer<BinetteLocalFieldAbi> {
        let pointer = UnsafeMutablePointer<BinetteLocalFieldAbi>.allocate(capacity: fields.count)
        pointer.initialize(from: fields, count: fields.count)
        fieldArrays.append((pointer, fields.count))
        return pointer
    }

    private func storeVariants(
        _ variants: [BinetteLocalVariantAbi]
    ) -> UnsafeMutablePointer<BinetteLocalVariantAbi> {
        let pointer = UnsafeMutablePointer<BinetteLocalVariantAbi>.allocate(capacity: variants.count)
        pointer.initialize(from: variants, count: variants.count)
        variantArrays.append((pointer, variants.count))
        return pointer
    }
}

public func binetteLayout<T>(of _: T.Type) -> BinetteLocalLayoutAbi {
    BinetteLocalLayoutAbi(
        size: MemoryLayout<T>.size,
        align: MemoryLayout<T>.alignment,
        stride: MemoryLayout<T>.stride
    )
}

public func binetteLocalStr(_ value: StaticString) -> BinetteLocalStrAbi {
    BinetteLocalStrAbi(ptr: value.utf8Start, len: value.utf8CodeUnitCount)
}

public func binetteTypeSchema(_ typeID: UInt64) -> BinetteLocalSchemaRefAbi {
    BinetteLocalSchemaRefAbi(
        tag: UInt32(BINETTE_LOCAL_SCHEMA_REF_TYPE),
        type_id: typeID,
        owner_type_id: 0,
        path: BinetteLocalStrAbi(ptr: nil, len: 0)
    )
}

public func binetteDirectOptionalU16Representation() -> BinetteLocalOptionRepresentationAbi {
    let none: UInt16? = nil
    let zero: UInt16? = 0
    let some: UInt16? = 0xCAFE
    let noneBytes = bytes(of: none)
    let zeroBytes = bytes(of: zero)
    let someBytes = bytes(of: some)
    let tagOffset = noneBytes.indices.first {
        noneBytes[$0] != someBytes[$0] && zeroBytes[$0] == someBytes[$0]
    }!

    return BinetteLocalOptionRepresentationAbi(
        tag: UInt32(BINETTE_LOCAL_OPTION_DIRECT_TAG),
        tag_offset: tagOffset,
        tag_width: MemoryLayout<UInt8>.size,
        none_value: Int(noneBytes[tagOffset]),
        some_value: Int(someBytes[tagOffset]),
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
}

public func binetteSwiftStringLen(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> Int {
    UnsafeRawPointer(value!).assumingMemoryBound(to: String.self).pointee.utf8.count
}

public func binetteSwiftStringElement(
    _ value: UnsafePointer<UInt8>?,
    _ index: Int,
    _ context: UnsafeMutableRawPointer?
) -> UInt8 {
    let text = UnsafeRawPointer(value!).assumingMemoryBound(to: String.self).pointee
    return Array(text.utf8)[index]
}

public func binetteSwiftStringWrite(
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

public func binetteSwiftByteArrayLen(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> Int {
    UnsafeRawPointer(value!).assumingMemoryBound(to: [UInt8].self).pointee.count
}

public func binetteSwiftByteArrayElement(
    _ value: UnsafePointer<UInt8>?,
    _ index: Int,
    _ context: UnsafeMutableRawPointer?
) -> UInt8 {
    UnsafeRawPointer(value!).assumingMemoryBound(to: [UInt8].self).pointee[index]
}

public func binetteSwiftByteArrayWrite(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ ptr: UnsafePointer<UInt8>?,
    _ len: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    let bytes = Array(UnsafeBufferPointer(start: ptr, count: len))
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: [UInt8].self).initialize(to: bytes)
    return true
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

private func optionKind(
    some: UnsafePointer<BinetteLocalDescriptorAbi>,
    representation: BinetteLocalOptionRepresentationAbi
) -> BinetteLocalKindAbi {
    var kind = emptyKind()
    kind.tag = UInt32(BINETTE_LOCAL_KIND_OPTION)
    kind.option = BinetteLocalOptionAbi(some: some, representation: representation)
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
        tuple: BinetteLocalStructAbi(fields: nil, field_count: 0),
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

private func stringStorage() -> BinetteLocalSequenceStorageAbi {
    BinetteLocalSequenceStorageAbi(
        tag: UInt32(BINETTE_LOCAL_SEQUENCE_THUNK),
        offset: 0,
        element_count: 0,
        pointer_offset: 0,
        length_offset: 0,
        has_capacity: 0,
        capacity_offset: 0,
        element_stride: MemoryLayout<UInt8>.stride,
        thunks: BinetteLocalSequenceThunksAbi(
            len: binetteSwiftStringLen,
            element_u8: binetteSwiftStringElement,
            element_ptr: nil,
            write_bytes: binetteSwiftStringWrite,
            write_fixed_elements: nil,
            context: nil
        )
    )
}

private func byteArrayStorage() -> BinetteLocalSequenceStorageAbi {
    BinetteLocalSequenceStorageAbi(
        tag: UInt32(BINETTE_LOCAL_SEQUENCE_THUNK),
        offset: 0,
        element_count: 0,
        pointer_offset: 0,
        length_offset: 0,
        has_capacity: 0,
        capacity_offset: 0,
        element_stride: MemoryLayout<UInt8>.stride,
        thunks: BinetteLocalSequenceThunksAbi(
            len: binetteSwiftByteArrayLen,
            element_u8: binetteSwiftByteArrayElement,
            element_ptr: nil,
            write_bytes: binetteSwiftByteArrayWrite,
            write_fixed_elements: nil,
            context: nil
        )
    )
}

private func bytes<T>(of value: T) -> [UInt8] {
    var value = value
    return withUnsafeBytes(of: &value) { Array($0) }
}
