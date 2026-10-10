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
