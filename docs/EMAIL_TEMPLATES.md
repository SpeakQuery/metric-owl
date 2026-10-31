# Transactional email templates

Metric Owl renders warning, critical, recovery and scheduled-summary emails with a
shared HTML layout and a plain-text alternative. Verification and connection-test
messages use the same visual system. Templates are in
`crates/core/src/email.rs`; direct SMTP and the optional relay use this renderer.

The English templates use inline styles, presentation tables, a 600-pixel maximum
width and system fonts. They load no external images/fonts, scripts, forms or
tracking resources. Status is expressed in words as well as color. Critical/warning severity wins over
custom titles that happen to match a report or test name. Only the explicit
structured `kind: recovery` overrides active critical/warning severity; existing
queued records without an explicit kind retain their structured `resolved: true`
recovery status. Titles alone never indicate recovery. The relay's
Postmark transport explicitly disables open and link tracking; its
[official email API](https://postmarkapp.com/developer/api/email-api) accepts
`TextBody` and `HtmlBody` together.

## Content and links

- Notifications include the bounded/redacted server alias, status, title, recorded
  UTC time, observed condition/evidence and suggested next checks. Recovery is
  identified separately from an active critical/warning condition.
- Scheduled summaries distinguish the latest collected CPU/memory sample from
  period averages. Missing or invalid values display “Not available”; no zero or
  healthy result is invented. The sample time is shown independently from the
  email generation time.
- Only allowlisted structured fields are used. Arbitrary HTML, raw log fields,
  credentials and unknown payload fields are ignored. Dynamic text is escaped;
  common credential patterns are redacted and fields are bounded.
- The dashboard button is optional for direct SMTP. The agent supplies its configured validated
  HTTPS origin from `WebAccess`; payload text cannot override it. The shared
  renderer rejects credentials, paths, query strings, fragments, control
  characters, noncanonical URLs and loopback destinations. A private/local-only
  installation gets a written instruction to open its known dashboard, with no
  invented localhost/public URL. The relay omits the button entirely because an
  authenticated device payload does not verify ownership of a panel origin. A
  `panel_origin` value in a relay notification is ignored.

Direct SMTP emits MIME `multipart/alternative` with text and HTML parts. Relay
SMTP does the same and still accepts previously queued plain-text-only messages.
Relay Postmark submission includes both parts. HTML bodies are limited to 64 KiB;
plain text remains limited to 16 KiB.

## Compatibility and delivery

The agent→relay API retains `id`, `recipient` and `notification`. New structured
fields are inside the existing notification object, already represented as
`serde_json::Value`. An older relay continues composing its original plain-text
message, ignoring those fields; upgrade the relay to receive the shared HTML
templates. No raw HTML is accepted from an agent notification request.

Recipient verification, single-recipient SMTP validation, TLS, quotas, dedupe,
retry and provider-delivery status behavior are unchanged. The six-digit
verification challenge retains its existing plain-text subject/prefix/expiry
contract; the code is not exposed by API responses or log messages.

Provider acceptance is separate from inbox delivery. The automated checks below
do not send a real email or verify Gmail/Outlook rendering. Button-bearing fixtures illustrate direct SMTP with a configured HTTPS origin.
Relay messages omit that button. Browser previews check responsive layout; actual mail-client delivery remains an operator acceptance
step using a configured and verified recipient.

## Synthetic previews and checks

From the repository root:

```sh
cargo test -p guard-core email::
cargo test -p server-guard smtp::
cargo test -p guard-relay
node web/tests/email-templates.mjs
```

The preview script runs the actual Rust renderer with synthetic fixture data,
writes HTML/plain-text files to ignored `artifacts/validation/email-templates/`,
then checks six templates at 600, 390 and 320 pixels. It checks for overflow,
unexpected active elements, destination links and external requests. Screenshots
are saved in ignored `artifacts/screenshots/email-templates/`.

To render files without opening a browser or sending email:

```sh
cargo run --quiet -p guard-core --example email-preview
```
