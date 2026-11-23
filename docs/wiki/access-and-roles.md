# Access and roles
<!-- group: getting started -->

Ventosa keeps identities locally in `~/.config/ventosa/identity.json` (mode 600): roles, users, password hashes (PBKDF2-SHA256, 120 000 rounds). The same file guards the desktop app, the launcher and every web app.

## Open mode

With no users the app runs open, for one person on one machine. Creating the first user (an admin) turns login on everywhere.

## Roles

A role grants:

- **graph** — the full desktop app (graph, queues, settings). Without it the person lands in the launcher.
- **apps** — the apps it may open: `dashboard`, `uploads`, `teleprompter`, `labelling`, `studio`, `docs`, or an AI-made app's id. `*` is all.
- **chat** + **tools** — a chat in the launcher whose AI may call only these tools (pick them in the tool picker). This is how a workload user gets the AI for one job without the rest of the graph.
- **docs edit** — may edit these docs.
- **admin** — may manage users and roles.

## Users

Name, display name, email, role, enabled, expiry, workspaces (empty = all). A user's tools can narrow the role's tools, never widen them. "must change password" asks for a new one at the next login.

## Lifecycle

- **Join**: create the user with a temporary password and "must change password".
- **Move**: change the role; the next request uses it.
- **Leave**: disable. Expired and disabled accounts are refused everywhere at once.

## Audit

`~/.local/share/ventosa/audit.jsonl`: every login (ok or refused), every refused app, every user change. access ▸ audit shows the latest lines.

## Web apps

Each web app asks for the login with HTTP Basic auth and checks the role's apps. Basic auth sends the password with every request: on anything but localhost, put the apps behind TLS (*Local network*, *SSH deploy*).
