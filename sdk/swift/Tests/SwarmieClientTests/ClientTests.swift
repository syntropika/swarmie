import XCTest
import Foundation
@testable import SwarmieClient

final class ClientTests: XCTestCase, @unchecked Sendable {
    func testJSONValueAndInputSecrets() throws {
        let content = JSONValue.imageContent(mimeType: "image/png", base64: "YQ==")
        XCTAssertEqual(try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(content)), content)
        let grant = InputHostToolGrant(argumentConstraints: ["path": [.string("notes.txt")]], extensionId: "workspace", toolId: "read_file")
        let execution = InputSessionExecutionOptions(env: ["EXAMPLE_TOKEN": "input-only"], ephemeral: false, hostTools: [grant], mcpServers: [], tools: "enabled")
        let input = InputSpawnOptions(cwd: "/tmp", execution: execution, harness: "mock")
        let json = try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(input))
        XCTAssertEqual(json["execution"]?["env"]?["EXAMPLE_TOKEN"], .string("input-only"))
        guard case .array(let grants)? = json["execution"]?["hostTools"] else { return XCTFail("Missing host tool grants") }
        XCTAssertEqual(grants.first?["toolId"], .string("read_file"))
        let output = SessionExecutionOptions(ephemeral: false, hostTools: [], mcpServers: [], outputSchema: .null, tools: "enabled")
        XCTAssertNil(try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(output))["env"])
        XCTAssertTrue(protocolMethods.contains("session/create"))
    }
    func testOwnedStdioDeadlineAndCancellation() async throws {
        let client = try Client(command: "/usr/bin/env", arguments: ["python3", "-c", "import time; time.sleep(60)"], options: ClientOptions(timeout: 0.02))
        defer { try? client.dispose() }
        do { _ = try await client.initialize(); XCTFail("Deadline ignored") }
        catch let error as ProtocolError { XCTAssertTrue(error.message.contains("timed out")) }
        do { _ = try await client.initialize(); XCTFail("Closed process accepted another call") }
        catch let error as ProtocolError { XCTAssertTrue(error.message.contains("closed")) }
        let cancellable = try Client(command: "/usr/bin/env", arguments: ["python3", "-c", "import time; time.sleep(60)"])
        defer { try? cancellable.dispose() }
        let request = Task { try await cancellable.initialize() }
        try await Task.sleep(for: .milliseconds(20))
        request.cancel()
        do { _ = try await request.value; XCTFail("Cancellation ignored") }
        catch is CancellationError { }
        do { _ = try await cancellable.initialize(); XCTFail("Cancelled process accepted another call") }
        catch let error as ProtocolError { XCTAssertTrue(error.message.contains("closed")) }
    }
    func testLiveHTTPConformance() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let endpoint = env["SWARMIE_TEST_URL"] else { throw XCTSkip("Live daemon URL required") }
        let client = try Client(url: XCTUnwrap(URL(string: endpoint)), options: ClientOptions(token: env["SWARMIE_TEST_TOKEN"]))
        defer { try? client.dispose() }
        let initialized = try await client.initialize(); XCTAssertNotEqual(initialized, .null)
        let schema = try await client.schema(); XCTAssertNotNil(schema["x-methods"]?["session/create"])
        let harnesses = try await client.discover(); XCTAssertTrue(harnesses.contains { $0.harness == "mock" && $0.available })
        do { let _: JSONValue = try await client.call("not/a/method", params: [String: String]()); XCTFail("Unknown method accepted") }
        catch let error as RPCError { XCTAssertEqual(error.code, -32601) }
        let session = try await client.spawn(InputSpawnOptions(cwd: env["SWARMIE_TEST_CWD"] ?? "/tmp", harness: "mock"))
        do {
            XCTAssertEqual(session.capabilities.queue.source, "runtime")
            XCTAssertEqual(session.capabilities.configure.availability, "available")
            let models: [ModelDescriptor] = try await client.call("agent/models", params: ["sessionId": session.id])
            XCTAssertEqual(models.first?.metadata?["nativeField"], .string("retained"))
            let settings: [SettingDescriptor] = try await client.call("agent/configuration", params: ["sessionId": session.id])
            XCTAssertEqual(settings.first?.application, "next_turn")
            let configured: ConfigurationReceipt = try await client.call("agent/configure", params: JSONValue.object(["sessionId": .string(session.id), "config": .object(["fast": .bool(true)])]))
            XCTAssertEqual(configured.values["fast"], .bool(true))
            XCTAssertEqual(configured.configuration.first?.value, .bool(true))
            XCTAssertEqual(settings.last?.id, "executionProfile")
            XCTAssertEqual(configured.executionPolicy.status, "unknown")
            XCTAssertFalse(configured.executionPolicy.dispatchBlocked)
            XCTAssertEqual(configured.executionPolicy.choices.count, 3)
            let receipt = try await client.prompt(sessionID: session.id, content: [.textContent("Reply")], idempotencyKey: "request-1"); XCTAssertEqual(receipt.delivery, "accepted")
            let events = try await client.waitForTurn(sessionID: session.id, options: EventOptions(afterSequence: session.sequence, timeout: 3, pollInterval: 0.005))
            XCTAssertEqual(events.filter { $0.type == "text.delta" }.compactMap { $0.data?["text"]?.stringValue }.joined(), "client-ready")
            XCTAssertEqual(events.last?.type, "turn.ended")
            XCTAssertEqual(events.last?.data?["outcome"]?.stringValue, "completed")
            for pair in zip(events, events.dropFirst()) { XCTAssertLessThan(pair.0.sequence, pair.1.sequence) }
            let recovered = try await client.submission(sessionID: session.id, idempotencyKey: "request-1")
            let reconciled = try await client.reconcileSubmission(sessionID: session.id, idempotencyKey: "request-1")
            XCTAssertEqual(reconciled?.id, recovered?.id)
            XCTAssertEqual(recovered?.id, receipt.id)
            let duplicate = try await client.prompt(sessionID: session.id, text: "Reply", idempotencyKey: "request-1")
            XCTAssertEqual(duplicate.id, receipt.id)
            do { _ = try await client.prompt(sessionID: session.id, text: "Changed", idempotencyKey: "request-1"); XCTFail("Changed payload accepted") }
            catch let error as RPCError { XCTAssertEqual(error.code, -32004) }
            let replay: [RuntimeEvent] = try await client.call("session/transcript", params: ["sessionId": session.id]); XCTAssertTrue(replay.contains { $0.type == "text.delta" })
            do { _ = try await client.waitForTurn(sessionID: session.id, options: EventOptions(afterSequence: events.last!.sequence, timeout: 0.03, pollInterval: 0.005)); XCTFail("Polling deadline ignored") }
            catch let error as ProtocolError { XCTAssertTrue(error.message.contains("deadline")) }
            catch let error as URLError { XCTAssertEqual(error.code, .timedOut) }
            let waiting = Task { try await client.waitForTurn(sessionID: session.id, options: EventOptions(afterSequence: events.last!.sequence, timeout: 60, pollInterval: 0.01)) }
            try await Task.sleep(for: .milliseconds(20))
            waiting.cancel()
            do { _ = try await waiting.value; XCTFail("Event cancellation ignored") } catch is CancellationError { }
            try await client.close(sessionID: session.id)
        } catch { try? await client.close(sessionID: session.id); throw error }
        let denied = try Client(url: XCTUnwrap(URL(string: endpoint)), options: ClientOptions(token: "incorrect"))
        do { _ = try await denied.initialize(); XCTFail("Invalid auth accepted") } catch is ProtocolError { }
        let bounded = try Client(url: XCTUnwrap(URL(string: endpoint)), options: ClientOptions(token: env["SWARMIE_TEST_TOKEN"], maxResponseBytes: 1024))
        do { _ = try await bounded.schema(); XCTFail("Byte limit ignored") } catch is ProtocolError { }
    }
}
