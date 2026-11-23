# Plugins

A plugin is **one JSON file**. Installing it (Settings ▸ Plugins, or the AI's `plugin_install`) adds, depending on what the file declares:

| Section | Where it shows up |
|---|---|
| `nodes` | the node gallery, tab **Plugins** (the "+" on the graph bar) |
| `connections` | Connections, section **External apps (plugins)**: the apps a human-owned node works in |
| `settings` | Settings, a section of its own under **Plugin settings** |
| `tools` | shell tools added to a workspace's `pipeline/tools.json` (`<plugin>_<tool>`), run by the AI with `tool_run` |

Example: `docs/examples/pixel-art.ventosa-plugin.json`.

## The file

```json
{
  "format": 1, "id": "pixel-art", "name": "Pixel Art", "version": "1.0.0", "author": "", "description": "",
  "permissions": ["nodes", "connections", "settings", "tools"],
  "nodes":       [{"id": "pixelate_stills", "owner": "ai|human", "need": "one line", "skill": "markdown, ≤ 8 KB",
                   "kind": "" | "folder", "per": "" | "clip", "app": "", "path": ""}],
  "connections": [{"id": "aseprite", "name": "Aseprite", "what": "", "link": "https://…", "key_env": "NAME_OF_ENV_VAR"}],
  "settings":    {"title": "", "fields": [{"key": "palette", "label": "", "kind": "text|toggle|choice|number", "default": null, "options": [], "help": ""}]},
  "tools":       [{"name": "x", "description": "", "command": "sh -c body; $1 is the argument"}]
}
```

## Policy

1. **One file**, UTF-8 JSON, at most 256 KB, `"format": 1`. Nothing else is read or fetched.
2. **Declared permissions.** `nodes`, `connections`, `settings` and `tools` must be listed in `permissions` to be used; anything present but undeclared rejects the file.
3. **Slugs, no collisions.** Plugin and connection ids are `a-z0-9-`; node, setting and tool ids `a-z0-9_`; at most 40 characters, starting with a letter. A node may not reuse a built-in gallery id, a connection may not reuse a built-in connection id, duplicates are rejected.
4. **Caps.** 40 nodes, 20 connections, 20 setting fields, 10 tools; a node's skill ≤ 8 KB; plain text only (no control characters).
5. **No secrets, no installers.** `key_env` is the *name* of an environment variable (`A-Z0-9_`), never a key. A connection has an `https://` link; it carries **no install command**: Ventosa does not run code a plugin brings when someone presses Connect. A human-owned app with no key is marked "set up" by a person.
6. **Node extras** are limited to `kind` (`folder`), `per` (`clip`), `app`, and a workspace-relative `path` (no `..`, not absolute).
7. **Shell tools are human-installed.** A plugin that declares `tools` runs shell commands, so only a person can install it (Settings ▸ Plugins), and adds its tools to a workspace on purpose (a button), after enabling it. The AI's `plugin_install` refuses such a file. Running a tool still goes through the `run` permission (settings ▸ permissions).
8. **AI proposes, a human enables.** A plugin installed by the AI starts **disabled** and adds nothing until an admin enables it. `plugin_install` is an *install* capability, so settings ▸ permissions can set it to ask or deny.
9. **Versions.** The same id with a higher or different version upgrades in place (settings values are kept). The same id **and** version with other content is refused: raise the version or remove it first. The file's SHA-256 is stored.
10. **Admins only** manage plugins in the app. Install, enable, disable and remove land in the audit log (`audit.jsonl`).
11. **Removal** deletes the plugin's folder: its nodes leave the gallery, its settings section and connections disappear. Nodes already placed in a graph stay (they are copies).

Stored in `$XDG_DATA_HOME/ventosa/plugins/<id>/` (`plugin.json` = the file as installed, `state.json` = enabled, hash, who, setting values, connections marked set up).

## For the AI

| Tool | Capability | Does |
|---|---|---|
| `plugin_list` | read | installed plugins and what each adds |
| `plugin_validate` | read | checks a file (path or JSON text) against this policy, lists every reason to reject |
| `plugin_install` | install | installs a file **disabled** |
| `plugin_remove` | install | removes a plugin by id |

Write the file, run `plugin_validate`, fix what it lists, `plugin_install`, then ask the human (`request_human`) to enable it in Settings ▸ Plugins.
