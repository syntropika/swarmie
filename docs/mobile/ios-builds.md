# iOS builds and TestFlight

The **Mobile iOS** GitHub Actions workflow builds the Expo application with Fastlane on
GitHub-hosted macOS. It uses local Expo prebuild, CocoaPods and the shared Rust SDK without
EAS infrastructure. Ruby, Node, Bun, Rust, Just and the Xcode selection come from the root
`mise.toml`; Ruby dependencies are locked in the root `Gemfile.lock`.

## App environments

| Variant | Bundle identifier | URL scheme |
| --- | --- | --- |
| Development | `app.swarmie.mobile.dev` | `swarmie-dev` |
| Production | `app.swarmie.mobile` | `swarmie` |

Both variants display the name **Swarmie**. Identifiers and link schemes separate their
installation and callback namespaces. App Store Connect associates uploads with the bundle
identifier, independently of its app record's display name.

`APP_VARIANT` selects the identifier and scheme; local commands default to development.
`MOBILE_API_BASE_URL` is a public HTTPS URL without credentials, query parameters or fragments.
It is embedded as `extra.apiBaseUrl`, alongside `extra.appVariant`. Do not store credentials
in public Expo configuration. `IOS_BUILD_NUMBER` is a numeric build string; the workflow
uses its run number and attempt so retries receive a different build number.

Both variants compile in Release with bundled JavaScript. Development service selection
does not depend on `__DEV__`, and installation does not require a Metro server. New native
builds embed the variant's signed OTA configuration; see [OTA updates](ota-updates.md).

The mobile account adapter is still unconfigured. Embedding the backend URL prepares its
configuration but does not implement native account sign-in, passkeys or provider access.
The initial native build qualifies packaging and native linking, not live authentication.

## Workflow behavior

Every relevant pull request and manual run checks the tooling and mobile code, exports
Expo, then resolves the development iOS runtime fingerprint. If it matches the last
successfully uploaded signed build, the run finishes after Expo export and does not
allocate a macOS runner. Otherwise, it creates a native Release archive.

The baseline is the `ios-development-native-baseline` artifact from a successful manual
run on the default branch. It records the fingerprint read from the signed IPA, which
must match the Linux planning step before upload. Pull-request, failed, expired and
other-branch artifacts cannot establish a baseline. The first run without a retained
baseline requires a native build. Missing API access or invalid metadata fails planning
rather than assuming compatibility. Baseline metadata is retained for 90 days.

Pull-request native builds are unsigned. This path receives no signing password, SSH
identity or Apple API key and does not
produce a distributable IPA. The archive check verifies the development identifier and
that JavaScript is bundled inside the app.

Manual execution is allowed only from the default branch. It selects development explicitly,
imports existing signing material through readonly Match and exports an App Store-signed
IPA when the native runtime has changed. It cannot create certificates, register identifiers or modify signing storage. Use the
separate [signing maintenance workflow](apple-signing.md) for those operations.

The `upload` input defaults to true. Upload uses the existing `ASC_SIGNING_API_*` credentials
only in the TestFlight upload step. The compiler does not receive that API key. Setting
`upload` to false produces a signed IPA when rebuilding is necessary without contacting
App Store Connect. Such a run does not replace the uploaded-build baseline. A matching
runtime skips native compilation and TestFlight upload regardless of this input. Expo
exports are retained for seven days; export alone does not publish an OTA update.

The workflow uploads the build without waiting for Apple processing or distributing it to
testers. Configure internal testing in App Store Connect after processing. It does not
submit an App Store version for review or publish a store release. Signed IPA files,
symbols and public build metadata are retained as GitHub artifacts for 14 days; Xcode build
logs are retained for seven days, including failed builds when logs are available.

## Repository configuration

Use the existing signing secrets and variables from the maintenance workflow. Builds need
the read-only `MATCH_SSH_PRIVATE_KEY`, not the maintenance write key. Set the repository
variable `MOBILE_DEV_API_BASE_URL` to the development backend's HTTPS URL. The backend URL
is public configuration and does not require a GitHub secret.

An App Store Connect app record must exist for `app.swarmie.mobile.dev` before upload.
Its display name may differ from the name registered for the identifier in the Developer
portal. A production identifier requires its own app record and provisioning profile;
the current manual workflow uploads development only.

After the workflow reaches the default branch:

1. Open **Actions → Mobile iOS → Run workflow**.
2. Choose the default branch and enable `upload` to build and upload to TestFlight.
3. Check the signed build artifact and the TestFlight processing state in App Store Connect.
4. Add the processed build to an internal test group to install it on a device.

## Root commands

Install the root tools, run a frozen Bun install and `bundle install`, then use a macOS host
with the pinned Xcode selection:

```sh
export APP_VARIANT=development
export MOBILE_API_BASE_URL=https://api.example.com
export IOS_BUILD_NUMBER=1.1

just mobile-build-check
just mobile-check
just mobile-ios-prepare
just mobile-ios-build unsigned
```

`mobile-ios-prepare` writes the resolved public Expo configuration, builds the native SDK,
generates the iOS project and installs Pods. `mobile-ios-build signed` additionally requires
the readonly signing environment described in the maintenance guide. `mobile-ios-upload`
uploads the existing signed IPA using the Apple API key. Outputs live under `dist/mobile/ios`
and generated native projects remain untracked.

The build lane rejects a manifest whose variant or bundle identifier differs from the
selected environment, and a signed build requires an explicit backend URL. Raw Fastlane
build calls require `signing:unsigned` or `signing:signed`; the root command forwards this
option using Fastlane's colon syntax.

## Encryption declarations and France

The shared Rust client includes a TLS implementation through `reqwest` and `rustls`.
Its encryption is not limited to Apple's operating-system APIs. Distribution geography
and encryption declarations are managed in App Store Connect; the app variant alone does
not restrict which countries can receive a build.

Development currently targets internal testing outside France. Complete its export
questionnaire in App Store Connect according to that distribution. The generated app does
not declare an unverified exemption or inherit the production app's approval. Review the
declaration before extending development distribution to France.

Production is intended to include France. Complete the applicable French encryption
declaration and submit the required documentation for the production app record in App
Store Connect. After Apple approves the documentation, use its supplied code as
`IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE` when preparing the production app.
This is an approval reference embedded in the app, not an API credential. Do not invent a
code or reuse the development app's declaration.

With `APP_VARIANT=production` and that value present, Expo writes
`ITSAppUsesNonExemptEncryption=true` and `ITSEncryptionExportComplianceCode` into the native
Info.plist. Without it, configuration and archives remain available for preparation, but
the Fastlane upload lane rejects production uploads before contacting Apple. The guard
checks the prepared Expo configuration, so adding an environment variable after prebuild
does not qualify an older build. It verifies the presence of the reference, not Apple's
approval status or the contents of the signed IPA.

The future production workflow must pass this variable throughout configuration, prebuild
and archive steps. The current development workflow does not consume it. Neither this
configuration nor merging it submits documents, obtains approval, changes country
availability or modifies an already uploaded build.

## Production release path

The app configuration and Fastlane build/upload lanes also support the production variant.
A future production workflow must select its backend and signing material explicitly from
the chosen release revision. Test the resulting production build through TestFlight, then
select that same build for an App Store version and submit it to Apple review. Publishing
is a separate release decision; development builds cannot be promoted into production
because their bundle identifiers and service configuration differ.

App names must be available for their App Store localizations. When keeping separate Dev
and production app records, choose distinct names and reserve the intended public name for
the production record before release.

## References

- [Expo release builds with local native tools](https://docs.expo.dev/guides/local-app-production/)
- [Fastlane build_app](https://docs.fastlane.tools/actions/build_app/)
- [Fastlane TestFlight upload](https://docs.fastlane.tools/actions/testflight/)
- [Apple build submission](https://developer.apple.com/help/app-store-connect/manage-builds/choose-a-build-to-submit/)
- [Apple encryption documentation requirements](https://developer.apple.com/help/app-store-connect/reference/export-compliance-documentation-for-encryption/)
- [Apple export compliance code](https://developer.apple.com/documentation/bundleresources/information-property-list/itsencryptionexportcompliancecode)

## Signed archive qualification

Manual runs on a feature branch may use `upload: false` to qualify the real signing path
without sending a build to TestFlight or recording a release baseline. Uploads remain
restricted to the default branch. Pull requests validate unsigned archives.

The signed lane installs the current software-signing WWDR G3 intermediate in its temporary
keychain after readonly Match. Before archiving, it checks that a certificate in the selected
profile has a trusted signing identity and signs a disposable executable with that identity.
Trust-chain and private-key access failures therefore stop before the full Xcode archive.
This does not regenerate signing material or change system trust settings.
