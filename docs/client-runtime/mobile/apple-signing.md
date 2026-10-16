# Apple signing maintenance

Apple signing uses Fastlane Match and a private Git repository containing encrypted
certificates and provisioning profiles. GitHub-hosted macOS runs the manual maintenance
workflow. Normal application checks and source builds do not require signing credentials.

This workflow configures signing only. It does not compile the mobile application, create
an App Store Connect app record, upload a build, publish a store release, or configure OTA
delivery. App Store Connect app records are required separately before TestFlight uploads.

## Identities and distribution

| Variant | Bundle identifier | Registered name |
| --- | --- | --- |
| Development | `app.swarmie.mobile.dev` | Swarmie Dev |
| Production | `app.swarmie.mobile` | Swarmie |

The existing Expo configuration still uses the production identifier. Selecting development
in signing maintenance does not change the application's build configuration. A subsequent
build workflow must select the matching identifier, backend and callback scheme together.

The `appstore` profile supports both TestFlight and App Store submission. A development app
can use an App Store profile while connecting to development services: the app environment
and signing profile are independent choices. The `development` and `adhoc` profiles require
enabled iOS devices registered in the Apple Developer account.

## GitHub configuration

Set these repository variables in the application repository:

| Variable | Value |
| --- | --- |
| `APPLE_TEAM_ID` | Apple Developer membership Team ID |
| `MATCH_GIT_URL` | SSH URL of the private signing repository |
| `MATCH_GIT_BRANCH` | Optional signing branch; defaults to `main` |

Set these repository secrets:

| Secret | Purpose |
| --- | --- |
| `ASC_SIGNING_API_KEY_ID` | App Store Connect team API key ID |
| `ASC_SIGNING_API_ISSUER_ID` | Issuer UUID for that team key |
| `ASC_SIGNING_API_KEY_P8` | Complete multiline Apple private key, including PEM boundaries |
| `MATCH_PASSWORD` | Encryption password for the signing repository |
| `MATCH_SSH_PRIVATE_KEY` | Read-only deploy key for the signing repository |
| `MATCH_MAINTENANCE_SSH_PRIVATE_KEY` | Read/write deploy key used only for synchronization |

Use a team API key with the permissions required to create distribution certificates and
profiles. An Individual Apple Developer membership and an individual API key are different
concepts; individual API keys cannot use provisioning endpoints. Reserve the administrative
API key for maintenance; ordinary builds should fetch existing Match material in readonly
mode without receiving that key. Keep the Match password in a separate password manager too.

The SSH private keys authenticate access to Git storage; the Match password decrypts its
contents; the Apple API key authenticates requests to Apple. These credentials have distinct
responsibilities. GitHub's per-job `GITHUB_TOKEN` does not grant cross-repository Git access.

Several applications in the same Apple team can share certificates in one signing branch,
while retaining separate provisioning profiles per bundle identifier. Use separate branches
for distinct Apple teams and serialize synchronization across callers sharing a branch.
Reading that branch and knowing its password grants access to its shared signing material.

## First execution

After the workflow is present on the default branch:

1. Open Actions, select **Apple signing maintenance**, and choose **Run workflow**.
2. Select the default branch, `operation: inspect`, `variant: development`, and
   `profile: appstore`.
3. Check that the workflow can read the configured signing branch, decrypt all existing
   signing files and access Apple's provisioning API. An empty repository or branch reports
   uninitialized storage: its password cannot be verified until signing material exists.
   A missing branch in a nonempty repository is an error. A missing bundle identifier is
   expected on a fresh account. Inspection does not register it or create certificates.
   The API key and password are never printed.
4. Run the workflow again with `operation: sync` and the same variant and profile.
5. Confirm the run succeeds and encrypted certificates/profiles appear in signing storage.
   The lane makes a fresh readonly Match call after synchronization to verify retrieval.

Both operations verify existing storage before contacting Apple. A missing branch, incorrect
password or decryption failure stops synchronization before it can register an identifier.
Preflight clones are removed on both success and failure without pushing changes. Successful
decryption checks storage access and encryption; it does not qualify certificates or profiles
for the selected application, which Match checks during synchronization.

Synchronization reuses an existing bundle identifier and its capabilities, or registers only
the selected missing identifier. It creates or fetches the selected signing material and
stores changes encrypted. It does not revoke existing certificates, run `match nuke`, or
change the other app variant. Existing Apple certificates whose private keys are not in
Match storage may require an explicit import before reuse.

Maintenance only runs on manual dispatch from the repository's default branch. Pull requests
run credential-free tooling tests on Linux. There is no automatic certificate generation on
push or pull request. Production is an explicit variant selection; do not reuse its profiles
for development builds with a different identifier.

## Root commands and implementation

The root `mise.toml` pins Ruby and Just; `Gemfile.lock` pins Fastlane and its dependencies.
Install the root Ruby dependencies with `bundle install`, then run:

```sh
just mobile-signing-check
just mobile-signing inspect development appstore
just mobile-signing sync development appstore
```

Maintenance requires macOS with Xcode and the environment variables above, plus
`MATCH_GIT_PRIVATE_KEY` pointing to the selected SSH private-key file. GitHub Actions prepares
that file from its selected secret, verifies GitHub's SSH host keys using the HTTPS metadata
endpoint authenticated with the automatic job token, and removes temporary SSH files after
the job. Metadata retrieval does not require another repository secret. Match uses a
temporary CI keychain.

`fastlane/apple_signing.rb` validates operation, variant and credentials before network work.
It also overrides Match's storage commit-message hook so generated commits remain lowercase
Conventional Commits. Match's root README generation is disabled; project documentation
remains under `docs/`. No private signing material belongs in the source repository.

## References

- [Fastlane Match](https://docs.fastlane.tools/actions/match/)
- [Fastlane App Store Connect API authentication](https://docs.fastlane.tools/app-store-connect-api/)
- [Fastlane CI keychain setup](https://docs.fastlane.tools/actions/setup_ci/)
- [GitHub deploy keys](https://docs.github.com/en/authentication/connecting-to-github-with-ssh/managing-deploy-keys)
