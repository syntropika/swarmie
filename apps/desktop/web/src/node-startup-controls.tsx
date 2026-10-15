import { useState } from "react";

import { startAction } from "~/web/src/action-boundary";
import { Button } from "~/web/src/components/ui/button";
import { Switch } from "~/web/src/components/ui/switch";
import type { ClientNode, NodeClient, NodeStartup } from "~/web/src/nodes";
import { hasText } from "~/web/src/value-presence";

export function NodeStartupControls({
  node,
  client,
  busy,
  run,
}: {
  node: ClientNode;
  client: NodeClient;
  busy: boolean;
  run: (action: () => Promise<unknown>) => Promise<void>;
}) {
  const [startup, setStartup] = useState<NodeStartup>();
  const [uncertain, setUncertain] = useState(false);
  const canInspect =
    node.local ||
    node.connections.some(
      (route) => route.transport.kind === "ssh" && !hasText(route.transport.socketPath),
    );
  const inspect = async () => {
    await run(async () => {
      setStartup(undefined);
      const observed = await client.startup(node);
      setStartup(observed);
      setUncertain(false);
    });
  };
  const configure = async (enabled: boolean, boot = false) => {
    await run(async () => {
      setUncertain(true);
      try {
        setStartup(await client.startup(node, enabled, boot));
        setUncertain(false);
      } catch (error) {
        setStartup(undefined);
        throw new Error(
          `${error instanceof Error ? error.message : String(error)} Check startup again before retrying; configuration may have changed.`,
          { cause: error },
        );
      }
    });
  };
  return (
    <section aria-label="Runtime startup" className="mb-5 border-t border-border pt-4">
      <h3 className="mb-2 text-sm font-medium">Runtime startup</h3>
      <p className="text-muted mb-3 text-xs leading-relaxed">
        Keep the runtime available when the app is closed. Changing future startup does not stop or
        restart active work.
      </p>
      {canInspect ? (
        <>
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => {
              startAction(inspect(), () => {
                setUncertain(true);
              });
            }}
          >
            Check startup
          </Button>
          {uncertain && (
            <output className="text-muted mt-3 block text-xs">
              Check the current configuration before trying again.
            </output>
          )}
          {startup && (
            <div className="mt-3 grid gap-3">
              <Switch
                label={
                  startup.trigger === "boot"
                    ? "Start runtime when the computer boots"
                    : "Start runtime when you sign in"
                }
                checked={startup.enabled === true}
                disabled={busy || !startup.available || startup.enabled === null}
                onChange={(enabled) => {
                  startAction(configure(enabled), () => {
                    setUncertain(true);
                  });
                }}
              />
              {(startup.reason || startup.notice) && (
                <output className="text-muted block text-xs leading-relaxed">
                  {startup.reason || startup.notice}
                </output>
              )}
              {startup.available && startup.canEnableBoot && !startup.bootReady && (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy}
                  onClick={() => {
                    startAction(configure(true, true), () => {
                      setUncertain(true);
                    });
                  }}
                >
                  Enable startup before sign-in and after logout
                </Button>
              )}
              {startup.enabled === true && !(startup.running === true) && (
                <p className="text-muted text-xs leading-relaxed">
                  Automatic startup is enabled for the next sign-in or boot. The current runtime
                  keeps running as it is; service supervision starts with the next service launch.
                </p>
              )}
              {startup.running === true && (
                <p className="text-muted text-xs">
                  The operating system is supervising the runtime.
                </p>
              )}
            </div>
          )}
        </>
      ) : (
        <p className="text-muted text-xs">
          Manage startup on this computer or through a saved SSH connection to its default runtime.
        </p>
      )}
    </section>
  );
}
