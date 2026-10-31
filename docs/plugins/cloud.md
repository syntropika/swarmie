# Swarmie Cloud client

`swarmie-cloud` contains native account and device-linking interfaces in
`plugins/cloud`. Hosted identity, enrollment and routing authority remain in
the independent hosted service. This package does not run that service.

## Account sessions

`swarmie_cloud::account` owns native account authorization, retained sessions
and origin-bound HTTPS requests. The embedding supplies browser presentation,
protected session storage and its I/O runtime. The `native-keyring` feature
provides the concrete operating-system session store.

## Device linking

`swarmie_cloud::device_linking` owns endpoint keys, link offers and authenticated
account device operations. `CloudDevice` uses the existing `AccountSession`.
Device registration belongs to that account session. Public device identifiers
do not grant access to private keys or transport authority.

## Vault integration

Vault's `cloud-client` feature uses this package for the account session and
HTTPS transport. Vault retains its encrypted records, keys and access rules.
Unlock requires a live account binding and a Cloud-authorized release. An
unavailable Cloud service does not authorize an offline release.
