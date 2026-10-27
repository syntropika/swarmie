# Node connections

A node has a stable identity. Routes and selected UI nodes can change without
changing the destination of existing work.

Tembra owns RPC, references and State delivery. The embedding owns channel
authentication, the listener and transport queues. Connections coordinates one
independent client session per node.

## Verify a node

Compose `swarmie_runtime::node_orb(NodeId)` before exposing the host. The embedding
creates and persists the ID. It contains 1–128 UTF-8 bytes without whitespace.

The service uses `NODE_SERVICE`, `NODE_KEY` and `NodeMethod::Identify`. It returns
`NodeIdentity` with the node ID, invocation principal and binding protocol version.
This confirms application expectations after authentication; it does not authenticate
the transport or persist the ID itself.

A configured connection becomes ready only after checking node, principal and
version. Credentials belong to the connector and are absent from serialized State.
A WebSocket URL is a route, not a node identity.

## Own a session

`Connections.open(nodeId)` returns a connection bound to a node, principal and
session generation. Its `get` acquires references through that session. Consumers
release references and subscriptions when the session ends. References do not
migrate to a replacement session.

Readiness has a ten-second deadline. Network failures can retry with bounded
backoff and a new generation. Identity, protocol and resource-limit failures are
terminal. Late connector results and stale loss callbacks cannot replace or close
a newer session. Disconnecting one node leaves other nodes usable.

The direct connector uses binary WebSocket frames and a 64 KiB Tembra frame limit.
The server must use a matching limit. A local oversized call is refused without
closing unrelated references. An oversized live State closes its whole channel.
Map resource or queue exhaustion to WebSocket code 1009. Protocol or policy refusal
uses 1002, 1003 or 1008. The connector treats these codes as terminal; network loss
remains retryable.

## Bind work to a destination

Connections exposes selection separately from per-node sessions. Selection is a
UI default. Sending and stopping capture the conversation's bound destination.

Bind the destination before the first dispatch. A failure before dispatch leaves
the draft unsent. A failure after dispatch leaves admission unknown and the
destination fixed. It cannot redirect or automatically replay on another node.
Only authoritative admission reconciliation can resolve that uncertainty. A stop
acknowledgement cannot make a submission with an unknown outcome safe to send again.
Cancellation targets a conversation, not an individual turn. A production adapter must
supply narrower cancellation guarantees if it needs them.

The embedding supplies `execution(connection)` to bind execution references to
that exact session. It must not read a global selected node. Session validity is
checked before dispatch and after acknowledgement. Later draft edits survive an
older acknowledgement.

The current frontend conversation store is volatile. Awaiting a managed UI update
is not durable backend admission. History create/append alone is not idempotent
admission either. A production execution adapter must supply that authority.

## Native integration and limits

With the runtime `rpc` feature, the embedding can call
`swarmie_runtime::rpc::serve(runtime.host().clone(), options, send)`. This is the
upstream Tembra implementation, with no second protocol engine. The embedding
supplies a bounded byte transport and an authenticated principal policy.

The Rust dependency does not upgrade the browser SDK. Consume a matching published
SDK and browser engine pair. Qualify reference cleanup and session replacement with
that installed pair; a native dependency update alone does not verify browser behavior.
An aborted acquisition race previously retained a late reference until channel close.
That race has not been qualified with the current browser pair; do not assume immediate
cleanup from the native or ordinary browser checks.

Shared Projects authority, hosted routing, relay channels and mobile runtime
qualification are separate integrations. Direct node access supplies none of these.
