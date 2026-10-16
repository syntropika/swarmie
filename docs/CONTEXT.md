# Execution domain

The application presents conversations and connects them to prepared execution nodes. Projects organize work; execution placement, working directories, and permissions have separate meanings.

## Language

**Environment**:
A prepared execution node with its installed system, Harnesses, tools, and configuration. It can support unrelated Projects and conversations.
_Avoid_: Using Environment to mean a Project folder or a preparation template.

**Node**:
The execution host referred to as an Environment in the product. Node and Environment do not denote two separate levels of execution placement.

**Project**:
An individual's or a collaboration scope's organized work and related resources. A Project does not own an Environment.

**Workspace**:
A premium collaboration scope for people, Agents, and shared work. Individual Projects do not require a Workspace.
_Avoid_: Using Workspace to mean an execution directory or a cloud instance.

**Working directory**:
The folder selected for a session's execution on its Environment. Local sessions use a linked Project folder, the owner's project-free default, or an explicit selection.

**Session work area**:
A managed cloud session's parent area for worktrees and other session files. It organizes session resources independently of their execution permissions.

**Conversation**:
The logical history presented by the application, including its participants and authored contributions. Its visible history is distinct from a Harness's native context.

**Harness**:
The execution system that runs the native agent loop and manages native context. It is distinct from the configured Agent and its model.

**Native session**:
The conversation and context managed by a Harness for execution.

**Execution binding**:
The association between a conversation's execution and a particular native session on an Environment.

**Runtime**:
The execution owner on an Environment. It manages native processes and authoritative session state independently of the connected application's window.

**Client**:
An interface that observes and controls a Runtime through the common protocol. Closing a client does not stop its Runtime or another client's execution.

**Runtime incarnation**:
One live execution owner on an Environment. A replacement owner has a new incarnation even when the retained Environment remains the same.

**Reconnect**:
Reestablishing observation or control of the same retained Environment. Reconnecting after an owner restart requires reconciling retained execution facts.

**Migration**:
Moving execution or its context to a different Environment. It is a separate operation from reconnecting.
