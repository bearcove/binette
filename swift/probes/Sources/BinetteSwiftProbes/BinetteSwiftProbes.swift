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

public indirect enum BinetteLocalKind: Equatable {
    case scalar
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
    case directTag(offset: Int, noneValue: UInt)
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
    let int32 = scalarDescriptor("i32", Int32.self)
    let int64 = scalarDescriptor("i64", Int64.self)
    let string = stringDescriptor(element: uint8)
    let array = arrayDescriptor(element: int64)
    let optionalString = optionalDescriptor("option<string>", String?.self, some: string)
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
        leaf,
        nested,
        enumPayloads,
    ]
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
    ].allSatisfy(names.contains)
}

private func scalarDescriptor<T>(_ name: String, _: T.Type) -> BinetteLocalDescriptor {
    return BinetteLocalDescriptor(
        schemaName: name,
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: T.self),
        kind: .scalar
    )
}

private func stringDescriptor(element: BinetteLocalDescriptor) -> BinetteLocalDescriptor {
    BinetteLocalDescriptor(
        schemaName: "string",
        backend: .swiftProbe,
        layout: BinetteLocalLayout(of: String.self),
        kind: .sequence(
            element: element,
            storage: .thunk(
                count: "Swift.String.utf8.count",
                element: "Swift.String.utf8.element",
                write: "Swift.String.init.utf8"
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
    let uint8 = scalarDescriptor("u8", UInt8.self)
    let string = stringDescriptor(element: uint8)
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
