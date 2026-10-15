# Signed macOS releases

The **Release selected tag** workflow builds a standalone app, signs it with your Developer ID Application certificate, submits it to Apple's notary service, staples the accepted ticket to the app, then packages it in a signed, notarized, and stapled drag-to-Applications DMG. A failed or incomplete signing/notarization step prevents the entire release from publishing. There is no unsigned-release fallback.

The app currently supports **Apple Silicon Macs with macOS 13 or later**. Recipients download the `ac-man-app-<tag>-macos-apple-silicon.dmg` asset, open it, drag **ac-man.app** to Applications, and open it. They sign in with their own SmartHome account and need no Rust, Swift, or Python installation. Intel macOS currently has a CLI release only.

## One-time Apple setup

You need an Apple Developer Program membership, a **Developer ID Application** signing identity with its private key, and a **team App Store Connect API key** for notarization. The certificate identifies the app's publisher; the API key authorizes submission to Apple's notary service. [Apple's Developer ID guide](https://developer.apple.com/developer-id/) explains certificate creation and distribution.

1. Create or locate your Developer ID Application certificate in Xcode or the Apple Developer portal. In Keychain Access, find it under **My Certificates**, expand it to confirm its private key is present, and export that identity as a password-protected `.p12`. A `.cer` file alone is insufficient. Export only the one signing identity. See [Apple's signing-identity guidance](https://developer.apple.com/documentation/xcode/sharing-your-teams-signing-certificates).
2. In App Store Connect, create a **team API key** and select **Developer** under Access; this role permits notarization. You can name it `ac-man releases`. Record its **Key ID** and **Issuer ID**, then download its `.p8` private key and keep it securely; Apple allows only one download. This workflow explicitly uses team-key authentication with an issuer ID. See [Apple's role permissions](https://developer.apple.com/help/account/access/roles/) and [API-key guide](https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api).
3. Keep the `.p12`, its password, and the `.p8` outside this repository. They belong in GitHub environment secrets, never commits, issues, or chat messages.

## Configure GitHub

In the repository, go to **Settings → Environments → New environment** and name it exactly **`macos-release`**. Restrict deployment branches to your trusted default branch, and use required reviewers when available. Run the release workflow from that branch; its separate `tag` input selects the source commit to build.

Add these **environment secrets**:

| Secret | Value |
|---|---|
| `MACOS_CERTIFICATE_P12` | Base64-encoded contents of the exported `.p12`, including the private key |
| `MACOS_CERTIFICATE_PASSWORD` | The password chosen when exporting that `.p12` |
| `APPLE_API_PRIVATE_KEY` | Complete text of the `.p8` file, including its BEGIN/END lines and line breaks |

Add these **environment variables** in the same environment:

| Variable | Value |
|---|---|
| `APPLE_API_KEY_ID` | The team API key's Key ID |
| `APPLE_API_ISSUER_ID` | The team API key's Issuer ID UUID; this is not the Developer Team ID |

To copy the P12's base64 value on macOS without printing it in the terminal:

```sh
base64 -i /path/to/DeveloperID.p12 | pbcopy
```

Paste that into `MACOS_CERTIFICATE_P12`. Paste the P12 password directly into its secret field, and the `.p8` file's contents directly into `APPLE_API_PRIVATE_KEY`. The workflow creates its own temporary keychain password, so no additional keychain-password secret is required. GitHub documents this approach in [signing on macOS runners](https://docs.github.com/en/actions/how-tos/deploy/deploy-to-third-party-platforms/sign-xcode-applications).

## Publish a version

1. Push the completed source and release workflow to GitHub.
2. Write the feature notes for the version, then create and push a signed annotated tag. The workflow publishes the tag annotation as the release notes. For the first release:

   ```sh
   git tag -s v0.0.1 --cleanup=verbatim -F docs/releases/v0.0.1.md
   git push origin v0.0.1
   ```

3. Choose **Actions → Release selected tag → Run workflow**. Select the trusted default branch for the workflow and enter the existing tag, such as `v0.0.1`, in the `tag` input.
4. Approve the `macos-release` environment when its protection rules request review.

The reusable CI workflow checks the tagged commit and builds the app once without signing credentials. It verifies and packages that exact build, then uploads the unsigned archive with a one-day retention. Standalone CI uses the commit SHA as its archive label; release calls supply the selected tag. After every verification job succeeds, a fresh signing job downloads the archive from the same workflow run. That job imports your P12 into a temporary keychain, selects its single valid Developer ID Application identity, and signs with a secure timestamp and Hardened Runtime. It submits the signed app using `notarytool`, requires an **Accepted** result, staples and validates the ticket, and runs Gatekeeper assessment. It then creates a DMG containing the stapled app and an Applications shortcut, signs and notarizes the disk image, staples its ticket, and verifies it with Gatekeeper. The packaging scripts come from the trusted workflow revision, and their Python dependencies are pinned by version and SHA-256 before signing secrets are used. The selected tag’s code is never executed in the signing job. See [Apple's notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).

Only the final publish job can create the GitHub release. It receives the completed signed app and CLI artifacts, computes `SHA256SUMS`, records the source commit, and checks that the tag has not moved. The unsigned intermediate app is not a release asset. The separate CLI archives are outside the app's notarization flow.

## If a release fails

- **Missing configuration:** check the exact environment, secret, and variable names above. Repository settings do not travel with a clone.
- **No valid signing identity:** export the certificate together with its private key, ensure it is unexpired, and use a Developer ID Application identity rather than an Apple Development or Mac App Distribution identity.
- **Notarization authentication fails:** verify the team API key, its issuer, and your active Apple Developer membership and agreements.
- **Apple rejects the app:** inspect the signing job's notarization result and Apple's submission log before retrying. Publication stays blocked.
- **Notarization exceeds 20 minutes:** the run fails without publishing; Apple may continue processing its submission. Re-run only after checking the submission's status.
- **An existing release has the same tag:** the workflow will not overwrite it. Choose a new version tag for a new release.

Local `make app` builds remain ad hoc signed for development. Running it does not use your Apple credentials or notarize the result. Do not rebuild or modify an app after signing/notarizing it; changes invalidate the signature or ticket relationship.

## Local installer preview

On macOS, after `make app` (or with an existing signed app), install the isolated packaging tools and create an installer:

```sh
python3 -m venv .venv/dmg
.venv/dmg/bin/pip install --require-hashes --only-binary=:all: -r scripts/dmg/requirements.txt
DMGBUILD="$PWD/.venv/dmg/bin/dmgbuild" ./scripts/package-dmg.sh "$PWD/dist/ac-man.app" "$PWD/release-assets/ac-man-preview.dmg"
```

The packager preserves the input app and its signature. This preview command does not sign or notarize the DMG; the release workflow performs those steps. The disk image has a compact Finder window with the app on the left, an arrow, and the Applications shortcut on the right. Open the image, drag the app to Applications, then eject the image.
