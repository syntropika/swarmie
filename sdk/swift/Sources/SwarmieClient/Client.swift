import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

public enum JSONValue: Codable, Sendable, Equatable {
    case null, bool(Bool), number(Double), unsigned(UInt64), string(String), array([JSONValue]), object([String: JSONValue])
    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() { self = .null }
        else if let value = try? container.decode(Bool.self) { self = .bool(value) }
        else if let value = try? container.decode(String.self) { self = .string(value) }
        else if let value = try? container.decode(Double.self) { self = .number(value) }
        else if let value = try? container.decode([JSONValue].self) { self = .array(value) }
        else { self = .object(try container.decode([String: JSONValue].self)) }
    }
    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .unsigned(let value): try container.encode(value)
        case .string(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        }
    }
    public subscript(key: String) -> JSONValue? { if case .object(let value) = self { return value[key] }; return nil }
    public var stringValue: String? { if case .string(let value) = self { return value }; return nil }
    public static func textContent(_ text: String) -> JSONValue { .object(["type": .string("text"), "data": .string(text)]) }
    public static func imageContent(mimeType: String, base64: String) -> JSONValue { .object(["type": .string("image"), "data": .object(["mime_type": .string(mimeType), "data": .string(base64)])]) }
}

public struct RPCError: Error, Codable, Sendable {
    public let code: Int
    public let message: String
    public let data: JSONValue?
}
public struct ProtocolError: Error, Sendable { public let message: String; public init(_ message: String) { self.message = message } }
public struct ClientOptions: Sendable {
    public var token: String?
    public var timeout: TimeInterval
    public var maxResponseBytes: Int
    public init(token: String? = nil, timeout: TimeInterval = 30, maxResponseBytes: Int = 16 * 1024 * 1024) { self.token = token; self.timeout = timeout; self.maxResponseBytes = maxResponseBytes }
}
public struct EventOptions: Sendable {
    public var afterSequence: UInt64
    public var timeout: TimeInterval
    public var pollInterval: TimeInterval
    public init(afterSequence: UInt64 = 0, timeout: TimeInterval = 600, pollInterval: TimeInterval = 0.2) { self.afterSequence = afterSequence; self.timeout = timeout; self.pollInterval = pollInterval }
}

public actor Client {
    private nonisolated let native: NativeHandle
    public init(url: URL, options: ClientOptions = ClientOptions()) throws {
        native = try NativeHandle(command: .object(["action": .string("create_http"), "config": .object(["url": .string(url.absoluteString), "token": options.token.map(JSONValue.string) ?? .null, "timeout_ms": try milliseconds(options.timeout), "max_response_bytes": .number(Double(options.maxResponseBytes))])]))
    }
    public init(command: String, arguments: [String] = [], options: ClientOptions = ClientOptions()) throws {
        native = try NativeHandle(command: .object(["action": .string("create_stdio"), "config": .object(["command": .string(command), "args": .array(arguments.map(JSONValue.string)), "timeout_ms": try milliseconds(options.timeout), "max_response_bytes": .number(Double(options.maxResponseBytes))])]))
    }
    public nonisolated func dispose() throws { try native.dispose() }
    public func call<P: Encodable & Sendable, R: Decodable & Sendable>(_ method: String, params: P, as type: R.Type = R.self, timeout: TimeInterval? = nil) async throws -> R {
        let data = try JSONEncoder().encode(params)
        guard let encoded = String(data: data, encoding: .utf8) else { throw ProtocolError("Request is not UTF-8") }
        let result = try await nativeOperation(.object(["action": .string("invoke"), "client": native.value, "request": .object(["method": .string(method), "params_json": .string(encoded), "timeout_ms": try timeout.map(milliseconds) ?? .null])]))
        return try JSONDecoder().decode(R.self, from: result)
    }
    public func initialize() async throws -> JSONValue { try await call("runtime/initialize", params: [String: String]()) }
    public func schema() async throws -> JSONValue { try await call("runtime/schema", params: [String: String]()) }
    public func discover() async throws -> [Detection] { try await call("runtime/discover", params: [String: String]()) }
    public func spawn(_ options: InputSpawnOptions) async throws -> SessionSnapshot { try await call("session/create", params: options) }
    public func prompt(sessionID: String, text: String, idempotencyKey: String) async throws -> SubmissionReceipt { try await call("session/prompt", params: ["sessionId": sessionID, "text": text, "idempotencyKey": idempotencyKey]) }
    public func prompt(sessionID: String, content: [InputPromptContent], idempotencyKey: String) async throws -> SubmissionReceipt { try await call("session/prompt", params: JSONValue.object(["sessionId": .string(sessionID), "content": .array(content), "idempotencyKey": .string(idempotencyKey)])) }
    public func submission(sessionID: String, idempotencyKey: String) async throws -> SubmissionReceipt? { try await call("session/submission", params: ["sessionId": sessionID, "idempotencyKey": idempotencyKey]) }
    public func reconcileSubmission(sessionID: String, idempotencyKey: String) async throws -> SubmissionReceipt? { try await call("session/submission/reconcile", params: ["sessionId": sessionID, "idempotencyKey": idempotencyKey]) }
    public func close(sessionID: String) async throws { let _: JSONValue = try await call("session/close", params: ["sessionId": sessionID]) }
    public nonisolated func events(sessionID: String, options: EventOptions = EventOptions()) throws -> EventStream {
        try EventStream(client: native, sessionID: sessionID, options: options)
    }
    public func waitForTurn(sessionID: String, options: EventOptions = EventOptions()) async throws -> [RuntimeEvent] {
        let result = try await nativeOperation(.object(["action": .string("wait_turn"), "client": native.value, "session_id": .string(sessionID), "options": .object(["after_sequence": .unsigned(options.afterSequence), "timeout_ms": try milliseconds(options.timeout), "poll_interval_ms": try milliseconds(options.pollInterval)])]))
        return try JSONDecoder().decode([RuntimeEvent].self, from: result)
    }
}

// A reference owns the cursor so copied iterators release it exactly once.
public struct EventStream: AsyncSequence, Sendable {
    public typealias Element = RuntimeEvent
    private let client: NativeHandle
    private let sessionID: String
    private let options: EventOptions
    fileprivate init(client: NativeHandle, sessionID: String, options: EventOptions) throws {
        _ = try milliseconds(options.timeout)
        _ = try milliseconds(options.pollInterval)
        self.client = client; self.sessionID = sessionID; self.options = options
    }
    public func makeAsyncIterator() -> AsyncIterator { AsyncIterator(client: client, sessionID: sessionID, options: options) }
    public struct AsyncIterator: AsyncIteratorProtocol {
        private let client: NativeHandle
        private let sessionID: String
        private let options: EventOptions
        private var stream: NativeHandle?
        fileprivate init(client: NativeHandle, sessionID: String, options: EventOptions) {
            self.client = client; self.sessionID = sessionID; self.options = options
        }
        public mutating func next() async throws -> RuntimeEvent? {
            if stream == nil {
                stream = try NativeHandle(command: .object(["action": .string("open_events"), "client": client.value, "session_id": .string(sessionID), "options": .object(["after_sequence": .unsigned(options.afterSequence), "timeout_ms": try milliseconds(options.timeout), "poll_interval_ms": try milliseconds(options.pollInterval)])]))
            }
            guard let stream else { throw ProtocolError("Event stream is closed") }
            let result = try await nativeOperation(.object(["action": .string("next_event"), "stream": stream.value]))
            return try JSONDecoder().decode(RuntimeEvent.self, from: result)
        }
    }
}
