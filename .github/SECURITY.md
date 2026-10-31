# Security reports

Please email services@metricowl.cloud with a concise reproduction and affected version. Do not include passwords, setup keys, recovery codes, API tokens, private SSH keys or unredacted server logs. Use synthetic examples where possible.

The project is a public preview. The application listener remains loopback-only.
The guided installer can configure protected public-IP HTTPS; private access uses
loopback/SSH. Browser access requires administrator authentication, with MFA when
enabled. Do not expose port 4877 directly or bypass Host/Origin validation.
Browser sessions have explicit revocation, not fixed idle/step-up timeouts.

MCP is a ten-tool, single-host read interface over local/SSH stdio, not HTTP or an
execution agent. The calling client's model interprets its evidence. Panel Server
AI uses the server's separate BYOK connection and fixed bounded read-only context.
Root-only administrator recovery and explicit purge are operator actions, not
remote reset features. See the [security model](../docs/SECURITY.md) and
[operations runbook](../docs/OPERATIONS.md). This contact does not promise a
response time or support service level.
