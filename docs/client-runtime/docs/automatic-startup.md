# Runtime automatic startup

Automatic startup is a native node operation. The node details view provides an explicit check followed by an enable or disable action. Inspection, pairing, package installation and ordinary reconnection do not configure startup. Local runtime attachment remains automatic when a client opens.

The native gateway resolves a remote target from its saved SSH route, node identity and observed connection revision. It rejects stale revisions, other node identities, direct endpoint routes and custom runtime sockets. It verifies the remote runtime before changing its service configuration and passes the retained identity to the destination CLI. The destination checks that identity again. A runtime restart can retain the node identity; replacing its state directory cannot authorize a service change for the original node.

## Service scopes

| Platform | User startup | Startup without sign-in |
| --- | --- | --- |
| Linux | User systemd service | Explicit `--boot` enables user lingering after a capability check |
| macOS | LaunchAgent in the user GUI domain | Requires a separately managed system daemon |
| Windows | Task Scheduler logon task for the current user | Requires a separately managed administrative task or Windows service |

Windows uses an interactive user token and least privilege. No password is collected or stored. A configuration-specific task name prevents temporary or separate installations from changing each other's tasks. The task runs the runtime directly, has no time limit, ignores duplicate launches and requests recovery after failure. The task is not a Windows system service and does not establish availability before user sign-in or after user sign-out.

Linux boot enablement explicitly requests `loginctl --no-ask-password enable-linger` for the current user. It does not use sudo or install a privileged service. If host policy denies the request, startup is not installed or enabled by that action. Disabling runtime startup preserves lingering because other user services may need it.

## CLI and ownership

```sh
swarmie service startup inspect
swarmie service startup enable
swarmie service startup disable
swarmie service startup enable --boot
```

The final command applies only to Linux. Use an explicit `--state-dir` when managing a nondefault runtime. Native SSH startup management currently addresses the default runtime.

The automatic profile binds authenticated HTTP to an ephemeral loopback port and publishes the private local transport. It enables the common workbench extensions and uses the runtime data root for managed worktrees and resources. It does not move Project working directories or turn session directories into permission boundaries.

Configuration is serialized by an operating-system file lock. A retained marker records the canonical state root and unit checksum. Unknown, modified or foreign units and ownership markers are preserved and require manual resolution. Windows also compares the registered task's action, principal and trigger before allowing a change. Comparison resolves account names and SID strings to the same current user and recognizes the scheduler's omitted default least-privilege setting. Registration supplies a UTF-16 declaration for the managed XML string while preserving the recorded UTF-8 definition.

Enablement never starts, stops or restarts the active runtime. Disablement changes future startup and preserves current work. Results report whether operating-system supervision is running and whether supervision awaits the next service launch. A detached runtime remains detached until an explicit service launch or the next applicable login/boot; installing a service cannot transfer a running process to a supervisor.

A failed or unconfirmed action requires a new inspection before retry. A service configuration may have changed even when its acknowledgment was lost. The UI clears the prior observation after an uncertain mutation and offers inspection rather than repeating it automatically.

## Qualification

Linux tests cover owner and identity preservation, explicit lingering, denied host policy, modified units and stale node routes. Native macOS checks validate the LaunchAgent with the operating-system parser, enable and disable a disposable label without interrupting its detached owner, and verify real service supervision after an explicit launch. Native Windows x86_64 checks cover named-pipe attachment, authenticated HTTP, retained identity, logon-task enablement and disablement, and binary-safe SSH streams. A disposable task launches the runtime through the real scheduler; inspection confirms supervision, disabling future startup preserves its incarnation, and explicit service stop and uninstall succeed. The release workflow prepares Linux, macOS and Windows desktop and runtime packages on x86_64 and aarch64.

Compilation and local fixtures do not establish native operating-system behavior. Native Windows desktop launch, live OpenSSH enrollment and RPC, and Task Scheduler launch are qualified on x86_64. Sign-out/reboot, crash recovery, native MSVC release builds, ARM64 execution, and authenticated external Harness turns remain separate qualification requirements. macOS graphical login and system-daemon scope also require native qualification. Supervision does not guarantee that an interrupted Harness turn automatically resumes.
