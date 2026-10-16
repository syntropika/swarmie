# Shared skills

Swarmie discovers complete skill folders under project `.agents/skills/` and user
`~/.agents/skills/`. The same instructions, scripts, references and assets can be
used by supported Harnesses without a separate content installation for each one.
Place or link an existing skill folder containing `SKILL.md` in a canonical root.
Swarmie does not currently provide an installer, source-update manager or account
skill synchronization.

## Discovery and invocation

The shared catalog resolves canonical origins, aggregates folder aliases, parses
metadata and retains same-name conflicts. It stops descending after finding a
skill, so supporting Markdown files are not separate catalog entries. Project
roots are inspected from the working directory through the nearest repository
root, or only the working directory outside a repository. User scope follows the
execution owner's home. Discovery is bounded and reports unreadable, broken,
oversized or malformed entries. Invalid entries remain inspectable when their
instruction file is readable.

Catalog preference orders project roots before user scope and nearer project
roots before ancestors. This order does not override a Harness's own native
precedence. Different folders declaring the same name remain distinct entries;
links to the same canonical folder appear once with their discovered origins.
Generated session overlays are excluded from catalog discovery.

Each adapter separates native availability from filesystem discovery:

| Harness | Discovery | Explicit invocation |
| --- | --- | --- |
| Codex | App Server `skills/list`, refreshed for the session's working directory; retains native enabled state and metadata | Typed skill input preserving the selected name and exact path |
| Claude Code | Session-owned folder links exposed through `--add-dir`, correlated with native advertised commands and their aliases | An advertised origin-specific directory alias, invoked through the native slash command |
| Pi | RPC `get_commands`, including native source path and scope | Native `/skill:name` expansion after matching the selected origin |

Codex and Pi use their native canonical-directory discovery. Pi's project trust
decision is preserved: a project skill that Pi declines to load is not silently
loaded through an extra explicit path. Native resources can remain available
outside the shared catalog. OpenCode and ACP retain their existing command
interfaces; shared selected-skill inputs are not enabled for them.

Claude's overlay contains links to complete folders and never replaces
`CLAUDE_CONFIG_DIR`, the working directory or authentication configuration.
Distinct canonical origins receive distinct aliases. Swarmie exposes a selected
origin only when the running native command catalog advertises its alias. Existing
personal, project and managed native resources keep their native behavior.

## Using skills in the desktop

The resource library reports whether a skill is merely discovered, advertised by
the live session, incompatible, blocked, or awaiting restart. Opening a discovered
file is not activation. **Use native skill** retains the selected origin and uses
the adapter's native invocation. **Attach instructions as text** adds the file to
one message without emulating skill activation, argument substitution, supporting
files, tool grants or automatic selection.

The slash picker retains distinct paths for same-name native skills. Claude and
Pi accept one selected native skill per message. A new selection replaces the
previous selected skill for those Harnesses; Codex can retain multiple selections.
Pi selected-skill expansion requires a new turn, so queue it rather than steering
an active turn.

The application validates the selection against the session's live native catalog
before preparing attachments. The adapter validates the canonical origin again
before native submission. A missing, disabled, model-only, ambiguous or changed
selection is rejected instead of silently invoking another skill with the same
name. Native textual slash commands remain native textual commands.

## Permissions, changes and lifecycle

Frontmatter is preserved and interpreted by the native Harness. Swarmie does not
translate Harness-specific argument substitutions, fork contexts, model selection
or tool-grant semantics. Supporting metadata files stay in the canonical folder.

Skills operate under the Harness's normal filesystem and tool policy. Linking a
folder does not bypass native deny rules, sandbox restrictions or approval
requests. Tool-disabled Claude sessions do not expose shared skill overlays;
tool-disabled Pi sessions disable native skill discovery and reject explicit
skill-loading arguments. Shared folders bundling additional Claude customization
require a separate integration and are not placed in the plain-skill overlay.

Claude and Pi retain instruction digests for shared skills bound at launch. A
changed instruction file makes explicit selection require restart. New skill sets
also require a native restart/reload supported by that Harness. External edits to
supporting files remain live filesystem edits; Swarmie does not provide immutable
folder snapshots or lock user-owned files. Automatic model activation remains a
native Harness decision, subject to its visibility controls and context budget.

Every Claude launch or native relaunch owns its overlay independently. Normal
close, forced termination and startup failure remove owned links while preserving
canonical targets. Linux can reap unlocked, structurally intact stale overlays
after checking ownership and a known native process lifetime. Unknown process
bindings are preserved. An overlay with unexpected files or a live native process
is preserved. Other Unix platforms use normal
owned cleanup without the Linux stale-process sweep. Windows shared Claude
overlays return Unsupported until directory-link support is qualified.

Paths and catalogs belong to the execution node. Selecting a remote node does not
make a local folder available there; that node must already have its canonical
skill folders. No human account credentials or skill content are synchronized by
this feature.

## Verification

Automated tests cover canonical aliases and cycles, scope conflicts, metadata
errors, selected origins, complete supporting folders, disabled tools, stale
selections, concurrent overlays, failed initialization and cleanup. Native catalog
acceptance is available through the ignored `installed_skills` adapter test with
the installed Harnesses and fixture dependencies. Native catalogs and controlled
model peers establish instruction loading and supporting-file access; they do not
guarantee that a real model will choose the right skill for every natural-language
request.
