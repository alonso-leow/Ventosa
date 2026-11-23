# AI providers
<!-- group: providers -->

Ventosa is AI first: the built-in AI, executions and schedules all run on the provider in settings ▸ provider. Keys live only in environment variables; the config stores the variable's **name**.

Set a key for the desktop session (fish: `set -Ux ANTHROPIC_API_KEY sk-ant-…`; bash/zsh: `export …` in `~/.profile`; niri: `environment { … }`), then restart Ventosa.

Screenshots: each step below has a slot (`img/<provider>/…png`). Drop a capture of your own console there and it shows up in place.

## Anthropic (Claude)

1. Open console.anthropic.com and sign in.
2. Settings ▸ **API keys** ▸ **Create key**. Name it "ventosa", pick the workspace.
3. Copy the key once (it starts with `sk-ant-`).
4. Billing: add credits under **Billing**; set a monthly spend limit under **Limits**.
5. In Ventosa: kind `anthropic`, base url `https://api.anthropic.com`, model `claude-opus-5-5` (or `claude-sonnet-5-5` for cheaper runs), key env `ANTHROPIC_API_KEY`.

![Anthropic console, API keys page with the Create key button](img/anthropic/api-keys.png)
![Anthropic console, Limits page with the monthly spend limit](img/anthropic/limits.png)

## OpenAI (ChatGPT)

1. platform.openai.com ▸ **API keys** ▸ **Create new secret key** (scope it to a project).
2. Billing: Settings ▸ Organization ▸ **Billing**, add credit; set usage limits per project.
3. In Ventosa: kind `openai-compatible`, base url `https://api.openai.com/v1`, key env `OPENAI_API_KEY`.

![OpenAI platform, API keys page](img/openai/api-keys.png)

## OpenRouter

One key, many models (Claude, GPT, open models).

1. openrouter.ai ▸ Settings ▸ **Keys** ▸ **Create key**; you can give the key a credit limit.
2. Add credits under **Credits**.
3. In Ventosa: kind `openai-compatible`, base url `https://openrouter.ai/api/v1`, model as `vendor/model` (e.g. `anthropic/claude-opus-5.5`), key env `OPENROUTER_API_KEY`.

![OpenRouter keys page with a credit limit](img/openrouter/keys.png)

## Google Gemini

1. aistudio.google.com ▸ **Get API key** ▸ create a key in a Google Cloud project.
2. In Ventosa: kind `openai-compatible`, base url `https://generativelanguage.googleapis.com/v1beta/openai/`, model e.g. `gemini-2.5-pro`, key env `GEMINI_API_KEY`.

![Google AI Studio, API keys](img/google/ai-studio-keys.png)

## DeepInfra

1. deepinfra.com ▸ Dashboard ▸ **API keys** ▸ **New API key**.
2. In Ventosa: kind `openai-compatible`, base url `https://api.deepinfra.com/v1/openai`, key env `DEEPINFRA_API_KEY`.

## Local models (Ollama)

1. Install Ollama, `ollama pull <model>`.
2. In Ventosa: kind `openai-compatible`, base url `http://127.0.0.1:11434/v1`, key env any variable set to any value (e.g. `OLLAMA_KEY=local`).
3. Tool calling depends on the model; pick one that supports tools.

## Voice (ElevenLabs and others)

settings ▸ voice. ElevenLabs: elevenlabs.io ▸ **API keys** (in your profile settings) ▸ create; the voice id is on the voice's page in **Voices**. Key env `ELEVENLABS_API_KEY`. OpenAI, DeepInfra and OpenRouter speech use the same keys as above.

![ElevenLabs, API keys](img/elevenlabs/api-keys.png)

## Choosing

- Scripts and decisions: the strongest model you can afford.
- Batches over many subjects: a cheaper model, a requests-per-minute limit, retries on.
- The dashboard's token columns show what each video cost; adjust from there.
