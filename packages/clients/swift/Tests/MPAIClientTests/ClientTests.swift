import XCTest
import Foundation
@testable import MPAIClient

final class ClientTests: XCTestCase, @unchecked Sendable {
    func testJSONValueAndInputSecrets() throws {
        let content = JSONValue.imageContent(mimeType: "image/png", base64: "YQ==")
        XCTAssertEqual(try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(content)), content)
        let execution = InputSessionExecutionOptions(env: ["EXAMPLE_TOKEN": "input-only"], ephemeral: false, mcpServers: [], tools: "enabled")
        let input = InputSpawnOptions(cwd: "/tmp", execution: execution, harness: "mock")
        let json = try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(input))
        XCTAssertEqual(json["execution"]?["env"]?["EXAMPLE_TOKEN"], .string("input-only"))
        let output = SessionExecutionOptions(ephemeral: false, mcpServers: [], outputSchema: .null, tools: "enabled")
        XCTAssertNil(try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(output))["env"])
        XCTAssertTrue(protocolMethods.contains("session/create"))
    }
    func testLiveHTTPConformance() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let endpoint = env["MPAI_TEST_URL"] else { throw XCTSkip("Live daemon URL required") }
        let client = try Client(url: XCTUnwrap(URL(string: endpoint)), options: ClientOptions(token: env["MPAI_TEST_TOKEN"]))
        let initialized = try await client.initialize(); XCTAssertNotEqual(initialized, .null)
        let schema = try await client.schema(); XCTAssertNotNil(schema["x-methods"]?["session/create"])
        let harnesses = try await client.discover(); XCTAssertTrue(harnesses.contains { $0.harness == "mock" && $0.available })
        do { let _: JSONValue = try await client.call("not/a/method", params: [String: String]()); XCTFail("Unknown method accepted") }
        catch let error as RPCError { XCTAssertEqual(error.code, -32601) }
        let session = try await client.spawn(InputSpawnOptions(cwd: env["MPAI_TEST_CWD"] ?? "/tmp", harness: "mock"))
        do {
            let receipt = try await client.prompt(sessionID: session.id, content: [.textContent("Reply")]); XCTAssertTrue(receipt.accepted)
            let events = try await client.waitForTurn(sessionID: session.id, options: EventOptions(afterSequence: session.sequence, timeout: 3, pollInterval: 0.005))
            XCTAssertEqual(events.filter { $0.type == "text.delta" }.compactMap { $0.data?["text"]?.stringValue }.joined(), "client-ready")
            XCTAssertEqual(events.last?.type, "turn.ended")
            for pair in zip(events, events.dropFirst()) { XCTAssertLessThan(pair.0.sequence, pair.1.sequence) }
            let replay: [RuntimeEvent] = try await client.call("session/transcript", params: ["sessionId": session.id]); XCTAssertTrue(replay.contains { $0.type == "text.delta" })
            do { _ = try await client.waitForTurn(sessionID: session.id, options: EventOptions(afterSequence: events.last!.sequence, timeout: 0.03, pollInterval: 0.005)); XCTFail("Polling deadline ignored") }
            catch let error as ProtocolError { XCTAssertTrue(error.message.contains("deadline")) }
            catch let error as URLError { XCTAssertEqual(error.code, .timedOut) }
            try await client.close(sessionID: session.id)
        } catch { try? await client.close(sessionID: session.id); throw error }
        let denied = try Client(url: XCTUnwrap(URL(string: endpoint)), options: ClientOptions(token: "incorrect"))
        do { _ = try await denied.initialize(); XCTFail("Invalid auth accepted") } catch is ProtocolError { }
        let bounded = try Client(url: XCTUnwrap(URL(string: endpoint)), options: ClientOptions(token: env["MPAI_TEST_TOKEN"], maxResponseBytes: 1024))
        do { _ = try await bounded.schema(); XCTFail("Byte limit ignored") } catch is ProtocolError { }
    }
}
