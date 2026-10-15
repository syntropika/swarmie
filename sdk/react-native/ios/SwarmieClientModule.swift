import CSwarmie
import Foundation
import ExpoModulesCore

public final class SwarmieClientModule: Module {
  private static func invoke(_ command: String) throws -> String {
    guard swarmie_client_abi_version() == 1 else {
      throw NSError(domain: "SwarmieClient", code: 1)
    }
    return try command.withCString { input in
      guard let result = swarmie_client_execute(input) else {
        throw NSError(domain: "SwarmieClient", code: 2)
      }
      defer { swarmie_client_free(result) }
      return String(cString: result)
    }
  }

  public func definition() -> ModuleDefinition {
    Name("SwarmieClient")

    Function("control") { (command: String) in
      try Self.invoke(command)
    }

    AsyncFunction("execute") { (command: String, promise: Promise) in
      // Blocking Rust operations must never occupy the JS or main thread.
      DispatchQueue.global(qos: .userInitiated).async {
        do {
          promise.resolve(try Self.invoke(command))
        } catch {
          promise.reject("ERR_SWARMIE_NATIVE", "Native client operation failed")
        }
      }
    }
  }
}
