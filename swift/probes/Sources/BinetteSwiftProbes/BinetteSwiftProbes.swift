public enum BinetteProbeBackend: Equatable {
    case swiftProbe
}

// r[impl binette.local-access.descriptor+2]
public struct BinetteLocalLayout: Equatable {
    public var size: Int
    public var alignment: Int
    public var stride: Int

    public init<T>(of _: T.Type) {
        size = MemoryLayout<T>.size
        alignment = MemoryLayout<T>.alignment
        stride = MemoryLayout<T>.stride
    }
}

// r[impl binette.local-access.boundary]
// r[impl binette.local-access.descriptor+2]
public struct BinetteLocalDescriptor: Equatable {
    public var schemaName: String
    public var backend: BinetteProbeBackend
    public var layout: BinetteLocalLayout
    public var kind: BinetteLocalKind
}

public enum BinetteScalarAccess: Equatable {
    case plain
    case string(storage: BinetteSequenceStorage)
    case bytes(storage: BinetteSequenceStorage)
}

public indirect enum BinetteLocalKind: Equatable {
    case scalar(BinetteScalarAccess)
    case storedStruct(fields: [BinetteLocalField])
    case enumPayloads(tag: BinetteLocalAccess, variants: [BinetteLocalVariant])
    case optional(some: BinetteLocalDescriptor, storage: BinetteOptionalStorage)
    case sequence(element: BinetteLocalDescriptor, storage: BinetteSequenceStorage)
    case opaque(reason: String)
}

public struct BinetteLocalField: Equatable {
    public var name: String
    public var access: BinetteLocalAccess
    public var descriptor: BinetteLocalDescriptor
}

public struct BinetteLocalVariant: Equatable {
    public var name: String
    public var index: UInt32
    public var access: BinetteLocalAccess
    public var construct: String?
    public var payload: BinetteLocalDescriptor?
}

public enum BinetteLocalAccess: Equatable {
    case direct(offset: Int)
    case thunk(String)
}

public enum BinetteSequenceStorage: Equatable {
    case directContiguous(pointerOffset: Int, countOffset: Int, elementStride: Int)
    case thunk(count: String, element: String, write: String?)
}

public enum BinetteOptionalStorage: Equatable {
    case directTag(offset: Int, width: Int, noneValue: UInt, someValue: UInt, someOffset: Int)
    case nicheTag(offset: Int, width: Int, noneValue: UInt, someOffset: Int)
    case thunk(isSome: String, some: String, writeNone: String?, writeSomeBytes: String?)
}

public struct ProbeLeaf {
    public var count: Int32
    public var flag: Bool

    public init(count: Int32, flag: Bool) {
        self.count = count
        self.flag = flag
    }
}

public struct ProbeNested {
    public var title: String
    public var leaf: ProbeLeaf
    public var values: [Int64]

    public init(title: String, leaf: ProbeLeaf, values: [Int64]) {
        self.title = title
        self.leaf = leaf
        self.values = values
    }
}

public enum ProbeEnum {
    case empty
    case titled(String)
    case nested(ProbeLeaf)
}

public func makeProbeDescriptors() -> [BinetteLocalDescriptor] {
    let bool = scalarDescriptor("bool", Bool.self)
    let uint8 = scalarDescriptor("u8", UInt8.self)
    let uint16 = scalarDescriptor("u16", UInt16.self)
    let int32 = scalarDescriptor("i32", Int32.self)
    let int64 = scalarDescriptor("i64", Int64.self)
    let string = stringDescriptor()
    let array = arrayDescriptor(element: int64)
    let optionalString = optionalDescriptor("option<string>", String?.self, some: string)
    let optionalBool = optionalBoolDescriptor(some: bool)
    let optionalUInt16 = optionalUInt16Descriptor(some: uint16)
    let leaf = leafDescriptor(count: int32, flag: bool)
    let nested = nestedDescriptor(title: string, leaf: leaf, values: array)
    let enumPayloads = enumDescriptor()

    return [
        bool,
        uint8,
        int32,
        int64,
        string,
        array,
        optionalString,
        optionalBool,
        optionalUInt16,
        leaf,
        nested,
        enumPayloads,
    ]
}

// r[impl binette.local-access.backends]
// r[impl binette.local-access.swift-probes+2]
public func validateProbeDescriptors(_ descriptors: [BinetteLocalDescriptor]) -> Bool {
    let names = Set(descriptors.map(\.schemaName))
    return [
        "ProbeLeaf",
        "ProbeNested",
        "ProbeEnum",
        "string",
        "array<i64>",
        "option<string>",
        "option<bool>",
        "option<u16>",
    ].allSatisfy(names.contains)
}

// r[impl binette.local-access.runtime-facts]
// r[impl binette.local-access.swift-probes+2]
public func validateProbeRuntimeFacts() -> Bool {
    let descriptors = makeProbeDescriptors()
    guard
        let leaf = descriptors.first(where: { $0.schemaName == "ProbeLeaf" }),
        let nested = descriptors.first(where: { $0.schemaName == "ProbeNested" }),
        let optionalBool = descriptors.first(where: { $0.schemaName == "option<bool>" }),
        let optionalUInt16 = descriptors.first(where: { $0.schemaName == "option<u16>" })
    else {
        return false
    }

    let leafValue = ProbeLeaf(count: -12_345, flag: true)
    guard
        loadStructField(leaf, "count", from: leafValue, as: Int32.self) == leafValue.count,
        loadStructField(leaf, "flag", from: leafValue, as: Bool.self) == leafValue.flag
    else {
        return false
    }

    let nestedValue = ProbeNested(
        title: "runtime facts",
        leaf: ProbeLeaf(count: 67_890, flag: false),
        values: [1, 2, 3]
    )
    guard
        loadNestedLeafField(nested, "count", from: nestedValue, as: Int32.self) == nestedValue.leaf.count,
        loadNestedLeafField(nested, "flag", from: nestedValue, as: Bool.self) == nestedValue.leaf.flag
    else {
        return false
    }

    return validateNicheOptionalBool(optionalBool)
        && validateDirectOptionalUInt16(optionalUInt16)
}

private func loadStructField<Root, Field>(
    _ descriptor: BinetteLocalDescriptor,
    _ fieldName: String,
    from value: Root,
    as _: Field.Type
) -> Field? {
    guard
        case let .storedStruct(fields) = descriptor.kind,
        let field = fields.first(where: { $0.name == fieldName }),
        case let .direct(offset) = field.access
    else {
        return nil
    }
    return loadValue(from: value, offset: offset, as: Field.self)
}

private func loadNestedLeafField<Field>(
    _ descriptor: BinetteLocalDescriptor,
    _ fieldName: String,
    from value: ProbeNested,
    as _: Field.Type
) -> Field? {
    guard
        case let .storedStruct(fields) = descriptor.kind,
        let leafField = fields.first(where: { $0.name == "leaf" }),
        case let .direct(leafOffset) = leafField.access,
        case let .storedStruct(leafFields) = leafField.descriptor.kind,
        let field = leafFields.first(where: { $0.name == fieldName }),
        case let .direct(fieldOffset) = field.access
    else {
        return nil
    }
    return loadValue(from: value, offset: leafOffset + fieldOffset, as: Field.self)
}

private func loadDirectOptionalTag(
    _ descriptor: BinetteLocalDescriptor,
    from value: UInt16?
) -> UInt8? {
    guard
        case let .optional(_, storage) = descriptor.kind,
        case let .directTag(offset, width, _, _, _) = storage,
        width == MemoryLayout<UInt8>.size
    else {
        return nil
    }
    return loadValue(from: value, offset: offset, as: UInt8.self)
}

private func loadDirectOptionalPayload<Field>(
    _ descriptor: BinetteLocalDescriptor,
    from value: UInt16?,
    as _: Field.Type
) -> Field? {
    guard
        case let .optional(_, storage) = descriptor.kind,
        case let .directTag(_, _, _, _, someOffset) = storage
    else {
        return nil
    }
    return loadValue(from: value, offset: someOffset, as: Field.self)
}

private func directOptionalTagValues(
    _ descriptor: BinetteLocalDescriptor
) -> (none: UInt, some: UInt)? {
    guard
        case let .optional(_, storage) = descriptor.kind,
        case let .directTag(_, _, noneValue, someValue, _) = storage
    else {
        return nil
    }
    return (none: noneValue, some: someValue)
}

private func nicheOptionalNoneValue(
    _ descriptor: BinetteLocalDescriptor
) -> UInt? {
    guard
        case let .optional(_, storage) = descriptor.kind,
        case let .nicheTag(_, _, noneValue, _) = storage
    else {
        return nil
    }
    return noneValue
}

private func validateNicheOptionalBool(_ descriptor: BinetteLocalDescriptor) -> Bool {
    let none: Bool? = nil
    let someFalse: Bool? = false
    let someTrue: Bool? = true
    guard
        case let .optional(_, storage) = descriptor.kind,
        case let .nicheTag(tagOffset, width, noneValue, someOffset) = storage,
        width == MemoryLayout<UInt8>.size,
        nicheOptionalNoneValue(descriptor) == noneValue,
        loadValue(from: none, offset: tagOffset, as: UInt8.self) == UInt8(noneValue),
        loadValue(from: someFalse, offset: tagOffset, as: UInt8.self) != UInt8(noneValue),
        loadValue(from: someTrue, offset: tagOffset, as: UInt8.self) != UInt8(noneValue),
        loadValue(from: someFalse, offset: someOffset, as: Bool.self) == false,
        loadValue(from: someTrue, offset: someOffset, as: Bool.self) == true
    else {
        return false
    }
    return true
}

private func validateDirectOptionalUInt16(_ descriptor: BinetteLocalDescriptor) -> Bool {
    let none: UInt16? = nil
    let some: UInt16? = 0xCAFE
    guard
        let tagValues = directOptionalTagValues(descriptor),
        let noneTag = loadDirectOptionalTag(descriptor, from: none),
        let someTag = loadDirectOptionalTag(descriptor, from: some),
        tagValues.none == UInt(noneTag),
        tagValues.some == UInt(someTag),
        loadDirectOptionalPayload(descriptor, from: some, as: UInt16.self) == 0xCAFE
    else {
        return false
    }
    return noneTag != someTag
}

private func loadValue<Root, Field>(from value: Root, offset: Int, as _: Field.Type) -> Field {
    var value = value
    return withUnsafeBytes(of: &value) { bytes in
        bytes.baseAddress!.advanced(by: offset).load(as: Field.self)
    }
}

private func scalarDescriptor<T>(_ name: String, _: T.Type) -> BinetteLocalDescriptor {
    return BinetteLocalDescriptor(
        schemaName: name,
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: T.self),
        kind: .scalar(.plain)
    )
}

private func stringDescriptor() -> BinetteLocalDescriptor {
    BinetteLocalDescriptor(
        schemaName: "string",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: String.self),
        kind: .scalar(
            .string(
                storage: .thunk(
                    count: "Swift.String.utf8.count",
                    element: "Swift.String.utf8.element",
                    write: "Swift.String.init.utf8"
                )
            )
        )
    )
}

// r[impl binette.local-access.backends]
private func arrayDescriptor(element: BinetteLocalDescriptor) -> BinetteLocalDescriptor {
    BinetteLocalDescriptor(
        schemaName: "array<i64>",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: [Int64].self),
        kind: .sequence(
            element: element,
            storage: .thunk(
                count: "Swift.Array.count",
                element: "Swift.Array.element",
                write: "Swift.Array.init.elements"
            )
        )
    )
}

private func optionalDescriptor<T>(
    _ name: String,
    _: T.Type,
    some: BinetteLocalDescriptor
) -> BinetteLocalDescriptor {
    BinetteLocalDescriptor(
        schemaName: name,
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: T.self),
        kind: .optional(
            some: some,
            storage: .thunk(
                isSome: "Swift.Optional.isSome",
                some: "Swift.Optional.some",
                writeNone: "Swift.Optional.init.none",
                writeSomeBytes: "Swift.Optional<String>.init.some.utf8"
            )
        )
    )
}

private func optionalBoolDescriptor(some: BinetteLocalDescriptor) -> BinetteLocalDescriptor {
    let storage = probeOptionalBoolStorage()
    return BinetteLocalDescriptor(
        schemaName: "option<bool>",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: Optional<Bool>.self),
        kind: .optional(some: some, storage: storage)
    )
}

// r[impl binette.local-access.runtime-facts]
private func probeOptionalBoolStorage() -> BinetteOptionalStorage {
    let none: Bool? = nil
    let someFalse: Bool? = false
    let someTrue: Bool? = true
    let noneBytes = bytes(of: none)
    let falseBytes = bytes(of: someFalse)
    let trueBytes = bytes(of: someTrue)

    if
        let tagOffset = noneBytes.indices.first(where: {
            noneBytes[$0] != falseBytes[$0] && noneBytes[$0] != trueBytes[$0]
        }),
        loadValue(from: someFalse, offset: 0, as: Bool.self) == false,
        loadValue(from: someTrue, offset: 0, as: Bool.self) == true
    {
        return .nicheTag(
            offset: tagOffset,
            width: MemoryLayout<UInt8>.size,
            noneValue: UInt(noneBytes[tagOffset]),
            someOffset: 0
        )
    }

    return .thunk(
        isSome: "Swift.Optional<Bool>.isSome",
        some: "Swift.Optional<Bool>.some",
        writeNone: "Swift.Optional<Bool>.init.none",
        writeSomeBytes: "Swift.Optional<Bool>.init.some.bytes"
    )
}

private func optionalUInt16Descriptor(some: BinetteLocalDescriptor) -> BinetteLocalDescriptor {
    let storage = probeOptionalUInt16Storage()
    return BinetteLocalDescriptor(
        schemaName: "option<u16>",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: Optional<UInt16>.self),
        kind: .optional(some: some, storage: storage)
    )
}

// r[impl binette.local-access.runtime-facts]
private func probeOptionalUInt16Storage() -> BinetteOptionalStorage {
    let none: UInt16? = nil
    let zero: UInt16? = 0
    let some: UInt16? = 0xCAFE
    let noneBytes = bytes(of: none)
    let zeroBytes = bytes(of: zero)
    let someBytes = bytes(of: some)

    if
        let tagOffset = noneBytes.indices.first(where: {
            noneBytes[$0] != someBytes[$0] && zeroBytes[$0] == someBytes[$0]
        }),
        loadValue(from: some, offset: 0, as: UInt16.self) == 0xCAFE,
        loadValue(from: zero, offset: 0, as: UInt16.self) == 0
    {
        return .directTag(
            offset: tagOffset,
            width: MemoryLayout<UInt8>.size,
            noneValue: UInt(noneBytes[tagOffset]),
            someValue: UInt(someBytes[tagOffset]),
            someOffset: 0
        )
    }

    return .thunk(
        isSome: "Swift.Optional<UInt16>.isSome",
        some: "Swift.Optional<UInt16>.some",
        writeNone: "Swift.Optional<UInt16>.init.none",
        writeSomeBytes: "Swift.Optional<UInt16>.init.some.bytes"
    )
}

private func bytes<T>(of value: T) -> [UInt8] {
    var value = value
    return withUnsafeBytes(of: &value) { Array($0) }
}

private func leafDescriptor(
    count: BinetteLocalDescriptor,
    flag: BinetteLocalDescriptor
) -> BinetteLocalDescriptor {
    BinetteLocalDescriptor(
        schemaName: "ProbeLeaf",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: ProbeLeaf.self),
        kind: .storedStruct(fields: [
            BinetteLocalField(
                name: "count",
                access: .direct(offset: MemoryLayout<ProbeLeaf>.offset(of: \ProbeLeaf.count)!),
                descriptor: count
            ),
            BinetteLocalField(
                name: "flag",
                access: .direct(offset: MemoryLayout<ProbeLeaf>.offset(of: \ProbeLeaf.flag)!),
                descriptor: flag
            ),
        ])
    )
}

// r[impl binette.local-access.backends]
private func nestedDescriptor(
    title: BinetteLocalDescriptor,
    leaf: BinetteLocalDescriptor,
    values: BinetteLocalDescriptor
) -> BinetteLocalDescriptor {
    BinetteLocalDescriptor(
        schemaName: "ProbeNested",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: ProbeNested.self),
        kind: .storedStruct(fields: [
            BinetteLocalField(
                name: "title",
                access: .direct(offset: MemoryLayout<ProbeNested>.offset(of: \ProbeNested.title)!),
                descriptor: title
            ),
            BinetteLocalField(
                name: "leaf",
                access: .direct(offset: MemoryLayout<ProbeNested>.offset(of: \ProbeNested.leaf)!),
                descriptor: leaf
            ),
            BinetteLocalField(
                name: "values",
                access: .direct(offset: MemoryLayout<ProbeNested>.offset(of: \ProbeNested.values)!),
                descriptor: values
            ),
        ])
    )
}

// r[impl binette.local-access.backends]
private func enumDescriptor() -> BinetteLocalDescriptor {
    let string = stringDescriptor()
    let leaf = leafDescriptor(
        count: scalarDescriptor("i32", Int32.self),
        flag: scalarDescriptor("bool", Bool.self)
    )

    return BinetteLocalDescriptor(
        schemaName: "ProbeEnum",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: ProbeEnum.self),
        kind: .enumPayloads(
            tag: .thunk("ProbeEnum.discriminant"),
            variants: [
                BinetteLocalVariant(
                    name: "empty",
                    index: 0,
                    access: .thunk("ProbeEnum.project.empty"),
                    construct: "ProbeEnum.init.empty",
                    payload: nil
                ),
                BinetteLocalVariant(
                    name: "titled",
                    index: 1,
                    access: .thunk("ProbeEnum.project.titled"),
                    construct: "ProbeEnum.init.titled.utf8",
                    payload: string
                ),
                BinetteLocalVariant(
                    name: "nested",
                    index: 2,
                    access: .thunk("ProbeEnum.project.nested"),
                    construct: "ProbeEnum.init.nested",
                    payload: leaf
                ),
            ]
        )
    )
}
