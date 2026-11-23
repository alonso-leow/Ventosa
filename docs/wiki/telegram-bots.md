# Telegram bots
<!-- group: integrations -->

A Telegram bot lets people talk to the pipeline's AI from their phone: ask where a video is, add a task, approve a decision. Bots run **inside the desktop app** (no server needed): access ▸ bots.

## 1. Make the bot

1. In Telegram, open **@BotFather** ▸ `/newbot` ▸ a name ▸ a username ending in `bot`.
2. BotFather replies with a token (`123456:ABC…`). Put it in an environment variable, e.g. `TELEGRAM_BOT_TOKEN`, and restart Ventosa.
3. Optional: `/setprivacy` (groups), `/setcommands` (`status`, `todo`, `help`).

![BotFather, /newbot reply with the token](img/telegram/botfather.png)

## 2. The bot in Ventosa

access ▸ bots ▸ add:

- **name**, **token env** (`TELEGRAM_BOT_TOKEN`), **workspace**.
- **tools**: the tools its AI may call (the tool picker). Start read-only: `workspace_status`, `queue_list`, `list_tasks`, `help`.
- **allowed chats**: Telegram chat ids that may use it. Message the bot once; refused chats show their id in the bot's log line, copy it here.
- **enabled**.

## 3. The bot as a subgraph

Each bot is a small graph of its own: **receive** (long polling `getUpdates`) → **ai reply** (a chat with the bot's tools, unattended: tools set to *ask* are refused) → **send** (`sendMessage`). Each Telegram chat keeps its own Ventosa chat, saved like any other (ai ▸ history, mode `bot`).

## 4. Safety

- Only allowed chats get answers.
- No write tools until you trust it; decisions stay with people.
- The token is a password: revoke it in BotFather (`/revoke`) if it leaks.
