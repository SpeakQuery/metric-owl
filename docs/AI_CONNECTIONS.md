# Connect an AI provider

Use this connection for the dashboard's **Server AI** text chat and optional
incident explanations. Basic monitoring and local alert rules work without it.
An external MCP client is a separate connection; see [MCP connections](MCP_CONNECTIONS.md).

**Which AI does the reasoning?** In panel Server AI and optional incident
analysis, the provider connected here answers using this server's saved key.
Through MCP, Metric Owl only returns data: the model selected in the calling
Codex/AI client interprets it using that client's own account and billing.
MCP data reads do not automatically call the server's Grok connection. The MCP
read key and this provider API key have different purposes and permissions.

## Connect once

1. Open **Notifications & connections → AI providers**.
2. Choose **xAI (Grok)**, **OpenAI**, **Anthropic (Claude)**, **Google Gemini**,
   **DeepSeek** or **Mistral AI**. Grok is the default for a new installation.
3. Paste that provider's API key and select **Connect**. Use a key belonging to
   the selected provider; a ChatGPT or other consumer subscription is not this key.
4. Wait for **Connected**, then open **Server AI** or an event's
   **Analyze evidence with AI** action.

The app chooses the endpoint, model and reasoning settings. There are no model or
API-URL fields to fill in. An already verified connection is reused by chat; it
needs neither another key nor an MCP setup. Changing provider requires that
provider's own key. Existing saved connections are not silently moved to a vendor.

Connect makes one short synthetic generation to check authentication and model
access. This can incur a small provider charge, but sends no monitoring evidence.
Only a readable successful response allows the encrypted key and settings to be
saved together. Failure preserves the previous connection. Saving or checking a
connection does not enable automatic AI.

## Choose how to use AI

| Action | What is sent | Limit |
| --- | --- | --- |
| **Connect / Check connection** | Short synthetic test | 20 checks per UTC day |
| **Server AI** | Your question, bounded monitoring summary and limited conversation | 20 provider attempts per UTC day |
| **Analyze evidence with AI** on an event | Bounded, redacted event evidence | Separate 10-request daily analysis budget; cached results can be reused |
| Optional automatic event explanations | Eligible, unexplained important-event evidence | Shares the event-analysis budget; off by default |

Chat answers in the question's Turkish or English language and stays within this
monitored server. Event explanations use the product's English analysis format.
Chat has no background generation or automatic retry. Failed provider attempts
count toward its daily limit; local scope refusals do not. The panel shows
remaining chat attempts. [Server AI behavior](SERVER_AI_CHAT.md)

Ordinary CPU, memory and disk alerts are detected locally; they do not require an
AI request. Automatic explanations can be enabled separately after connecting,
for eligible important events. AI cannot execute commands, inspect arbitrary
files/URLs or change this server.

## Cost, privacy and failure states

API charges are billed by the provider to the key owner, separately from this
open-source application. A daily request cap is not a monetary spending cap.
Set an account budget or quota with your provider if you need a monetary limit.
Check that provider's current pricing and regional/account terms before connecting;
Metric Owl does not guarantee a fixed per-question price.

The key is encrypted on this server and never returned to the browser. Authenticated
sessions, same-origin checks and CSRF protect connection changes; the UI does not
require repeated timed password/MFA prompts during an active session. Evidence
selected for AI leaves the server for the provider you choose. Redaction and size
limits reduce exposure; provider retention and use policies remain that provider's.
Never paste passwords, access tokens or private keys into a question.

A provider can reject a key, retire a model, exhaust its quota or require credit.
The UI reports a safe error without exposing the provider's raw response body.
Check the saved connection, account access and quota, then retry deliberately.
The app does not silently upgrade to a more expensive fallback model.

## Operator reference: presets in this source

These are internal code presets, not promises about a provider's future catalog,
prices or access for every account. A saved legacy connection may retain its model.

| Provider choice | Current code preset |
| --- | --- |
| xAI (Grok), default | `grok-4.3` |
| OpenAI | `gpt-6-luna` |
| Anthropic (Claude) | `claude-haiku-5-5` |
| Google Gemini | `gemini-3.5-flash-lite` |
| DeepSeek | `deepseek-flash` |
| Mistral AI | `mistral-small-2603` |

OpenAI-compatible, Anthropic and Gemini transports are implemented separately.
Preset reasoning controls keep output bounded; they are not user setup fields.
Older saved Groq, OpenRouter, NVIDIA and SiliconFlow connections remain recognized,
but are not offered as extra choices. Legacy/custom settings remain intact until
the operator explicitly connects one of the six choices. An unsupported custom
endpoint is not admitted to the bounded chat path.

Automated checks cover protocol payloads, key isolation, atomic save, failure
preservation, auth/CSRF and the absence of model/endpoint fields. Mocked provider
responses do not establish live account acceptance for all six providers. A
successful **Connect** confirms the selected key/model at that moment; it does
not prove later answer quality, future credit availability or historical coverage.
