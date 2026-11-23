# Team setup
<!-- group: getting started -->

The checklist for a team to work well in the pipelines. Do it once per installation; the docs page for each step has the detail.

## 1. One machine is the hub

The hub runs the desktop app, the engine and the web apps. Workspaces (the folders with the graph, knowledge and data) live on it. Pick a machine that stays on.

## 2. AI provider

Set it in settings ▸ provider. Keys stay in environment variables, never in files (*AI providers*). Set the limits (settings ▸ limits): concurrency, requests per minute, retries.

## 3. Identities

- access ▸ users: create the first **admin** (this turns login on).
- One user per person; never share accounts — the audit log is per user.
- Roles: `admin` (everything), `operator` (runs graphs), `labeller` (labelling + studio only, lands in the launcher), `assistant` (a chat with a chosen set of tools). Make your own roles for your steps.
- Give an expiry date to freelancers and guests. Disable, don't delete, people who leave: their history stays readable.

## 4. Reaching the hub

- Same office or home: *Local network* (bind to the LAN, firewall, TLS proxy).
- Remote people: *SSH deploy* (publish the apps to a server) or a VPN to the hub.

## 5. The pipeline

- graph ▸ overview: check every step's owner (AI or human) and its *done when*.
- Every human step names its app (labelling, teleprompter, uploads, studio).
- Elevate rules: what the AI must ask a person (decisions, edge cases, dangerous scenarios).
- Trusted sources for every recurring character, asset and style (*Trusted sources*).

## 6. Daily rhythm

- Morning: to-do (people), decisions (the AI asked), history ▸ failed.
- The dashboard: what moved per phase, what was uploaded, what the tokens cost.
- Weekly: access ▸ audit — logins, refusals, accounts close to expiry.
