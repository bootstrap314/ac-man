# Security

## Scope and reporting

Security fixes target the current Rust implementation and its Swift and CLI interfaces.

Report suspected vulnerabilities privately to the project owner. No public reporting address or hosted repository has been configured yet; do not include passwords, session tokens, Keychain exports, or household device details in a public issue. A useful report includes the affected revision, platform, reproduction steps using synthetic data, and expected versus actual behavior.

When publishing the repository, enable GitHub private vulnerability reporting and update this section with the reporting link. Require the PR test and security checks through branch protection. Those repository settings are separate from the workflow files.

## Accounts and local data

- Run `ac-control login` and enter the password at its hidden terminal prompt. There is no password command-line flag. Do not put passwords in shell history, environment variables, Docker build arguments, examples, or CI secrets for this project.
- Saved credentials and cloud sessions use macOS Keychain or the Linux user's unlocked Secret Service keyring. There is no plaintext credential-file fallback. Linux credential storage needs a running user-session D-Bus and Secret Service implementation; a minimal Docker container normally has neither.
- An in-memory sign-in lasts only for the process that owns it. Saving a login lets later CLI invocations reuse the OS credential store. Removing the saved login does not necessarily revoke a session on Midea's servers; use SmartHome's account settings when account access must be revoked.
- The user's application-data directory holds the discovered device inventory, labels, assigned integer IDs, a stable client identifier, and status information. New data files use owner-only permissions on Unix. This data is not a password store, but room names, device IDs, MAC addresses, and status can reveal household details. Keep the directory on a private local filesystem and exclude it from shared archives. Source and release bundles must not contain a developer's household inventory or private paths.
- Error messages and device snapshots must not contain authentication headers, passwords, encryption keys, session tokens, or complete cloud responses. Do not enable request-body logging while troubleshooting a real account.

For a shared computer, use a separate OS account and lock the session when unattended. If SmartHome permits sharing selected ACs with a dedicated account, that can limit the account access available to this controller. The controller discovers ACs from the signed-in account; device IDs are not authorization credentials. A local label or assigned integer ID must resolve to an AC accessible to the current account before a cloud command is sent.

## Network and command boundaries

Cloud requests use certificate-validated HTTPS on port 443. Region URLs are restricted to subdomains of `appsmb.com` or `smartmidea.net`; redirects are disabled so an authenticated request is not silently forwarded elsewhere. The controller does not start a network server or require an inbound firewall exception. The macOS client can retry a failed internet connection through Wi-Fi; this remains a cloud connection.

Temperature, mode, fan percentage, swing, and Boost changes are checked against the AC's reported capabilities. Replies are parsed with length and checksum checks, and requested changes are checked by reading state back. A transport timeout can occur after the AC accepted a command: check the current state before repeating an operation whose outcome is uncertain.

This is an unofficial client for a community-documented SmartHome protocol. Vendor-required password hashes, signatures, and payload encryption are compatibility mechanisms, not a new cryptographic design or a substitute for TLS. Midea service changes can break authentication or device control. The app is intended for ordinary comfort controls, not safety-critical automation.

## Dependencies, containers, and CI

Commit `Cargo.lock` and build with `--locked`. Review dependency and tool updates together with their lockfile changes. The local checks are:

```sh
make test
make audit       # cargo audit --file Cargo.lock: RustSec advisories, including CVEs
make trivy      # Trivy filesystem dependency, secret, and configuration checks
make security   # both vulnerability checks
```

Install `cargo-audit` and Trivy separately before running these targets; the workflow pins its tool versions. Cargo Audit checks the Rust dependency graph against RustSec. Trivy covers additional dependency, secret, and Docker configuration issues. The scanners need current advisory databases and fail on scan errors rather than treating an unavailable database as a clean result. Review findings and update affected dependencies; do not add blanket ignores to make a release pass.

The PR workflow uses read-only repository permissions, pinned actions, and no saved checkout credentials. It tests without access to a real SmartHome account. The manually dispatched release workflow resolves an existing tag, tests and scans that revision, and publishes checksummed archives in a separate job that alone has release-write permission. A checksum detects file corruption; it is not a publisher signature. The prepared macOS app is ad hoc signed, not Developer ID signed or notarized.

Docker is primarily a reproducible Linux CLI build environment. Its context excludes local data and credentials; the optional runtime uses a non-root user. Do not copy a host keyring or password into an image. The CI image scan covers the resulting runtime image as well as source files. Review base-image updates and rescan them alongside dependency updates.

Passing these checks establishes the results of those checks, not the absence of all vulnerabilities. Preserve the account, destination, input-validation, and redaction boundaries when adding features.
