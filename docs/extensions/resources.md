# Resources and workflows extension

Optional headless commands for project defaults, reusable prompts, daemon-running schedules, peer sessions and historical usage. Register `ResourcesExtension::open(path)?` before building a runtime. The extension retains a weak runtime reference; its background work stops on runtime shutdown.

Extension ID: `resources`. Invoke each command through `extension/invoke` using `arguments` shown below. Unknown or unavailable native capabilities remain explicit errors.

| Command | Arguments | Result |
| --- | --- | --- |
| `credentials.set` | `{id,value}` | `{id,configured:true}`; never returns the value |
| `credentials.remove` | `{id}` | `{removed:true}` |
| `credentials.status` | `{}` | `{credentials:[{id,configured:true}]}` |
| `project.get` | `{projectId}` | `{projectId,revision,cwd,model?,harness?,config?}` |
| `project.set` | `{projectId,expectedRevision,model?,harness?,config?,directoryPath?}` | Updates canonical host Project defaults; omitted fields are retained, config objects recursively patch, null clears a field; directoryPath explicitly links an existing folder |
| `text.generate` | `{projectId?,projectRevision?,cwd?,sessionId?,harness?,model?,config?,text,timeoutSeconds?}` | `{text}`; actual tool-disabled ephemeral native generation or Unsupported |
| `account.quotas` | `{projectId?,projectRevision?,cwd?,sessionId?,harness?,config?}` | Native account quotas with ordinary project controls; no conversation is created |
| `session.resume` | `{sessionId}` | SessionSnapshot; existing live handle or native resume with saved model and controls |
| `session.create` | `{projectId?,projectRevision?,cwd?,sessionId?,harness?,model?,config?}` | Runtime SessionSnapshot; the extension retains the peer handle |
| `templates.list` | `{}` | `{templates:[{id,name,text,aliases}]}` |
| `templates.put` | `{id,name,text,aliases?:string[]}` | Saved template |
| `templates.remove` | `{id}` | `{removed:true}` |
| `templates.render` | `{id,variables:{key:value}}` | `{id,text}`; IDs or unique aliases resolve to the canonical ID; placeholders are `{{key}}`, missing values are errors |
| `resources.list` | `{cwd,harness?,sessionId?}` | `{resources:[{id,name,kind,path,source,native,availability}],commands?,limitations:[]}` |
| `resources.read` | `{cwd?,sessionId?,path}` | `{path,text}`; only previously discovered resource paths are readable |
| `history.list` | `{cwd?,sessionId?,query?,limit?}` | `{items:[{sessionId,sequence,timestampMs,text}]}`; own durable prompt history |
| `schedules.list` | `{}` | `{schedules:[Schedule]}` |
| `schedules.put` | `{id?,sessionId,text,nextRunAtMs,intervalSeconds?,enabled?}` | Saved Schedule |
| `schedules.remove` | `{id}` | `{removed:true}`; active deliveries cannot be removed |
| `schedules.run` | `{id}` | `{id,sessionId,delivery:"started"|"queued",queueId?}` |
| `peers.list` | `{}` | `{sessions:[SessionSnapshot]}` |
| `peers.spawn` | Same as `session.create`, plus optional `text` | `{session:SessionSnapshot,delivery?}` |
| `peers.read` | `{sessionId,afterSequence?,limit?}` | `{events:[RuntimeEvent]}`; durable transcript or explicit live-only fallback |
| `peers.send` | `{sessionId,text}` | `{sessionId,delivery:"started"|"queued",queueId?}` |
| `fanout.start` | `{cwd,text,targets:[{harness?,model?,cwd?}],timeoutSeconds?}` | `{id,status:"running",targets:[...]}`; 1-8 independent native sessions |
| `fanout.status` | `{id}` | Durable job with per-target sessionId/status/text/error |
| `fanout.cancel` | `{id}` | `{job,errors:[]}`; active native sessions are interrupted |
| `fanout.list` | `{limit?,status?}` | `{jobs:[{id,status,createdAtMs,targets:[{harness,model,sessionId,status}]}]}`; newest first, no result text |
| `fanout.remove` | `{id}` | `{removed:true}`; refuses running/draining native operations |
| `usage.dashboard` | `{cwd?,harness?,sinceMs?,untilMs?}` | `{totals,byDay,byHarness,byModel,sessions,unknownCosts}`; observed historical usage only |
| `tools.list` | `{}` | MCP-compatible schemas for peer spawn/read/send and fanout start/status/cancel |

Explicit launch harness and model choices take precedence over project defaults. Nonsecret native configuration merges project defaults first, then explicit launch values. Omitted fields are retained; config objects recursively patch nested objects, and null clears saved fields. Project defaults belong to the host Project identity. A directoryPath update explicitly changes its linked directory for future launches; no worktree or session folder is created. A conflicting explicit cwd is rejected when a Project is selected. Session-scoped resource/generation/account commands use the retained directory and saved nonsecret settings rather than current Project defaults. Resume reuses the session's saved model and native controls. Authentication and execution configuration come from the native harness installation and daemon environment.

Operational resource state is stored in the runtime host database under the `resources` namespace and loaded after runtime initialization. Namespace writes are individually atomic; they do not share a transaction with Project edits or native effects. A restarted extension must attach to the same host store to recover its state. Legacy resources.json files are preserved and rejected explicitly; choose a fresh development resource root. The separate credential file uses owner-only permissions on Unix. Credential vault values are local plaintext and are never returned by commands. The vault is separate from native launch authentication. Native `config` carries nonsecret controls and endpoints: sensitive field names are rejected recursively, including environment/header containers and credential markers. Configure secrets in the native harness. Numeric and boolean token controls such as maxTokens/tokenBudget remain usable. Null can clear sensitive fields. Unsupported native options fail through the runtime's capability checks.

Schedules run while the daemon is alive. A recurring schedule delivers at most once per interval, skips missed intervals, and never overlaps its own delivery. A busy target receives a queued prompt; pending permission requests remain interactive. Overdue work runs once on the next daemon start. Interrupted deliveries are paused for inspection instead of retried blindly.

Native resource discovery uses the shared canonical skill catalog plus actual harness directories and project instruction/template files. Folder aliases share one canonical skill entry. See [shared skills](../runtime/skills.md) for native discovery and invocation. It does not promise that a resource is loaded by a harness merely because a file exists. Runtime command catalogs are included when a live session reports them. Reading a skill/template does not enable it or execute an instruction.

The daemon's `swarmie tools http://127.0.0.1:7777/rpc --extension resources` command bridges MCP stdio tools to these public commands. Supply `SWARMIE_TOKEN` when the daemon requires authentication. Native MCP declarations can launch that command, for example:

```json
{"name":"peers","transport":{"type":"stdio","command":"/absolute/path/swarmie","args":["tools","http://127.0.0.1:7777/rpc","--extension","resources"],"env":{"SWARMIE_TOKEN":"your-daemon-token"}}}
```

MCP-capable providers can use this declaration subject to their reported native transport support. Pi accepts managed STDIO/Streamable HTTP definitions through its native session MCP registration API; see [MCP management](../runtime/mcp-management.md). The separate optional Pi peer-tools extension can still be loaded directly: explicitly load `pi/peer-tools.ts` with Pi's `--extension` option and set `SWARMIE_RPC_URL` to the daemon RPC endpoint, plus `SWARMIE_TOKEN` if needed. The optional bridge reads the same public tool schemas, registers only the six peer/comparison operations, and forwards actual calls. It never exposes credential commands or silently changes a harness installation. Ordinary extension commands remain headless usable for every provider.

Historical dashboards count per-call/turn reports and increments of cumulative counters within each native session. Inherited fork journal events are excluded; a child session's first actual cumulative report is counted as reported because native counter reset/carry semantics are not guessed. Missing fields are recorded as unknown; known cost is a subtotal. UTC dates use the timestamp of each observed event. State retains at most 1,000 prompts and 10,000 usage reports and signals truncation. Templates accept at most 16 unique identifier aliases, shared across the catalog namespace with template IDs. Omitting aliases when updating a template retains existing aliases; an empty array clears them. Comparisons retain at most 128 jobs until explicitly removed. Intermediate text persistence is coalesced to the 250 ms daemon clock; status changes, completion and orderly shutdown flush immediately. An abrupt process loss can omit the latest intermediate comparison text, while canonical session journals retain their normal durability guarantees. Comparisons preserve up to 16 KiB of each target's text with a truncation flag; their sessions retain the full canonical transcript according to the runtime store policy.
