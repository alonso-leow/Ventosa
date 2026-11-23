# Notion
<!-- group: integrations -->

Use Notion as the team's brief and knowledge space while Ventosa runs the pipeline.

## 1. An integration

1. notion.so ▸ Settings ▸ **Connections** ▸ **Develop or manage integrations** (or notion.so/profile/integrations) ▸ **New integration**.
2. Type **Internal**, pick the workspace, give it read (and, if the AI writes back, update/insert) content capabilities.
3. Copy the **Internal integration secret** (`ntn_…`) into `NOTION_TOKEN`.

![Notion, new internal integration with its capabilities](img/notion/integration.png)

## 2. Share pages with it

An integration sees only the pages shared with it: on each page or database ▸ **⋯** ▸ **Connections** ▸ add the integration.

![Notion page menu, Connections, add the integration](img/notion/connections.png)

## 3. Give the AI the tools

- Notion's hosted MCP server: `claude mcp add --transport http notion https://mcp.notion.com/mcp` (OAuth in the browser), or for Ventosa's own chat, a custom tool that calls the API with `NOTION_TOKEN`.
- Typical uses: briefs database → `pipeline_intake` (one row = one subject); finished scripts written back as pages.

## 4. Roles

Give the Notion tools only to the roles that need them (access ▸ roles ▸ tools).
