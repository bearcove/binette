public enum BinetteProbeBackend: Equatable {
    case swiftProbe
}

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

public struct BinetteLocalDescriptor: Equatable {
    public var schemaName: String
    public var backend: BinetteProbeBackend
    public var layout: BinetteLocalLayout
    public var kind: BinetteLocalKind
}

public final class BinetteDescriptorExport: Codable, Equatable {
    public var schemaName: String
    public var backend: String
    public var layout: BinetteLayoutExport
    public var kind: BinetteKindExport

    public init(
        schemaName: String,
        backend: String,
        layout: BinetteLayoutExport,
        kind: BinetteKindExport
    ) {
        self.schemaName = schemaName
        self.backend = backend
        self.layout = layout
        self.kind = kind
    }

    public static func == (lhs: BinetteDescriptorExport, rhs: BinetteDescriptorExport) -> Bool {
        lhs.schemaName == rhs.schemaName
            && lhs.backend == rhs.backend
            && lhs.layout == rhs.layout
            && lhs.kind == rhs.kind
    }
}

public struct BinetteLayoutExport: Codable, Equatable {
    public var size: Int
    public var alignment: Int
    public var stride: Int
}

public struct BinetteKindExport: Codable, Equatable {
    public var tag: String
    public var fields: [BinetteFieldExport]?
    public var variants: [BinetteVariantExport]?
    public var element: BinetteDescriptorExport?
    public var some: BinetteDescriptorExport?
    public var storage: BinetteStorageExport?
    public var reason: String?

    public init(
        tag: String,
        fields: [BinetteFieldExport]? = nil,
        variants: [BinetteVariantExport]? = nil,
        element: BinetteDescriptorExport? = nil,
        some: BinetteDescriptorExport? = nil,
        storage: BinetteStorageExport? = nil,
        reason: String? = nil
    ) {
        self.tag = tag
        self.fields = fields
        self.variants = variants
        self.element = element
        self.some = some
        self.storage = storage
        self.reason = reason
    }
}

public struct BinetteFieldExport: Codable, Equatable {
    public var name: String
    public var access: BinetteAccessExport
    public var descriptor: BinetteDescriptorExport
}

public struct BinetteVariantExport: Codable, Equatable {
    public var name: String
    public var index: UInt32
    public var access: BinetteAccessExport
    public var construct: String?
    public var payload: BinetteDescriptorExport?
}

public struct BinetteAccessExport: Codable, Equatable {
    public var tag: String
    public var offset: Int?
    public var thunk: String?
}

public struct BinetteStorageExport: Codable, Equatable {
    public var tag: String
    public var pointerOffset: Int?
    public var countOffset: Int?
    public var elementStride: Int?
    public var count: String?
    public var element: String?
    public var write: String?
    public var optionTagOffset: Int?
    public var optionTagWidth: Int?
    public var noneValue: UInt?
    public var someValue: UInt?
    public var someOffset: Int?
    public var isSome: String?
    public var some: String?
    public var writeNone: String?
    public var writeSomeBytes: String?

    public init(
        tag: String,
        pointerOffset: Int? = nil,
        countOffset: Int? = nil,
        elementStride: Int? = nil,
        count: String? = nil,
        element: String? = nil,
        write: String? = nil,
        optionTagOffset: Int? = nil,
        optionTagWidth: Int? = nil,
        noneValue: UInt? = nil,
        someValue: UInt? = nil,
        someOffset: Int? = nil,
        isSome: String? = nil,
        some: String? = nil,
        writeNone: String? = nil,
        writeSomeBytes: String? = nil
    ) {
        self.tag = tag
        self.pointerOffset = pointerOffset
        self.countOffset = countOffset
        self.elementStride = elementStride
        self.count = count
        self.element = element
        self.write = write
        self.optionTagOffset = optionTagOffset
        self.optionTagWidth = optionTagWidth
        self.noneValue = noneValue
        self.someValue = someValue
        self.someOffset = someOffset
        self.isSome = isSome
        self.some = some
        self.writeNone = writeNone
        self.writeSomeBytes = writeSomeBytes
    }
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

public func exportProbeDescriptors() -> [BinetteDescriptorExport] {
    makeProbeDescriptors().map(\.export)
}

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

private extension BinetteLocalDescriptor {
    var export: BinetteDescriptorExport {
        BinetteDescriptorExport(
            schemaName: schemaName,
            backend: backend.export,
            layout: layout.export,
            kind: kind.export
        )
    }
}

private extension BinetteProbeBackend {
    var export: String {
        switch self {
        case .swiftProbe:
            return "swift-probe"
        }
    }
}

private extension BinetteLocalLayout {
    var export: BinetteLayoutExport {
        BinetteLayoutExport(size: size, alignment: alignment, stride: stride)
    }
}

private extension BinetteScalarAccess {
    var export: BinetteKindExport {
        switch self {
        case .plain:
            BinetteKindExport(tag: "scalar")
        case let .string(storage):
            BinetteKindExport(tag: "string", storage: storage.export)
        case let .bytes(storage):
            BinetteKindExport(tag: "bytes", storage: storage.export)
        }
    }
}

private extension BinetteLocalKind {
    var export: BinetteKindExport {
        switch self {
        case let .scalar(access):
            access.export
        case let .storedStruct(fields):
            BinetteKindExport(
                tag: "struct",
                fields: fields.map(\.export)
            )
        case let .enumPayloads(tag, variants):
            BinetteKindExport(
                tag: "enum",
                fields: [
                    BinetteFieldExport(
                        name: "$tag",
                        access: tag.export,
                        descriptor: scalarDescriptor("u32", UInt32.self).export
                    ),
                ],
                variants: variants.map(\.export)
            )
        case let .optional(some, storage):
            BinetteKindExport(
                tag: "option",
                some: some.export,
                storage: storage.export
            )
        case let .sequence(element, storage):
            BinetteKindExport(
                tag: "sequence",
                element: element.export,
                storage: storage.export
            )
        case let .opaque(reason):
            BinetteKindExport(tag: "opaque", reason: reason)
        }
    }
}

private extension BinetteLocalField {
    var export: BinetteFieldExport {
        BinetteFieldExport(
            name: name,
            access: access.export,
            descriptor: descriptor.export
        )
    }
}

private extension BinetteLocalVariant {
    var export: BinetteVariantExport {
        BinetteVariantExport(
            name: name,
            index: index,
            access: access.export,
            construct: construct,
            payload: payload?.export
        )
    }
}

private extension BinetteLocalAccess {
    var export: BinetteAccessExport {
        switch self {
        case let .direct(offset):
            BinetteAccessExport(tag: "direct", offset: offset, thunk: nil)
        case let .thunk(name):
            BinetteAccessExport(tag: "thunk", offset: nil, thunk: name)
        }
    }
}

private extension BinetteSequenceStorage {
    var export: BinetteStorageExport {
        switch self {
        case let .directContiguous(pointerOffset, countOffset, elementStride):
            BinetteStorageExport(
                tag: "direct-contiguous",
                pointerOffset: pointerOffset,
                countOffset: countOffset,
                elementStride: elementStride
            )
        case let .thunk(count, element, write):
            BinetteStorageExport(
                tag: "thunk",
                count: count,
                element: element,
                write: write
            )
        }
    }
}

private extension BinetteOptionalStorage {
    var export: BinetteStorageExport {
        switch self {
        case let .directTag(offset, width, noneValue, someValue, someOffset):
            BinetteStorageExport(
                tag: "direct-tag",
                optionTagOffset: offset,
                optionTagWidth: width,
                noneValue: noneValue,
                someValue: someValue,
                someOffset: someOffset
            )
        case let .nicheTag(offset, width, noneValue, someOffset):
            BinetteStorageExport(
                tag: "niche-tag",
                optionTagOffset: offset,
                optionTagWidth: width,
                noneValue: noneValue,
                someOffset: someOffset
            )
        case let .thunk(isSome, some, writeNone, writeSomeBytes):
            BinetteStorageExport(
                tag: "thunk",
                isSome: isSome,
                some: some,
                writeNone: writeNone,
                writeSomeBytes: writeSomeBytes
            )
        }
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
