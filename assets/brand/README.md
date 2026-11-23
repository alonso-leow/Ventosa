# Ventosa brand kit

Colours and type come from the app theme (`src/bin/ventosa/theme.rs`): sharp corners, hairline rules, monospace, orange = to-do / secondary, purple = AI / primary. Tokens: `colors.json`, `tokens.css`. Overview: `ventosa-palette.svg`.

Gradient: orange `#ff8a1f` → purple `#8b3dff` (left → right on the wordmark, diagonal on the V and backgrounds). Type: JetBrains Mono Bold, outlined in the SVGs.

| File | Use |
|---|---|
| `ventosa-wordmark` | primary logo, gradient, transparent |
| `ventosa-wordmark-orange` / `-purple` | solid, transparent |
| `ventosa-wordmark-light-on-gradient` / `-on-purple` / `-on-orange` | white on colour |
| `ventosa-wordmark-app-panel` | gradient on the app's panel surface |
| `ventosa-v` | gradient V, transparent |
| `ventosa-v-orange` / `-purple` / `-purple-lit` / `-white` / `-black` | solid V, transparent |
| `ventosa-v-light-on-gradient` | app icon (rounded); `png/ventosa-icon-*` and `favicon.ico` |
| `ventosa-v-light-on-purple` / `-on-orange` | white V on solid colour |
| `ventosa-v-app-panel` / `-app-tube` / `-app-orange-on-purple-deep` | square, hairline, in the app's surfaces |
| `ventosa-banner` | 1200×630 social / repo banner in the app's look |

Every SVG has a PNG in `png/`. Use the gradient only on transparent, panel or white; on busy backgrounds use a solid or white/black.
