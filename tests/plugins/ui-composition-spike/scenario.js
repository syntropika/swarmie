(async () => {
  const spike = window.uiSpike;
  const evidence = [];
  function check(condition, description) {
    if (!condition) {
      throw new Error(description);
    }
    evidence.push(description);
  }
  await spike.install();
  check(
    Boolean(document.querySelector('[data-screen="counter"]')),
    "Managed screen mounted from an approved module export",
  );
  check(
    document.querySelector('[id="counter.increment"]') !== null,
    "Managed action contributed to the shell",
  );
  check(spike.snapshotStable(), "React snapshot identity is cached between publications");
  await spike.increment();
  check(spike.diagnostics().snapshot.count === 1, "Action invokes the current Tembra operation");
  check(
    spike.diagnostics().snapshot.serverResult > 0,
    "Feature action reaches the actual native runtime",
  );
  const { publications } = spike.diagnostics().observation;
  await spike.update();
  check(spike.diagnostics().snapshot.count === 1, "Compatible reload retains existing State");
  check(
    spike.diagnostics().observerInstalls === 1,
    "The exact State observer remains installed across reload",
  );
  check(
    document.querySelector("#screen-label").textContent === "Counter screen 2",
    "Registry replacement selects the new implementation",
  );
  const afterReload = spike.diagnostics().observation.publications;
  await spike.increment();
  check(spike.diagnostics().snapshot.count === 3, "Retained Reference invokes replacement handler");
  check(
    spike.diagnostics().observation.publications === afterReload + 1 && afterReload >= publications,
    "Retained observer receives one publication for the new action",
  );
  check(
    document.querySelector("#count").textContent === "3" &&
      document.querySelector("#generation").textContent === "2",
    "React renders State published by the replacement handler",
  );
  await spike.toggleShell();
  check(
    document.querySelector('[data-screen="counter"]') === null,
    "Removing the shell unmounts its surface",
  );
  check(
    spike.diagnostics().contributions.length === 2,
    "Independent feature contributions remain dormant",
  );
  await spike.toggleShell();
  check(
    document.querySelector('[data-screen="counter"]') !== null,
    "Restoring the shell reuses live feature contributions",
  );
  await spike.failRenderer();
  check(
    document.querySelector("#renderer-error") !== null &&
      document.querySelector('[id="counter.increment"]') !== null,
    "Renderer failure is contained to its contribution",
  );
  await spike.refresh();
  check(
    document.querySelector('[data-screen="counter"]') !== null,
    "Renderer can recover without restarting the Host",
  );
  await spike.retireDuringResolution();
  check(
    spike.diagnostics().staleResolutions > 0,
    "Late renderer resolution cannot mount after retirement",
  );
  check(
    spike.diagnostics().contributions.length === 0 &&
      spike.diagnostics().referenceStatus === "gone",
    "Retirement removes owned entries and revokes the Reference",
  );
  check(
    document.querySelector('[data-screen="counter"]') === null &&
      document.querySelector('[id="counter.increment"]') === null,
    "No orphaned view or action remains",
  );
  const final = await spike.close();
  check(
    final.resources.opened === final.resources.closed &&
      final.host.callbacks === 0 &&
      final.host.jobs === 0,
    "Host closure releases managed resources, callbacks and jobs",
  );
  return { passed: evidence.length, evidence, final };
})();
