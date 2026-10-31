# Security reports

Please email services@metricowl.cloud with a concise reproduction and affected version. Do not include passwords, setup keys, recovery codes, API tokens, private SSH keys or unredacted server logs. Use synthetic examples where possible.

The project is a public preview. The application listener remains loopback-only. Browser access from other devices requires the explicit HTTPS reverse-proxy configuration, a private proxy credential and administrator authentication and optional MFA. Do not expose port 4877 directly or bypass Host/Origin validation. MCP remains local or SSH stdio; it is not a public HTTP endpoint. See docs/SECURITY.md for the supported boundaries. This contact does not promise a response time or support service level.
