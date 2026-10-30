# Notification relay

End users do not need an SMTP account. An operator sets up the relay and sending
domain once. The agent sends only a server alias, incident summary, selected metrics,
and a recommendation. Incident email cannot be sent to unverified recipients.
The allowance is 60 messages per device per day; retries of the same message do
not spend the allowance again. Exhausted quotas defer messages until the next UTC
day. Verification email is limited to five per device per day; a code expires after
10 minutes and permits at most five attempts.

## Local development (no external email)

```sh
./target/debug/guard-relay --data-dir .test-data/relay provision --name local-test
./target/debug/guard-relay --data-dir .test-data/relay serve --local-test
GUARD_ALLOW_LOCAL_RELAY=1 ./target/debug/server-guard --data-dir .local-data serve
```

The relay writes the device key into `device-<id>.key`. Enter
`http://127.0.0.1:4878` and that key in the dashboard, then test the connection.
Messages are written to `.test-data/relay/test-mailbox.jsonl`; read the verification
code there. This file is for local tests only. Production UI responses never contain
the verification code.

## SMTP submission through an existing provider

The relay also supports authenticated SMTP. It connects to an external email
provider; it does not install an MTA, open an inbound mail port, or require mail
software on the monitored server. Choose `RELAY_PROVIDER=smtp`. The only supported
combinations are `SMTP_SECURITY=tls` with port 465 or `starttls` with port 587.
TLS certificate/hostname validation is mandatory; STARTTLS cannot downgrade to
plaintext.

Example operator configuration (contains no password):

```ini
RELAY_PROVIDER=smtp
SMTP_HOST=smtp.hostinger.com
SMTP_PORT=465
SMTP_SECURITY=tls
SMTP_USERNAME=services@metricowl.cloud
SMTP_FROM="Metric Owl <services@metricowl.cloud>"
```

`SMTP_PASSWORD_FILE` must point to an absolute, regular private file (mode 0600
or 0400), at most 4 KiB. Symlinks and group/other-readable files are rejected.
Never place the password in the repository, a public web directory, command-line
arguments, or a shared environment file. The supplied systemd service uses
`LoadCredential` to pass a root-owned secret to its dedicated service account.
For systemd's private `/run/credentials` mount only, root:root 0440 credentials
with systemd's service-specific ACL are also accepted. This does not permit
ordinary group-readable password files.
Its `/etc/metric-owl/relay.env` contains only the settings above; the password
source is `/etc/metric-owl/relay-smtp-password`.

With the same private environment and credential available, run:

```sh
guard-relay smtp-check
```

This verifies TLS, authentication and SMTP NOOP, then disconnects without sending
a message or opening the database. Success is not evidence of inbox delivery.
Both SMTP and Postmark preserve device authentication, recipient verification,
durable queues and quotas. Provider errors shown in the queue are sanitized and
never include authentication transcripts.

## Operator production requirements

- An authenticated SMTP provider as above, **or** a Postmark-verified sending
  domain, `POSTMARK_SERVER_TOKEN`, and `POSTMARK_FROM` (default provider).
- A TLS reverse proxy in front of loopback port 4878, with infrastructure-level
  access controls and request limits.
- Separate device credentials, secure delivery, and revocation procedures.
- Delivery, bounce, and outage tests with a real mailbox.
- Privacy and retention policies, account deletion/revocation procedures, and
  operational alerting.

The relay must be provisioned separately from the public marketing website.
`provision` is an operator tool; never embed a shared credential in every binary.

## Delivery states

`pending` means queued locally; `relay_queued` means queued at the relay.
`provider_accepted` means the email provider accepted the HTTP or SMTP submission, not proof of inbox
delivery. `test_delivered` means written only to the local test mailbox. `failed`
means the final attempt failed. The agent reconciles relay status and exposes provider
failures in the dashboard. A webhook's `accepted` state means its destination returned
2xx. A real delivery/bounce webhook adapter must be completed before release.

Scheduled report keys combine timezone, local date, and report time. During the
fall-back daylight-saving transition, a repeated local time produces one report.
A nonexistent time during spring-forward is skipped. Valid missed report times are
recovered after interruptions of up to 24 hours.

A disconnected email API request can leave the provider's acceptance uncertain.
Local idempotency reduces duplicates, but **exactly-once delivery is not guaranteed**,
including when a process crashes after provider acceptance.
