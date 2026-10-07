import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

public enum JSONValue: Codable, Sendable, Equatable {
    case null, bool(Bool), number(Double), string(String), array([JSONValue]), object([String: JSONValue])
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
public struct PromptReceipt: Codable, Sendable { public let accepted: Bool }

// An actor gives concurrent requests unique IDs without sharing mutable encoders.
public actor Client {
    private let endpoint: URL
    private let options: ClientOptions
    private let session: URLSession
    private let prefix = UUID().uuidString
    private var sequence: UInt64 = 0
    public init(url: URL, options: ClientOptions = ClientOptions(), session: URLSession = .shared) throws {
        guard ["http", "https"].contains(url.scheme), url.host != nil else { throw ProtocolError("Use an HTTP or HTTPS endpoint") }
        guard options.timeout.isFinite, options.timeout > 0, options.maxResponseBytes > 0 else { throw ProtocolError("Limits must be positive") }
        self.endpoint = url.path.isEmpty || url.path == "/" ? url.appendingPathComponent("rpc") : url
        self.options = options; self.session = session
    }
    public func call<P: Encodable & Sendable, R: Decodable & Sendable>(_ method: String, params: P, as type: R.Type = R.self, timeout: TimeInterval? = nil) async throws -> R {
        sequence += 1
        let id = "\(prefix)-\(sequence)"
        try Task.checkCancellation()
        let encodedParams = try JSONEncoder().encode(params)
        let value = try JSONDecoder().decode(JSONValue.self, from: encodedParams)
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"; request.timeoutInterval = min(timeout ?? options.timeout, options.timeout)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let token = options.token { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        request.httpBody = try JSONEncoder().encode(JSONValue.object(["jsonrpc": .string("2.0"), "id": .string(id), "method": .string(method), "params": value]))
        // AsyncBytes bounds accumulated memory while preserving cancellation.
        let (bytes, response) = try await session.bytes(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { throw ProtocolError("Runtime HTTP status \((response as? HTTPURLResponse)?.statusCode ?? 0)") }
        var data = Data()
        for try await byte in bytes {
            if data.count >= options.maxResponseBytes { throw ProtocolError("Runtime response exceeded the configured byte limit") }
            data.append(byte)
        }
        let envelope: JSONValue
        do { envelope = try JSONDecoder().decode(JSONValue.self, from: data) } catch { throw ProtocolError("Runtime response is not JSON") }
        guard envelope["jsonrpc"] == .string("2.0"), envelope["id"] == .string(id) else { throw ProtocolError("JSON-RPC response identity mismatch") }
        if let error = envelope["error"] { throw try JSONDecoder().decode(RPCError.self, from: JSONEncoder().encode(error)) }
        guard let result = envelope["result"] else { throw ProtocolError("JSON-RPC response has no result") }
        return try JSONDecoder().decode(R.self, from: JSONEncoder().encode(result))
    }
    public func initialize() async throws -> JSONValue { try await call("runtime/initialize", params: [String: String]()) }
    public func schema() async throws -> JSONValue { try await call("runtime/schema", params: [String: String]()) }
    public func discover() async throws -> [Detection] { try await call("runtime/discover", params: [String: String]()) }
    public func spawn(_ options: InputSpawnOptions) async throws -> SessionSnapshot { try await call("session/create", params: options) }
    public func prompt(sessionID: String, text: String) async throws -> PromptReceipt { try await call("session/prompt", params: ["sessionId": sessionID, "text": text]) }
    public func prompt(sessionID: String, content: [InputPromptContent]) async throws -> PromptReceipt { try await call("session/prompt", params: JSONValue.object(["sessionId": .string(sessionID), "content": .array(content)])) }
    public func close(sessionID: String) async throws { let _: JSONValue = try await call("session/close", params: ["sessionId": sessionID]) }
    public nonisolated func events(sessionID: String, options: EventOptions = EventOptions()) throws -> EventStream {
        try EventStream(client: self, sessionID: sessionID, options: options)
    }
    public func waitForTurn(sessionID: String, options: EventOptions = EventOptions()) async throws -> [RuntimeEvent] {
        var collected: [RuntimeEvent] = []
        for try await event in try events(sessionID: sessionID, options: options) {
            collected.append(event)
            if event.type == "turn.ended" { return collected }
            if event.type == "session.ended" { throw ProtocolError("Session ended before the turn completed") }
        }
        return collected
    }
}

// Demand-driven iteration holds only one page and never drops ordered events.
public struct EventStream: AsyncSequence, Sendable {
    public typealias Element = RuntimeEvent
    private let client: Client
    private let sessionID: String
    private let options: EventOptions
    fileprivate init(client: Client, sessionID: String, options: EventOptions) throws {
        guard options.timeout.isFinite, options.timeout > 0, options.pollInterval.isFinite, options.pollInterval > 0 else { throw ProtocolError("Limits must be positive") }
        self.client = client; self.sessionID = sessionID; self.options = options
    }
    public func makeAsyncIterator() -> AsyncIterator { AsyncIterator(client: client, sessionID: sessionID, options: options) }
    public struct AsyncIterator: AsyncIteratorProtocol {
        private let client: Client
        private let sessionID: String
        private let options: EventOptions
        private let deadline: ContinuousClock.Instant
        private var sequence: UInt64
        private var page: [RuntimeEvent] = []
        private var index = 0
        private var shouldWait = false
        fileprivate init(client: Client, sessionID: String, options: EventOptions) {
            self.client = client; self.sessionID = sessionID; self.options = options
            self.sequence = options.afterSequence; self.deadline = ContinuousClock.now.advanced(by: .seconds(options.timeout))
        }
        public mutating func next() async throws -> RuntimeEvent? {
            while true {
                try Task.checkCancellation()
                guard ContinuousClock.now < deadline else { throw ProtocolError("Event polling deadline exceeded") }
                if index < page.count {
                    let event = page[index]; index += 1
                    guard event.sessionId == sessionID, event.sequence > sequence else { throw ProtocolError("Runtime event ordering or session identity mismatch") }
                    sequence = event.sequence; return event
                }
                let remaining = ContinuousClock.now.duration(to: deadline)
                let seconds = Double(remaining.components.seconds) + Double(remaining.components.attoseconds) / 1e18
                if shouldWait { try await Task.sleep(for: .seconds(Swift.min(options.pollInterval, seconds))) }
                guard ContinuousClock.now < deadline else { throw ProtocolError("Event polling deadline exceeded") }
                let currentRemaining = ContinuousClock.now.duration(to: deadline)
                let timeout = Double(currentRemaining.components.seconds) + Double(currentRemaining.components.attoseconds) / 1e18
                page = try await client.call("session/transcript", params: JSONValue.object(["sessionId": .string(sessionID), "afterSequence": .number(Double(sequence)), "limit": .number(256)]), timeout: timeout)
                index = 0; shouldWait = page.count < 256
            }
        }
    }
}
