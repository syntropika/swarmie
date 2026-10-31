# Node hosting

`swarmie-node` is a Rust library. It owns a worker, a persistent node identity,
exclusive profile locks, and the complete protected client composition. The
composition uses the existing Tembra host, Conversations, Durable Tasks, provider
admission, Git authority, and private native control. It has no dependency on the
Desktop application or the independent hosted-service repository.

## Embedded hosting

`Node::open(profile)` opens the local profile. `Node::with_configuration` accepts
a `HostConfiguration` with a profile, a protected profile, and an optional directory
source. These values come from the native embedding. RPC arguments cannot change
them. A directory source permits browsing. It does not grant Git mutation authority.

The node installs the complete composition before it enables recovery. It retains
the profile locks until physical shutdown finishes. A second owner is refused.
`Node::shutdown` requests host shutdown and waits for Tembra cleanup. Dropping the
node also shuts down its worker. Closing a client channel does not call shutdown.

Desktop imports this library. Its own runtime package supplies the C ABI and the
origin-restricted WebView adapters. Window visibility belongs to the native shell.
The native process must retain its node owner while it remains in the background.

`Node::private_port` is for a trusted native adapter that authenticates its operator
surface. It is not an ordinary RPC capability. Opening a private attachment does
not approve a protected action. Public channels cannot acquire private services.

## Independent hosting

Build the executable with `just node-build`. Start it explicitly:

```sh
swarmie-node --profile /path/to/profile
```

The independent host uses the same Rust composition. It runs until an explicit
SIGINT or SIGTERM requests shutdown. It does not stop when a connected client
exits. It does not install an operating-system service or start at login.

The local listener is restricted to loopback. The profile contains an owner-only
`node-endpoint.json` file with the current address, node identity, frame budget,
and an authentication token. Treat the token as a local access credential. It
grants ordinary node access. It cannot grant operator consent or select another
principal. Restart preserves the node identity and rotates the token. Graceful
shutdown removes only its own current endpoint file.

The host refuses malformed or unsupported endpoint files before node startup.
It checks the file again under exclusive profile ownership before publication.
An unrecognized file remains unchanged.

A client sends the token's 16 UUID bytes first. The host returns one byte with
value `1` after authentication and channel admission. Each subsequent frame has
a four-byte unsigned little-endian byte length followed by the complete Tembra
frame. The public frame budget is currently 64 KiB. At most eight clients can be
attached. Private operator channels are not served by this listener.

The independent host has no private input surface. Work that needs operator
authority remains subject to the existing protection policy. A missing private
decision cannot turn into consent or an unprotected call.

The listener adapter currently targets Unix systems. Other platforms report that
independent hosting is unavailable. The library and Desktop platform support have
their own qualification requirements. `just node-check` checks shared Rust hosting,
the Desktop Rust adapters, and independent process fixtures. It does not qualify
the packaged native application or real credential use.
