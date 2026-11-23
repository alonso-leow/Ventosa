# Google CLI and Google Workspace CLI
<!-- group: integrations -->

Two recommended tools (settings ▸ recommended tools).

## Google Cloud CLI (gcloud)

For Google Cloud projects, APIs and keys: the YouTube Data API for uploads, Cloud Storage for renders, Vertex AI.

```
yay -S google-cloud-cli          # Arch (AUR); other systems: cloud.google.com/sdk/docs/install
gcloud auth login
gcloud projects create my-channel-ops
gcloud config set project my-channel-ops
gcloud services enable youtube.googleapis.com drive.googleapis.com sheets.googleapis.com
```

OAuth clients (needed for YouTube uploads on your channel) are made in the console: APIs & Services ▸ Credentials ▸ Create credentials ▸ OAuth client ID ▸ Desktop app. Keep the client JSON out of workspaces.

![Google Cloud console, Credentials page, OAuth client ID](img/google/oauth-client.png)

## Google Workspace CLI (gws)

Drive, Docs, Sheets, Gmail and Calendar from the shell, so the AI can put scripts in Docs, read briefs from Sheets and put shoots on the Calendar.

```
npm i -g @googleworkspace/cli
gws --help
```

The first run walks through OAuth with a Google Cloud project (the one above, with the Workspace APIs enabled). Then grant the tool to the AI: settings ▸ permissions `run` = ask, and add it as a custom tool or let the AI call it through `tool_run`.

## Good practice

- One Google Cloud project per brand.
- Least scopes: only the APIs a pipeline uses.
- A shared Drive for the team; workspaces link to files, they don't copy them.
