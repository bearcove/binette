import BinetteSwiftProbes
import Foundation

let encoder = JSONEncoder()
encoder.keyEncodingStrategy = .convertToSnakeCase
encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

let data = try encoder.encode(exportProbeDescriptors())
FileHandle.standardOutput.write(data)
FileHandle.standardOutput.write(Data([0x0A]))
