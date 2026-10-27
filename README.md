# Bramley

**A small, fast, native desktop player for Apple Music.** For Linux and macOS.

Bramley is built in Rust with [egui](https://github.com/emilk/egui) and [fastframe](https://github.com/crmne/fastframe), in the spirit of [Spotifast](https://github.com/crmne/spotifast). There is no browser engine.

- On Linux, Apple's own MusicKit JS runs unmodified, fetched from Apple at runtime, inside a small embedded JavaScript engine (QuickJS-NG). The web interfaces it needs (media element, Media Source Extensions, Encrypted Media Extensions, fetch) are provided natively and backed by the Widevine CDM from your installed Chrome. Bramley never ships or modifies that CDM.
- On macOS, Bramley plays through Apple's native MusicKit framework.

Memory is a hard requirement. The budget is 200 MiB for the whole app while playing on Linux (173 MiB measured at v0.7.2), and 100 MiB at idle on macOS (95.7 MiB measured at v0.7.2).

## Status

Bramley is at v0.7 and releases for Linux and macOS ship together from [GitHub Releases](https://github.com/natevick/bramley/releases). It works today:

- Sign in with your Apple Music account, then play full songs with play, pause, next, previous, seek, shuffle, repeat and a queue.
- Home, search, and your library (playlists, albums, songs, artists), with album and playlist pages.
- Keyboard navigation. Press `?` in the app for the shortcuts.
- A volume control on Linux. macOS has none, because MusicKit offers no volume API: use the system volume.
- A command line (`bramley play`, `pause`, `next`, `search <term>` and more) and an MCP server (`bramley mcp`) for scripts and AI agents. Run `bramley help` to print the full command list.

Not supported: Intel Macs, and 32-bit or ARM Linux. There is no in-app update check.

## Requirements

- An Apple Music subscription.
- **Linux:** x86_64, and Google Chrome. Bramley loads the Widevine CDM that Chrome installs at `/opt/google/chrome/WidevineCdm`, because Bramley does not ship one. Also a PulseAudio-compatible sound server (PulseAudio, or PipeWire with `pipewire-pulse`), and `xdg-open` (the `xdg-utils` package), which sign-in uses to open your browser. At run time Bramley opens Wayland or X11, EGL and xkbcommon libraries.
- **macOS:** an Apple silicon Mac on macOS 14 or later. No Chrome is needed.

## Install on Linux

### Arch and Omarchy

The AUR package `bramley-bin` is not published. Build it from the PKGBUILD in this repository. Install `google-chrome` from the AUR first, then:

```
git clone https://github.com/natevick/bramley
cd bramley/packaging/aur/bramley-bin
makepkg -si
```

`packaging/aur/README.md` lists the dependencies and the checksum step.

To keep the cache warm while Bramley is closed: `systemctl --user enable --now bramley-warm.timer`. The package installs this timer.

### Release tarball

Download `bramley-v<version>-x86_64-unknown-linux-gnu.tar.gz` and its `.sha256` file from [Releases](https://github.com/natevick/bramley/releases), then:

```
sha256sum -c bramley-v*-x86_64-unknown-linux-gnu.tar.gz.sha256
tar xzf bramley-v*-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 bramley-v*/bramley ~/.local/bin/bramley
bramley
```

The tarball also holds a desktop entry, an icon and the warm-cache units under `packaging/`. The units expect `/usr/bin/bramley`, so only the package sets them up.

### From source

You need stable Rust, a C and C++ compiler and `pkg-config`.

```
cargo build --release -p bramley
target/release/bramley
```

## Install on macOS

1. Download `Bramley-<version>-macos-arm64.zip` from [Releases](https://github.com/natevick/bramley/releases) and unzip it. The app is signed with a Developer ID and notarized by Apple.
2. Move `Bramley.app` to `/Applications` and open it.
3. Click **Sign in**. macOS asks whether Bramley may access Media & Apple Music. Click Allow. If you refused it before, allow Bramley in System Settings > Privacy & Security > Media & Apple Music.

Bramley's **Sign out** only forgets its own tokens. To revoke access, turn Bramley off in that same System Settings pane. The command line lives inside the app: `/Applications/Bramley.app/Contents/MacOS/bramley`.

## Sign in on Linux

Click **Sign in** in the app, or run `bramley sign-in`. Bramley starts a short-lived web page on `127.0.0.1` and opens it in your default browser with `xdg-open`. Click **Sign in** on that page and finish in Apple's own sign-in window. Bramley only receives the access token Apple hands back. The page expires after 10 minutes. If no browser opens, run `bramley sign-in` while Bramley is running: it prints the address to open yourself.

Bramley keeps the token in your desktop's Secret Service keyring (for example gnome-keyring). Without one it uses a private file under `~/.config/bramley`.

## Updates

Bramley does not update itself. Download the new release and replace the old one: unzip the new Mac app over the old one, or untar and reinstall on Linux. With the PKGBUILD, set `pkgver` to the new version, run `updpkgsums`, and run `makepkg -si` again.

## Reporting bugs

Open an issue at [github.com/natevick/bramley/issues](https://github.com/natevick/bramley/issues). Include:

- the Bramley version (`bramley --version`), your OS and version, and for Linux your sound server;
- what you did, what you expected, and what happened;
- the output of `bramley now`, with Bramley running. It prints the app's state as JSON, including what is playing and, while a sign-in is waiting, its local sign-in link. Read it first and delete anything you want to keep private.

## Ground rules

`docs/compliance.md` lists the rules Bramley follows to stay within Apple's developer terms. Among them:

- MusicKit JS is never bundled, patched or re-hosted.
- Decrypted audio never touches disk.
- Bramley never impersonates Apple's web player.

Bramley is not affiliated with or endorsed by Apple. Apple and Apple Music are trademarks of Apple Inc., registered in the U.S. and other countries.

## Development

`AGENTS.md` is the guide for working on the code, and `docs/` holds the decisions, budgets and test results.

## License

MIT. See `LICENSE`. `crates/engine/cdm-api/` contains Chromium's BSD-licensed CDM interface header; see `LICENSE.chromium` there.
