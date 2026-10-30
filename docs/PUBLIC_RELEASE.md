# Release preparation and visibility

The current repository and release must remain PRIVATE. The owner will decide separately whether and when to make them public. No script or workflow in this repository changes visibility. A successful test is not permission to publish publicly.

## Source review

Use scripts/audit-release-source.py to generate an allowlisted source manifest. Review the exact staged index with --staged before committing. The heuristic audit rejects known token patterns, private key headers, operator paths and excluded data files; it complements human review rather than proving that no secret exists.

Never include artifacts, runtime databases, local/test data, SSH files, provider passwords, setup keys, owner-specific fixtures or private test reports. Website screenshots must use clearly labelled example data. Keep internal deployment evidence outside the published index.

## Build provenance

Create a reviewed commit with the authenticated repository owner's identity scoped to this repository. Build the Ubuntu artifact from that exact commit, including a freshly built web bundle. Run Rust unit tests and strict clippy, frontend unit/browser checks, API integration and Linux monitoring acceptance. Record the actual platform and any gaps.

Collect third-party licenses, then run package-linux-release.sh on Ubuntu 24.04 x86-64 with METRIC_OWL_SOURCE_COMMIT set to the reviewed 40-character commit and METRIC_OWL_RELEASE_VERSION set to the tag. The packager records GLIBC requirements, refuses to overwrite an existing version directory, emits SHA256SUMS and includes binary-use terms and dependency notices.

The manually triggered validation workflow is bounded to one Ubuntu job. It may upload short-lived private CI artifacts. It does not create releases, publish source, change visibility or install to an operator's server.

## Private release acceptance

Create a prerelease inside the private repository for the reviewed tag. Upload the versioned binary, bootstrap/install helpers, service unit, metadata, notices, licenses, archive and SHA256SUMS. Verify repository visibility remains private.

Use install-private-release.py from the authorized operator's computer. It obtains temporary GitHub asset URLs locally and passes them only through SSH stdin; do not copy a personal access token to the server. The server downloads directly from GitHub and validates every required asset and commit before installation.

Verify existing administrator credentials, settings, integration secrets and service drop-ins are preserved. Confirm loopback-only listening, the alternate-port SSH browser tunnel, scoped and revoked MCP client behavior, actual Linux measurements and unchanged existing websites. Measure monitoring overhead separately from builds and test startup; report observation duration and sample limitations.

Do not claim an email was delivered based only on a provider accepting it. Do not claim Mac checks validate Linux. Keep a private HTML validation report with screenshots, measured results and unresolved gaps.

## Later owner-controlled public launch

Only after a separate owner decision may visibility and public download instructions change. Re-audit source and release assets first. Test anonymous release downloads, checksums and installation before advertising a one-line installer. Public source visibility does not change the proprietary source license or grant open-source licensing rights.
