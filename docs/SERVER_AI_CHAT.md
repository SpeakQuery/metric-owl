# Server AI: a text conversation about this server

Open **Server AI** in the dashboard. It is a text-only conversation about the
single monitored server: health, performance, configured checks, incidents and
recorded history. Replies are concise and follow your question's **Turkish or
English** language. The product interface stays in English.

## Start a conversation

1. In **Notifications & connections → AI providers**, connect **Grok, OpenAI,
   Claude, Gemini, DeepSeek or Mistral** using your own API key. Grok is the
   default for a new installation. An existing verified connection is reused.
2. Open **Server AI**, type a question and send it. No MCP setup, second key,
   model/endpoint fields, uploads or audio are required.

Try:

- `How is this server doing?`
- `What changed in the last 20 minutes?`
- `Was CPU high in the last 3 hours?`
- `Son 20 dakikada sunucu nasıldı?`
- `Son 3 saatte OOM, swap veya disk sorunu var mı?`

Time ranges are understood from your text; the default context is the last
three hours. Relevant missing or partial history is shown explicitly. Dates and
peak intervals use readable **UTC** times. A recorded peak interval is not an
exact peak second, and UTC is not silently relabeled as your local time.

This screen uses the **provider saved on this server**. When Codex or another
AI client reads MCP tools, its own selected model does the reasoning instead;
an MCP read does not automatically invoke the server's Grok key.
[Connection distinction](AI_CONNECTIONS.md)

## Cost and conversation lifetime

Each explicit send can make one paid provider request. The panel shows the
connected provider, cost notice and remaining **20 provider attempts per UTC
day**, shared across sessions on this server. Failed provider attempts count;
local scope refusals do not. The limit resets at **00:00 UTC**. One AI operation
runs at a time. There is no automatic retry or background chat generation.

Monitoring and local rules continue without AI. Connecting or chatting does not
enable automatic event analysis. Optional incident explanations have a separate
10-request daily budget. A request cap is not a monetary spending cap; your
provider's API charges and account quota apply.

Conversation stays in the signed-in page session. It survives switching between
panel pages and clears on reload, logout or **New chat**. It is not saved to local
storage or a new conversation database. Limited completed turns are sent to the
provider to support follow-up questions; do not paste secrets into the conversation.

If a request fails, read the error and retry deliberately. A timed-out or interrupted
request may still have been counted by the provider. No missing answer is silently
retried on your behalf.

## What the assistant can establish

The assistant receives a bounded summary of this host's collected observations,
not open-ended access to the operating system. It may explain observed changes,
recorded incidents, configured monitor states and available process snapshots.
It should distinguish facts, possible explanations and unknowns.

- History exists only for observations actually collected and retained. A 60-day
  request limit does not mean 60 days of data exist.
- A current process list is not evidence of which process caused an earlier
  spike. Historical process snapshots are collected prospectively about once
  per minute, with seven-day retention; gaps and row limits remain explicit.
- CPU/RAM co-movement and statistical candidates do not establish root cause.
  Rolling five-minute OOM counts overlap and cannot be added as distinct kills.
- A configured website or backup/job check's latest state is not continuous
  historical uptime. Reported backup success does not prove restorability.
- A missing or disabled monitor is not a healthy check. Unavailable data and
  unmeasured time stay unknown.
- Questions outside this monitored server are refused. Chat has no command runner,
  arbitrary SQL, file/URL reader, external browser or server-changing tools.

Important findings remain labeled as AI interpretations requiring verification.
The fixed read-only data boundary is enforced in code; a system prompt alone is
not the authorization boundary.

## Operator reference

`POST /api/v1/ai/chat` accepts only `question` and optional `history` entries with
`question`/`answer` fields. The existing browser session, same-origin checks and
CSRF token are required. Clients cannot select a different server, endpoint,
model, database query, path, URL or tool. Questions are limited to 1,200 characters;
only up to six prior turns are admitted. Older conversation is dropped when
needed to preserve complete evidence and the current question within the input
bound.

The handler calls only the fixed `analysis::server_context(app, from, to)` builder.
It supplies sanitized identity/current overview, summarized metric history,
configured monitor states, data-health warnings, recent incidents, recorded process
context and a cautious CPU/RAM relationship summary. Settings, stored credentials,
raw log streams and process command lines are excluded. Omission metadata describes
context shortened to its bound; an omitted report is not proof that no measurement
ever existed.

The builder caps context at 24,000 bytes. The chat guard rejects context over
28,000 bytes after its known-path UTC timestamp adaptation, before a provider
attempt. The unredacted serialized input is kept within 56,000 bytes; provider
transport also bounds input and response size. The reply limit is 500 output
tokens, with a 120-word display cap and a target of 2–5 short sentences. Requests
have a 50-second provider-flow timeout; the browser stops waiting after 60 seconds.
These are implementation bounds, not a fixed monetary price or quality guarantee.

Numeric HTTP response-period values and MCP/analysis timestamps retain their
existing types. Only the chat copy's known timestamp paths become UTC labels;
metric values, units, durations and untrusted evidence strings are not rewritten.
The exact-peak-second-unknown flag is preserved.

A conservative local admission check saves paid calls for recognized unrelated,
prompt-override, credential, command and arbitrary-path/URL requests. The provider
has no tools. Both evidence strings and prior assistant answers are untrusted data,
not instructions or independently verified facts. Provider error bodies are not
exposed. Known-key echoes, credential patterns and email addresses are redacted.
Replies render as escaped plain text. Relevant missing/partial or omitted history
also produces a deterministic localized notice independent of the model's wording.

## Reproducible checks

From the repository root:

```sh
cargo test -p server-guard chat:: -- --nocapture
node web/tests/chat-ui.mjs
```

From `web`:

```sh
pnpm exec tsc --noEmit
```

The UI script serves current source through an isolated Vite instance and
intercepts APIs. It checks setup routing, text-only controls, CSRF, TR/EN text,
loading, cost/quota, manual retry, coverage notices, navigation/new chat, escaped
markup and dark/light/mobile states. It does not rebuild `web/dist`, use customer
credentials or make paid calls. Mocked tests establish application behavior, not
live response quality for every provider or deployment on another server.

[Provider setup](AI_CONNECTIONS.md) · [MCP](MCP_CONNECTIONS.md) · [Security](SECURITY.md) · [Operations](OPERATIONS.md)
