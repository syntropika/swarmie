# Remote qualification status

The previous tunnel-based runner has been retired with the removed CLI forwarding supervisor. Its commands and authentication assumptions do not describe the current native SSH transport.

Current transport integration coverage is in `crates/app/tests/node_local_transports.rs`. It exercises private local sockets, the stdio byte proxy, SSH child ownership, reconnect identity checks, and execution survival after disconnect. Desktop unit tests cover the guided SSH preparation and enrollment contract. These tests do not establish live remote platform or graphical qualification.

A future acceptance runner must qualify native desktop node enrollment and remote workspace interaction through the native client gateway and saved SSH proxy route. Use a separately built remote runtime, disposable state and working directories, the host's ordinary SSH authentication, and an existing runtime listener. Verify shared execution across clients, process cleanup, interruption without prompt replay, identity continuity, and preserved user resources. Keep operational evidence outside the repositories.
