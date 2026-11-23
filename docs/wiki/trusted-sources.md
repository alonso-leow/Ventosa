# Trusted sources
<!-- group: getting started -->

The array every scene is built over, so characters, assets, places, styles and voices stay the same across scenes and across videos. It lives in the workspace: `knowledge/trusted-sources.json`.

## An entry

- **kind**: character, asset, location, style, voice, brand.
- **refs**: files in the workspace (reference sheet, turnaround, model file, LUT, voice sample) or URLs.
- **locked**: traits that never change ("red scarf on the left", "logo 8 % of the width, top right").
- **recipe**: what reproduces it (seed, prompt fragment, model or LoRA name, render preset).
- **approved**: only approved entries bind the AI.

## The policy the AI follows

1. Load the trusted sources before any scene work.
2. Build over approved sources only: same refs, recipe and locked traits, in every scene.
3. A recurring element with no source: propose one and raise a decision; don't render it until approved.
4. Never change an approved source; a change is a decision for a person.
5. After a scene, record which sources it used (`V03#s2`).
6. Review steps check each scene against the locked traits.

## Where

access ▸ trusted sources: approve proposals, edit entries, see where each one was used.
