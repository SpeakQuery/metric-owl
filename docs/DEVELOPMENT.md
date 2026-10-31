# Development

Pinned tools: Rust **1.99.1**, Node.js **22.23.4**, pnpm **10.32.1**. End users of the binary do not need Rust, Node.js or a separate SQLite installation.

```sh
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo test --workspace --locked
pnpm --dir web test
pnpm --dir web build
cargo build --workspace --locked
python3 scripts/integration-test.py
pnpm --dir web e2e
node web/tests/email-ui.mjs
node web/tests/sources-ui.mjs
```

The first frontend build supplies the embedded assets for a clean checkout. Rust tests regenerate the shared TypeScript contracts; the final frontend build uses those exact types. Do not edit `web/src/generated` by hand. `scripts/build.sh` creates a local release and collects dependency licenses.

- [Contributing under Apache License 2.0](../CONTRIBUTING.md)
- [Architecture](ARCHITECTURE.md)
- [Security](SECURITY.md)
- [Notifications](NOTIFICATIONS.md)
- [Dependencies and licenses](DEPENDENCIES.md)
- [Release process and visibility gate](PUBLIC_RELEASE.md)

