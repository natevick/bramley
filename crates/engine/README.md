# bramley-engine

The playback engine behind [Bramley](https://github.com/natevick/bramley), a small native Apple Music player.

It runs Apple's MusicKit JS in an embedded JavaScript runtime (QuickJS, through `rquickjs`) and gives it the web interfaces it needs, implemented natively: a media element, Media Source Extensions, Encrypted Media Extensions and fetch. Underneath are an fMP4 demuxer, an AAC decoder (symphonia), a Widevine CDM host and a PulseAudio client. There is no browser engine.

## What you need

- Your own Apple developer token, signed with your own MusicKit key.
- An Apple Music subscription, and that user's Music-User-Token.
- Google Chrome installed, for its Widevine CDM. The engine loads the CDM that Chrome installed; it does not ship one. Linux x86_64 is the only supported platform today.
- A C++17 compiler at build time, for the small CDM host shim.

The build needs no network access and no Chrome. Both are needed only at run time.

## MusicKit JS

The engine fetches MusicKit JS from Apple's CDN at run time and runs it unmodified. It never bundles, re-hosts or patches it. Bramley follows the rules in [docs/compliance.md](https://github.com/natevick/bramley/blob/main/docs/compliance.md), which you should read before building on this crate.

This project is not affiliated with or endorsed by Apple. Apple Music and MusicKit are trademarks of Apple Inc.

## Status

Pre-1.0. The API may change in any 0.x release.

## Features

None are on by default. `sabotage`, `diagnostics` and `trace` are internal testing switches used by Bramley's own test harnesses. They have no stability guarantee, may change or disappear in any release, and should stay off in anything you ship.

## License

MIT, see `LICENSE`. `cdm-api/` holds Chromium's Content Decryption Module interface headers, under Chromium's BSD-3-Clause licence in `cdm-api/LICENSE.chromium`.
