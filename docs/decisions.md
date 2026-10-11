# Product decisions

- **2026-10-10: developer token distribution (Nate).**
  - A scheduled GitHub Action mints a MusicKit developer token from Nate's key and publishes it as a static file the app downloads. The key lives only as GitHub repository secrets.
  - The app caches the token, refreshes it when fewer than 30 days remain, and keeps working on the cached copy if the fetch fails.
  - No server and no re-release for expiry. Users never see the key.
  - Risk accepted: all Bramley traffic is attributed to Nate's developer account, and Apple can revoke the key.
- **2026-10-10: layout (Nate).** Spotifast's shape:
  - a library sidebar on the left;
  - a content area for search, playlists and albums;
  - a player bar along the bottom;
  - the queue as a side panel.
  Every UI slice ships screenshots before merge.
- **2026-10-10: artwork cache (Nate).** Artwork is cached in memory only, never on disk.
- **2026-10-10: agent access (Nate).** Bramley is controllable by agents: a CLI (`bramley play|pause|next|…`) and a local MCP server (`bramley mcp`). Both talk to the running instance. See the compliance interpretation log.
- **2026-10-10: user-token storage (coordinator's call after the S3 review; Nate may override).**
  - The keyring is the source of truth, and there is only ever one copy. An import into the keyring is read back and then deletes the old `music-user-token` file. A save to the keyring also removes the file.
  - The 0600 file is used only when no Secret Service exists on the bus. It is written atomically, and the storage mode ("keyring" or "file") shows in status and the UI. A keyring error, timeout or cancelled unlock prompt is an error, never a silent fallback. A failed keyring write makes the sign-in page show the error instead of "Done".
- **2026-10-10: the sign-in page wipes its own storage (coordinator's call; the reviewer asked for Nate's read under compliance rule 2).** After a successful token POST, the page calls `localStorage.clear()` and `sessionStorage.clear()` on its own loopback origin, and the response carries `Clear-Site-Data: "storage"`. This is our page's behaviour on our origin, not a change to MusicKit. `unauthorize()` is not called, because it would revoke the token Bramley just stored.
