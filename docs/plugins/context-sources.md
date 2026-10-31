# Context sources

`swarmie_context::environment` reads native environment instructions and skills. It has no model, scheduler,
PluginManager or Tembra host dependency. The embedding authorizes paths and users.
Run its synchronous filesystem operations on a blocking worker. This module is
separate from the client Projects feature that groups conversations.

## Select a scope

Construct `Scope::new(project_root, working_directory)`. Both paths must exist.
The working directory must be inside the project root. Set additional skill roots
and personal instruction files in `Configuration`. The library does not search the
home directory or read environment variables.

`Environment::inspect` discovers packages with a `SKILL.md` file. It reads configured
skill roots and `.agents/skills` in each directory from the project root to the working
directory. Discovery stops at each package. An `Environment` retains its first
successful inventory. Construct another owner to refresh discovery.

The inventory includes origins, aliases, metadata and diagnostics. Distinct origins
with the same name remain distinct. `Inventory::select` accepts a unique name or an
exact `id@revision`. An ambiguous name fails. Aliases of one physical package share
one identity. The package revision covers all files, relative paths, link targets
and executable permission bits on Unix. Binary resources contribute to the revision.

## Read a skill

A skill requires YAML frontmatter with `name` and `description`, followed by an
instruction body. Names use lowercase letters, digits and single hyphens.
The parser accepts a UTF-8 byte order mark, CRLF and YAML multiline descriptions.

`disable-model-invocation: true` removes the skill from the model catalog.
`user-invocable: false` prevents explicit user selection. Advisory metadata remains
available in the inventory. Unsupported execution fields appear in `unsupported`.
Such skills cannot be selected or read. Dynamic shell preprocessing is unsupported.
Metadata never grants tool permission or starts a process.

Call `read(&selection, "SKILL.md")` to read the body. Use the same selection with a
relative path to read a supporting UTF-8 file. The library checks the complete package
revision again. Changed content fails and requires a new selection. It does not return
a truncated resource. Scripts and binary assets remain on disk and are not executed.

Package links can resolve inside the package. Links that escape the package and
cyclic links fail. Absolute paths and parent components fail. These checks do not
provide operating-system isolation or an atomic snapshot against concurrent writers.

## Capture current instructions

`capture(node, principal, selected)` reads configured personal instruction files first.
It then reads `AGENTS.md` from the project root to the working directory. Later files
have more specific scope. Missing project files are optional. A missing configured
personal file, read error or invalid UTF-8 fails. Project instruction links must stay
inside the project root. Duplicate physical instruction files retain their most
specific occurrence. Capture checks the instruction bytes again before it returns.

The result is a `ContextSources` value with scope, source kind, identity, revision,
label, content and content digest. It contains the available skill descriptions and
explicitly selected skill bodies. It does not copy packages into another directory.

Default limits are 256 skills, 2,048 entries per traversal, depth 16, 16 MiB per
package, 32 MiB per library and 64 KiB per text file. An operational limit fails the
discovery. Malformed packages produce diagnostics. Shared request limits also require
at most 128 parts, 64 KiB per part and 256 KiB per source capture. JSON overhead counts.

## Compose a model request

`AgentContext.sources` is separate from canonical messages. `model_messages()` inserts
one User message with the source capture immediately before the admitted User message.
The historical summary and retained suffix precede current sources. Host system
instructions remain in `AgentContext.instructions`.

Providers must encode and measure the same `model_messages()` view. The AI adapter
uses this view. Sources consume the fixed request budget. Textual compaction excludes
current sources from its historical summary input. Conversation checkpoints retain
the capture and restore its placement after history projection. A context hook cannot
change the capture or its admitted input anchor. Recovery checks the frozen capture
before it asks a provider to recognize an outcome.

Prepared request records contain source evidence. Canonical message records do not
contain the inserted source message. Source labels do not grant filesystem access.

## Use the native node

Set `ServicesConfiguration.environment` to `Some(Configuration)` to enable acquisition
for conversations with a validated Git checkout. `swarmie-node` enables the default
configuration. This configuration adds no personal roots. The native working directory
is the captured checkout root.

Start a message with `/skill:name` or `/skill:id@revision` for explicit selection.
The original text remains in canonical history. Ordinary requests advertise available
skills by description. The model can call `skill_read` with `id`, `revision` and `path`.
Only a skill advertised or selected for the admitted physical turn can be read.

The owning conversation session exposes `project_skills(binding)` through Tembra.
The result includes the inventory and diagnostics. A response above 256 KiB fails.
The session validates the captured checkout before discovery or resource access.
Tool preparation records this session as the provider. Dispatch checks that identity,
the authenticated principal, the physical turn and the captured source scope.
Tembra owns References and their release. An uncertain read returns `Unknown` during
recovery and cannot authorize automatic replay.

The native source capability does not install dependencies, execute skill scripts,
change model permissions or provide a graphical skill picker.
