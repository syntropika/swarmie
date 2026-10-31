import type { SecretMetadata } from "@swarmie/plugin-vault-browser";
import { Input, Label, TextField } from "@swarmie/ui-web";

const scope = (value: SecretMetadata["projects"]) =>
  value.kind === "only" ? value.ids.join(", ") : "";

/** Read public policy fields only; secret inputs are never inspected here. */
export function readPolicy(form: HTMLFormElement, previous: SecretMetadata): SecretMetadata | null {
  const field = (name: string) => {
    const input = form.elements.namedItem(name);
    return input instanceof HTMLInputElement || input instanceof HTMLSelectElement
      ? input.value
      : "";
  };
  const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu;
  for (const name of ["projects", "agents"]) {
    const input = form.elements.namedItem(name);
    if (input instanceof HTMLInputElement) {
      const value = input.value.trim();
      input.setCustomValidity(
        value && value.split(/\s*,\s*/u).some((id) => !uuid.test(id))
          ? "Enter comma-separated UUIDs."
          : "",
      );
      if (!input.reportValidity()) {
        return null;
      }
    }
  }
  const ids = (name: string): SecretMetadata["projects"] => {
    const value = field(name).trim();
    return value ? { kind: "only", ids: value.split(/\s*,\s*/u) } : { kind: "unrestricted" };
  };
  const choice = field("approval");
  return {
    name: field("name"),
    purpose: field("purpose"),
    projects: ids("projects"),
    agents: ids("agents"),
    approval: choice === "operation" || choice === "conversation" ? choice : "none",
    policy_generation: crypto.randomUUID(),
    target_account: previous.target_account,
  };
}

export function PolicyFields({ metadata }: { metadata: SecretMetadata }) {
  return (
    <>
      <TextField name="name" isRequired defaultValue={metadata.name}>
        <Label>Name</Label>
        <Input maxLength={256} />
      </TextField>
      <TextField name="purpose" defaultValue={metadata.purpose}>
        <Label>Purpose</Label>
        <Input maxLength={4096} />
      </TextField>
      <TextField name="projects" defaultValue={scope(metadata.projects)}>
        <Label>Project IDs</Label>
        <Input placeholder="All projects, or comma-separated IDs" />
      </TextField>
      <TextField name="agents" defaultValue={scope(metadata.agents)}>
        <Label>Agent IDs</Label>
        <Input placeholder="All agents, or comma-separated IDs" />
      </TextField>
      <p className="text-xs text-muted">Both project and agent restrictions must match.</p>
      <label className="grid gap-2 text-xs">
        Approval
        <select
          name="approval"
          defaultValue={metadata.approval}
          className="rounded-lg bg-default px-3 py-2"
        >
          <option value="none">No additional approval</option>
          <option value="operation">Every operation</option>
          <option value="conversation">Operation or main agent in this conversation</option>
        </select>
      </label>
    </>
  );
}
