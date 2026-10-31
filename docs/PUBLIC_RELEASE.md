# Release preparation and visibility

The owner authorized public source and preview binaries for v0.1.0-preview.3, and
Apache License 2.0 for the free edition starting with v0.1.0-preview.8. Earlier
release tags and assets retain their published contents. Publication still requires
the source/asset audit and anonymous-download checks below. No script or workflow
changes repository visibility automatically.

## Current release target: v0.1.0-preview.10

This release includes BYOK Server AI text chat, six provider presets, ten scoped
read-only MCP tools with coverage-aware analysis, browser-session authentication,
root-only local administrator recovery, and ownership-checked preserve/purge
removal, plus branded multipart HTML/plain-text notification templates. It must use a freshly built application bundle; the installer-only
binary-reuse exception below does not apply. Preserve earlier versioned evidence
and release contents. New source or a preview screenshot is not a fresh-install
acceptance result.

## Source review

Use scripts/audit-release-source.py to generate an allowlisted source manifest. Review the exact staged index with --staged before committing. The heuristic audit rejects known token patterns, private key headers, operator paths and excluded data files; it complements human review rather than proving that no secret exists.

Never include artifacts, runtime databases, local/test data, SSH files, provider passwords, setup keys, owner-specific fixtures or private test reports. Website screenshots must use clearly labelled example data. Keep internal deployment evidence outside the published index.

## Build provenance

Create a reviewed commit with the authenticated repository owner's identity scoped to this repository. Build the Ubuntu artifact from that exact commit, including a freshly built web bundle. Run Rust unit tests and strict clippy, frontend unit/browser checks, API integration and Linux monitoring acceptance. Record the actual platform and any gaps.

Collect third-party licenses, then run package-linux-release.sh on Ubuntu 24.04 x86-64 with METRIC_OWL_SOURCE_COMMIT set to the reviewed 40-character commit and METRIC_OWL_RELEASE_VERSION set to the tag. The packager records static musl provenance, refuses to overwrite an existing version directory, emits SHA256SUMS and includes the Apache-2.0 license, distribution information and dependency notices.

The manually triggered source validation workflow is bounded to one Ubuntu job.
The separate published-installer workflow runs the actual public, version-pinned
command on a fresh disposable Ubuntu 24.04 VM, chooses private SSH/loopback access,
then verifies root recovery, data-preserving removal/reinstallation and explicit
purge. It is guarded against existing installations and never targets an operator
or customer host. It does not test new public-IP certificate issuance. Both
workflows may upload short-lived test artifacts; neither publishes a release or
changes repository visibility.

For an installer-only patch, an already published binary may be reused only when the complete runtime inputs (`crates/`, `web/`, Cargo manifests/lockfile, Rust toolchain and service packaging) are unchanged from its source commit. Verify the previous release checksum, record `binary_source_commit` and `binary_reused_from` separately from the new installer `source_commit`, and run the changed installer tests on Linux. Do not describe a reused binary as freshly rebuilt or claim a new end-to-end customer installation before it has passed.

## Preview acceptance before publication

Create a prerelease for the reviewed tag. Upload the versioned binary, bootstrap/install/HTTPS helpers, service unit, metadata, notices, licenses, archive and SHA256SUMS. Record the actual repository visibility and exact commit. Keep earlier release tags and assets immutable.

For a private staging release, use install-private-release.py from the authorized operator's computer. It obtains temporary GitHub asset URLs locally and passes them only through SSH stdin; do not copy a personal access token to the server. The server downloads directly from GitHub and validates every required asset and commit before installation.

Verify existing administrator credentials, settings, integration secrets and service drop-ins are preserved. Confirm loopback-only application listening, the alternate-port SSH fallback, protected HTTPS login and first-time setup, secure session cookies, scoped and revoked MCP client behavior, actual Linux measurements and unchanged existing websites. Measure monitoring overhead separately from builds and test startup; report observation duration and sample limitations.

Do not claim an email was delivered based only on a provider accepting it. Do not claim Mac checks validate Linux. Keep a private HTML validation report with screenshots, measured results and unresolved gaps.

## Public launch validation

The owner-authorized public launch must use the reviewed source commit and assets.
Re-audit source and release assets before publication. Test anonymous release
downloads, checksums and installation before advertising the one-command installer.
Verify that website copy matches the released platform, access mode and Apache-2.0
free-edition license. The per-host dashboard is a product capability, not a license
restriction on installing independent copies. Future Pro features belong in a
separate repository and must not change the license of this free edition.
