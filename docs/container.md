# CLI container package

The selected-tag release workflow builds the CLI runtime natively for **Linux amd64 and arm64** and publishes one multi-architecture image:

```sh
docker run --rm ghcr.io/bootstrap314/ac-man:v0.0.4 --help
```

Use the version of a completed release; the example tag becomes available only after that release's package job succeeds. Docker automatically selects the matching architecture, including arm64 on Apple Silicon Docker Desktop. This package contains the **CLI only**, not the macOS menu-bar application. There is deliberately no moving `latest` tag; choose a release tag or immutable digest.

## Authentication and storage

The runtime is a minimal, shell-free, non-root image (UID/GID 10001). It uses `/data` for device names and other application data. A named volume preserves that data:

```sh
docker run --rm -v ac-man-data:/data ghcr.io/bootstrap314/ac-man:v0.0.4 --help
```

**A volume alone does not enable cloud login.** Linux CLI authentication uses an unlocked Secret Service keyring over a user-session D-Bus. The image does not contain a keyring daemon, and it cannot access macOS Keychain through Docker Desktop. Consequently, the stock container supports help and local operations but cloud login/control requires deliberately providing a compatible Secret Service session. No plaintext credential fallback is included. For normal desktop use, install the native CLI and menu-bar app with Homebrew instead. Do not bake account passwords or saved sessions into an image.

## Release process

Run **Release selected tag** from the default branch and specify an existing release tag. The workflow:

1. Resolves the tag once and runs the existing release checks.
2. Builds each Linux architecture on its own native runner, includes dependency license notices, and smoke-tests `--help` and `--version` with networking disabled.
3. Downloads current Trivy vulnerability definitions into a fresh job-local cache and blocks publication on High/Critical vulnerabilities or secrets.
4. Transfers checked image archives to a separate publishing job. That runner executes no project code and alone receives `packages: write`.
5. Publishes the versioned multi-architecture manifest to GHCR after the GitHub release succeeds, verifying it contains exactly Linux amd64 and arm64. Run-specific platform tags are retained as backing images; users should pull the release tag.

If package publication fails after the GitHub release is created, rerun the failed package job. Do not recreate the already-published release. Workflow run summaries include the pull command.

The first GHCR package is private by default, even for a public repository. After the first successful publish, a repository/package administrator must open **Packages → ac-man → Package settings → Change visibility → Public** to allow anonymous pulls. The image's source label links the package to this repository. If a package with that name already exists, grant this repository Actions access to it. No personal access token is needed for publishing from this workflow.

References: [GitHub Container registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry), [Docker manifest commands](https://docs.docker.com/reference/cli/docker/manifest/).

## Verify build provenance

The release publisher uses `actions/attest` to sign the exact multi-architecture manifest digest with a short-lived Sigstore certificate. The attestation is recorded in GitHub's Attestations page and attached to the image in GHCR. It identifies the trusted release workflow; the selected source commit is also recorded in each platform image's `org.opencontainers.image.revision` label. This uses GitHub Actions identity, not a personal GPG private key.

After publication, verify the image with GitHub CLI (replace the digest with the published SHA-256 digest):

```sh
gh attestation verify oci://ghcr.io/bootstrap314/ac-man@sha256:DIGEST --repo bootstrap314/ac-man
```

Attestation failures fail the package job, even if the image has already been pushed. Rerun the failed job to complete publication and attestation.
