package app.swarmie.client

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.functions.Coroutine
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class SwarmieClientModule : Module() {
  companion object {
    init { System.loadLibrary("swarmie_mobile") }
  }

  private external fun invoke(command: String): String

  override fun definition() = ModuleDefinition {
    Name("SwarmieClient")
    Function("control") { command: String -> invoke(command) }
    AsyncFunction("execute") Coroutine { command: String ->
      withContext(Dispatchers.IO) { invoke(command) }
    }
  }
}
