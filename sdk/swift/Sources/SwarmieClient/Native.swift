import Foundation
import CSwarmie

internal func milliseconds(_ seconds: TimeInterval) throws -> JSONValue {
    guard seconds.isFinite, seconds > 0, seconds <= 365 * 24 * 60 * 60 else { throw ProtocolError("Deadline must be positive and at most one year") }
    return .number(ceil(seconds * 1000))
}

private enum NativeLibrary {
    static let available: Bool = {
        #if SWARMIE_STATIC
        return swarmie_binding_load("") == 1
        #else
        #if os(macOS)
        let name = "libswarmie_client_ffi.dylib"
        #else
        let name = "libswarmie_client_ffi.so"
        #endif
        guard let path = Bundle.module.url(forResource: name, withExtension: nil, subdirectory: "native")?.path else { return false }
        return path.withCString { swarmie_binding_load($0) == 1 }
        #endif
    }()
}

private struct NativeError: Decodable {
    let kind: String
    let message: String
    let code: Int64?
    let dataJSON: String?
    enum CodingKeys: String, CodingKey { case kind, message, code; case dataJSON = "data_json" }
}
private struct NativeReply: Decodable {
    let ok: JSONValue?
    let error: NativeError?
    enum CodingKeys: String, CodingKey { case ok, error }
    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        // Decode null as an explicit success value instead of a missing key.
        ok = container.contains(.ok) ? try container.decode(JSONValue.self, forKey: .ok) : nil
        error = try container.decodeIfPresent(NativeError.self, forKey: .error)
    }
}

internal func nativeControl(_ command: JSONValue) throws -> JSONValue {
    guard NativeLibrary.available else { throw ProtocolError("Native client library is missing or incompatible") }
    let data = try JSONEncoder().encode(command)
    guard let text = String(data: data, encoding: .utf8), let output = text.withCString({ swarmie_binding_execute($0) }) else { throw ProtocolError("Native client allocation failed") }
    defer { swarmie_binding_free(output) }
    let reply = try JSONDecoder().decode(NativeReply.self, from: Data(String(cString: output).utf8))
    if let value = reply.ok { return value }
    guard let error = reply.error else { throw ProtocolError("Invalid native error") }
    if error.kind == "rpc" {
        guard let rawCode = error.code, let code = Int(exactly: rawCode) else { throw ProtocolError("Invalid native RPC error code") }
        let data = try error.dataJSON.map { try JSONDecoder().decode(JSONValue.self, from: Data($0.utf8)) }
        throw RPCError(code: code, message: error.message, data: data)
    }
    if error.kind == "cancelled" { throw CancellationError() }
    throw ProtocolError(error.message)
}

internal final class NativeHandle: Sendable {
    let value: JSONValue
    init(command: JSONValue) throws { value = try nativeControl(command) }
    func dispose() throws { _ = try nativeControl(.object(["action": .string("release"), "handle": value])) }
    deinit { try? dispose() }
}

internal func nativeOperation(_ command: JSONValue) async throws -> Data {
    let operation = try NativeHandle(command: .object(["action": .string("prepare")]))
    defer { try? operation.dispose() }
    guard case .object(var fields) = command else { throw ProtocolError("Invalid native operation") }
    fields["operation"] = operation.value
    let request = JSONValue.object(fields)
    return try await withTaskCancellationHandler {
        if Task.isCancelled { _ = try nativeControl(.object(["action": .string("cancel"), "operation": operation.value])) }
        return try await Task.detached {
            let result = try nativeControl(request)
            guard let text = result.stringValue else { throw ProtocolError("Invalid native JSON result") }
            return Data(text.utf8)
        }.value
    } onCancel: {
        _ = try? nativeControl(.object(["action": .string("cancel"), "operation": operation.value]))
    }
}
