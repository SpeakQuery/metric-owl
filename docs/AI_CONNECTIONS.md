# AI provider connections

Open **Notifications & connections → AI providers**, choose a provider, paste its
API key, and select **Connect**. Metric Owl supplies the endpoint, model and request
settings. There are no model names, API URLs or tuning fields to fill in.

Connect sends a short synthetic message to the selected provider, verifies a text
response, and only then saves the encrypted key and settings together. Provider API
usage rates apply to this check. It does not send server evidence. Event analysis
still starts from **Analyze evidence with AI**; connecting does not turn on automatic
analysis. Existing automatic-analysis preferences are preserved.

A failed connection attempt keeps the previous key and configuration. Changing the
provider requires a key for that provider. The browser clears an unsaved key when
the provider is changed, and clears a submitted key after a successful save. Saved
keys never come back from the API. Session reauthentication and CSRF checks apply.
Connection checks are limited to 20 per day; incident analysis retains its separate
10-request daily budget.

## Automatic presets

The backend is the single source for the provider catalog. Presets reviewed against
the linked official documentation on October 31, 2026:

| Provider | Automatic model | Reference |
| --- | --- | --- |
| OpenAI | `gpt-4.1-mini` | [Model documentation](https://developers.openai.com/api/docs/models/gpt-4.1-mini) |
| Anthropic (Claude) | `claude-haiku-5-5` | [Models overview](https://platform.claude.com/docs/en/models/overview) |
| Google Gemini | `gemini-2.5-flash` | [Model catalog](https://ai.google.dev/gemini-api/docs/models) |
| Groq | `openai/gpt-oss-20b` | [Supported models](https://console.groq.com/docs/models) |
| xAI (Grok) | `grok-4.3` | [Model migration](https://docs.x.ai/developers/migration/may-15-retirement) |
| DeepSeek | `deepseek-flash` | [API quick start](https://api-docs.deepseek.com/) |
| Mistral AI | `mistral-small-latest` | [Chat API](https://docs.mistral.ai/api) |
| OpenRouter | `openai/gpt-4.1-mini` | [API overview](https://openrouter.ai/docs/api/reference/overview) |
| NVIDIA NIM | `nvidia/nemotron-3-nano-30b-a3b` | [Model API](https://docs.api.nvidia.com/nim/reference/nvidia-nemotron-3-nano-30b-a3b) |
| SiliconFlow | `Qwen/Qwen3-8B` | [Chat API](https://docs.siliconflow.com/en/api-reference/chat-completions/chat-completions) |

Access and credit requirements depend on the provider account. A catalog entry is
not proof that an account can generate: Connect checks that before reporting success.
Presets are versioned in the application and must be maintained as providers retire
models. There is no fallback to an arbitrary, potentially more expensive model.

Existing OpenAI-compatible installations are identified by their saved endpoint,
so an old Groq or xAI connection is not mislabeled as OpenAI. Checking an existing
key preserves its configured model. Unknown custom endpoints remain intact until
the user explicitly connects a listed provider with a new key.

## External AI apps and MCP

**External AI apps (MCP)** connects an application such as Codex to the monitoring
server's read tools. It is a different connection from a provider API key. The MCP
page links directly to the simple provider form; existing local/SSH MCP connections,
scopes and revocation continue to work. See [MCP connections](MCP_CONNECTIONS.md).

## Verification scope

Local automated checks cover the provider catalog, protocol payloads and response
parsing, encrypted storage, credential isolation, authentication/CSRF, failure
states, desktop/mobile form behavior and existing MCP setup. Browser success/error
responses are simulated so these tests do not spend provider credits. They are not
live generation acceptance for all ten providers or proof of a deployed Linux update.
