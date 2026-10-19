# Bramley

**A small, fast, native desktop player for Apple Music.** Linux first, then Windows and macOS.

Bramley is built in Rust with [egui](https://github.com/emilk/egui) and [fastframe](https://github.com/crmne/fastframe), in the spirit of [Spotifast](https://github.com/crmne/spotifast). There is no browser engine. Apple's own MusicKit JS runs unmodified, fetched from Apple at runtime, inside a small embedded JavaScript engine (QuickJS-NG). The web interfaces it needs (media element, Media Source Extensions, Encrypted Media Extensions, fetch) are provided natively and backed by the Widevine CDM from your installed Chrome. Bramley never ships or modifies that CDM.

The target is 200 MiB or less for the whole app while playing. The playback engine measures about 97 MiB today, most of which is Widevine itself.

## Install

Bramley needs:

- Linux on x86_64.
- Google Chrome. Bramley loads the Widevine CDM that Chrome installs in `/opt/google/chrome/WidevineCdm`. It does not ship one.
- An Apple Music subscription.
- A PulseAudio-compatible sound server: PulseAudio itself, or PipeWire with `pipewire-pulse`.
- `xdg-open` (the `xdg-utils` package), which sign-in uses to open your browser.

### Arch and Omarchy

The AUR package `bramley-bin` is coming soon. Until then, build the package from this repository once a release exists. Install `google-chrome` from the AUR first, then:

```
cd packaging/aur/bramley-bin
makepkg -si
```

`packaging/aur/README.md` explains the checksum step.

### From source

You need stable Rust, a C and C++ compiler and `pkg-config`. At run time Bramley opens Wayland or X11, EGL and xkbcommon libraries, so install those too.

```
cargo build --release -p bramley
target/release/bramley
```

## Status

Early. The playback engine (`crates/engine`) plays Widevine-protected audio end to end against public test streams. Apple Music playback and the interface come next.

## Ground rules

`docs/compliance.md` lists the rules Bramley follows to stay within Apple's developer terms. Among them:

- MusicKit JS is never bundled, patched or re-hosted.
- Decrypted audio never touches disk.
- Bramley never impersonates Apple's web player.

Requires an Apple Music subscription.

Bramley is not affiliated with or endorsed by Apple. Apple and Apple Music are trademarks of Apple Inc., registered in the U.S. and other countries.

## License

MIT. See `LICENSE`. `crates/engine/cdm-api/` contains Chromium's BSD-licensed CDM interface header; see `LICENSE.chromium` there.
