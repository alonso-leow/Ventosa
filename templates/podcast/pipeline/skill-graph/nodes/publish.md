# Node skill: publish [human, labelling app ▸ Publish]
**Human does:** watches the edit in the player, picks a title and a description candidate (or edits them freely), sets tags, presses **Approve for upload**. Unapprove re-opens editing.
**Events:** `publish.approved`.
**AI may:** propose more candidates only by attaching to `publish_meta` (candidates are merged, never replace the human's text).
