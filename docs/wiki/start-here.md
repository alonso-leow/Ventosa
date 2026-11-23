# Start here
<!-- group: getting started -->

Ventosa runs content pipelines where people and AI share the work. The AI sees the work as a **skill graph** (one step node at a time, each with its own skill). People see it as a **to-do list**, a **queue** of what the AI made, and a few **apps** (labelling, teleprompter, uploads, dashboard, label studio).

## The three tools

- **The desktop app** (`ventosa`): the main tool. Graph, queues, schedules, access, settings, the AI.
- **The launcher** (`ventosa-portal`): a small desktop app for people who only label, record or chat. It logs in and shows only the apps their role grants.
- **The web apps**: pages served by the engine (`ventosa-app <name>`). They open in the browser and ask for the same login (HTTP Basic auth). If a browser page is not wanted, a Tauri shell can wrap any of them (see *Local network*).

## First hour

1. Onboarding: brand, AI provider (see *AI providers*), the first workspace.
2. **access** tab: create the first admin. From then on the app asks for a login.
3. Add the team: one user per person, with a role (*Access and roles*).
4. If the team works from other machines: *Local network* or *SSH deploy*.
5. Fill the trusted sources before the first scene (*Trusted sources*).

## Where things are

| You want to | Go to |
|---|---|
| see where every video is | graph ▸ overview |
| your own steps | graph ▸ to-do |
| what the AI is doing / will do | history; graph ▸ configure cron job |
| who can do what | graph ▸ access |
| apps (label, record, upload, dashboard, studio) | header ▸ open apps ▾ |
| these docs | header ▸ open apps ▾ ▸ docs, or the launcher |
