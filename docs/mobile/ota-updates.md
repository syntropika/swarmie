# Mobile OTA updates

The mobile app uses the installed `expo-updates` client and the Expo Updates protocol.
`APP_VARIANT` selects the public update application ID, channel and verification certificate
in `apps/mobile/app.config.ts`. Both supported variants use the configured HTTPS manifest
endpoint. They have independent application records, publishing credentials and signing keys.

| Variant | Channel | Default server branch | Public certificate |
| --- | --- | --- | --- |
| `development` | `development` | `develop` | `apps/mobile/certs/development.pem` |
| `production` | `production` | `production` | `apps/mobile/certs/production.pem` |

The request headers declare `expo-app-id`, `expo-channel-name` and an empty `xprem-branch`.
The empty branch header preserves normal server-side channel resolution while reserving
the header for any future explicit branch switching. This integration does not add a branch
switching UI. Channels are resolved within their own application.

## Signing and startup

Each native binary embeds its variant's public RSA certificate. Updates must be signed
with the corresponding private key using `rsa-v1_5-sha256`, key ID `main`. Public certificates
are tracked in source; publishing tokens and private signing keys are not app configuration.
Changing the endpoint, app identity, channel or certificate requires a new native build.

The app checks for updates on launch and starts its cached or embedded bundle without
waiting for the network (`fallbackToCacheTimeout: 0`). A compatible downloaded update is
available on a subsequent launch. Embedded updates and the default update recovery measures
remain enabled. Development sessions connected to Metro do not qualify release OTA behavior.

## Native compatibility

`runtimeVersion` uses the `fingerprint` policy. The same app variant and build configuration
must be used when producing a native binary and publishing its updates. The backend URL
remains the independently configured `MOBILE_API_BASE_URL`; the update endpoint does not
provide account authentication or replace that backend.

`apps/mobile/fingerprint.config.cjs` adds the Rust client and C ABI crates, WIT/header
boundary, Cargo manifests and lockfile, pinned toolchain, native build scripts and public
OTA certificates to Expo's native dependency/configuration inputs. Generated Rust libraries
are excluded: compatibility is derived from their tracked source inputs, so publishing does
not require staging machine-specific binaries. The autolinked native bridge remains part
of Expo's normal inputs.

App version and native build counters are excluded from the fingerprint. Increasing a
TestFlight build number alone therefore does not require another OTA runtime. Native
source, dependency, plugin, certificate or configuration changes can require a new binary.
OTA updates only replace JavaScript and eligible assets for the matching runtime and platform.

## Qualification

Existing installed builds with updates disabled require a new native build before they
can contact the update service. The source integration does not publish an update, add
publishing credentials to GitHub, or trigger a signed build or store upload.

After installing a new development Release build, publish a small visible JavaScript
change to its branch using the matching runtime. Open the app online, then relaunch it and
verify the change. Also verify offline startup and rejection of an update from another
runtime or signing key before enabling a production publishing workflow.

## References

- [Update client configuration](https://docs.expo.dev/versions/v57.0.0/sdk/updates/)
- [Runtime fingerprints](https://docs.expo.dev/versions/v57.0.0/sdk/fingerprint/)
- [Update server application configuration](https://mercure-technologies.gitbook.io/xprem/installation-guide/configure-your-application)
