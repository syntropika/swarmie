# Persistent node transports

## Implemented boundary

The runtime owns execution independently of attached clients. HTTP runtimes expose the common JSON-RPC and event protocol over WebSocket and, on Unix, over an owner-private local socket. The native client connects locally through that socket and remotely through prepared HTTP/WebSocket routes or an owned SSH subprocess.

All persistent channels initialize and verify the retained node identity before becoming usable. Commands retain the caller's exact runtime incarnation, correlate concurrent request identifiers, and deliver unsolicited events through the same connection. A receiver mismatch establishes no effects; lost or malformed acknowledgments retain uncertainty. No dispatched prompt is replayed automatically.

## Local socket lifecycle

The socket directory must be a real private directory owned by the current user; the socket is mode 0600. The socket path is deterministic for the canonical application data root and recorded in runtime discovery. Long roots use a short owner-private user directory to stay within portable Unix socket address bounds. Environment differences between client and runtime do not change the fallback address.

A live listener cannot be replaced. An owned stale socket may be removed after connection refusal. Non-socket entries, unsafe directories, and foreign ownership are rejected without modifying them. Cleanup removes only the bound socket inode. Losing a client does not stop the listener or cancel accepted runtime effects. Explicit runtime shutdown drains connection tasks before socket cleanup.

Discovery verifies the retained identity and treats connection refusal as an unavailable owner. Starting a replacement still requires the released execution-owner lock. A stale endpoint record cannot authorize attachment to or shutdown of another retained node.

## SSH proxy and enrollment

Configured OpenSSH access is required. Batch authentication preserves configured host-key checks and rejects interactive setup; transport creation disables shared control channels and backgrounding. The remote runtime must be installed before proxy attachment. Command discovery checks the noninteractive PATH and then the managed `~/.local/bin/swarmie` launcher; it does not edit shell startup files.

SSH access is available without an account or hosted-service credentials. The SSH view is available before enrollment; adding a node from that view opens the SSH method directly.

Readiness checks distinguish local SSH client availability, remote access failures, missing installation, invalid version output, unavailable runtime services and incompatible protocol responses. The public `SshReadiness` contract uses `installed: null` when installation is unknown; an access failure does not prove that the remote executable is absent. Version output is limited to 256 bytes and its process lifetime to eight seconds. Runtime verification has a separate fifteen-second bound, keeping the complete check within the client request deadline. Raw SSH diagnostic output is not retained or returned to the view. Only a verified identity makes enrollment available.

The client invokes `swarmie runtime proxy`, adding `--socket PATH` only for an explicit override, which copies raw WebSocket bytes to the existing private socket. The proxy creates no runtime state and performs no installation or lifecycle changes.

The Add node dialog checks installation and protocol readiness, retains the verified identity, and requires another check after connection inputs change. Missing or stopped default runtimes expose separate explicit installation/start actions. Enrollment compares a fresh handshake with that exact prepared target before saving the route. SSH routes retain the host and socket, not a fragile forwarded TCP port.

The native client owns and disposes its SSH child. Closing a connection uses bounded transport cleanup followed by child kill and wait. Reconnection constructs a new proxy and verifies identity before any new command. Removing a route closes its transports while preserving remote execution and history.

## Qualification and remaining work

Linux integration tests cover actual Unix sockets and raw stdio proxies, shared runtime identity, accepted work surviving proxy closure, stale-owner reattachment, long configured roots, prepared-target mismatch, SSH subprocess reconnection and disposal, foreign-node replacement rejection, and missing installation. Command fixtures also cover a missing local SSH executable, access failure, invalid or oversized version output, stopped runtime and incompatible initialization responses without creating remote state. They distinguish installation uncertainty from confirmed absence. Native macOS tests cover the same local socket and proxy behavior, including clean proxy exit when a peer has already closed and when the client closes its output pipe before the close reply.

Actual Linux-to-macOS OpenSSH acceptance uses an isolated native runtime installation and an explicitly prepared noninteractive command path. It verifies readiness and enrollment, two clients observing one runtime, remote file edits, native PTY execution, previews over RPC, an accepted turn surviving SSH process loss and atomic executable replacement, reconnection without prompt replay, history retention across owner restart, stale-incarnation rejection, foreign-node replacement rejection, and SSH child disposal without stopping remote execution. A temporary command wrapper supplies only the isolated executable path and delegates authentication and transport to ordinary OpenSSH; it does not simulate the remote runtime or SSH connection.

A further actual SSH acceptance run exercises application session creation and authored input with a retained binary asset through the shared native workflow dispatcher. It verifies account-free enrollment, recovery after proxy loss with exactly one canonical user message, preserved authored metadata and remote execution continuing after route removal.

Guided dialog tests cover the initially available SSH view, default SSH method, readiness, connection-input invalidation, enrollment gating and targeted failure guidance. Missing-installation guidance directs users to the remote computer. Existing gateway tests retain prepared HTTP/WebSocket coverage. Windows local transport, automatic alias discovery, guided updates, unattended boot, release signing, and native desktop visual qualification remain follow-up work.

## Explicit SSH preparation

`nodes/prepareSsh` validates an unused connection name before remote changes and accepts an explicit `install` or `start` action for the default runtime. It freshly inspects the destination and rejects unknown installation/access state, incompatible installations, unsupported actions and custom sockets. An already verified live runtime is returned without replacement or restart. The native operation owns installation, verification and exact-identity enrollment together, and rechecks the local runtime incarnation before registration. Startup uses `swarmie runtime start`, which serializes first launch and attaches only to a verified existing owner; an occupied owner lock or unverifiable live process is not permission to replace it.

For installation, Python 3.9 or later and a supported Linux/macOS x86_64/aarch64 destination are required. The native client selects a trusted configured package for the remote platform rather than its own platform. It verifies the archive before transfer; the remote bootstrap independently verifies SHA-256, package platform and version before invoking the existing installer. Extraction accepts only the five runtime-package regular files, rejects links, traversal, duplicate or extra files, and bounds compressed/expanded input and member count. Private temporary directories hold preparation files. The installer retains its ownership checks and preserves runtime state and user directories. Its locked `--if-missing` mode refuses a managed installation that appeared concurrently instead of turning first installation into an update.

A configured HTTPS runtime release manifest or administrator-provisioned local runtime archive/checksum is required. There is no assumed public release endpoint, repository login, account enrollment or source build on the destination. Missing prerequisites remain visible instead of silently changing the remote host. Overall preparation is bounded to ten minutes, and the view grants the native request a longer deadline. A timeout or lost connection can leave completed installation/startup effects; check before retrying. Preparation does not install or enable an operating-system service, configure unattended boot, install Harnesses, update existing installations, or change host trust.

The bootstrap and seven local/SSH integration cases pass on Linux. Native transport cases also pass on macOS. A real cross-platform OpenSSH acceptance installs a matching native package into an isolated remote user home through native node controls, verifies exact-identity enrollment without an account or bearer, resolves the launcher without PATH edits, leaves a live owner unchanged on repeated preparation, preserves execution lifetime after route removal and client exit, and retains identity and user files across explicit stop/start. This does not qualify graphical pointer interaction, published release hosting, unattended boot, Windows destinations or production-provider execution during installation.
