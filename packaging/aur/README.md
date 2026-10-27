# AUR package: bramley-bin

`bramley-bin` installs the prebuilt Linux x86_64 binary from a GitHub release. It is not published to the AUR yet. This directory holds the files to publish once a release and an AUR account exist.

## What the package contains

| Path | From the release tarball |
| --- | --- |
| `/usr/bin/bramley` | `bramley` |
| `/usr/share/applications/bramley.desktop` | `packaging/applications/bramley.desktop` |
| `/usr/share/icons/hicolor/scalable/apps/bramley.svg` | `packaging/icons/bramley.svg` |
| `/usr/lib/systemd/user/bramley-warm.service` | `packaging/systemd/bramley-warm.service` |
| `/usr/lib/systemd/user/bramley-warm.timer` | `packaging/systemd/bramley-warm.timer` |
| `/usr/share/licenses/bramley-bin/LICENSE` | `LICENSE` |
| `/usr/share/doc/bramley-bin/README.md` | `README.md` |

## Dependencies

- `libglvnd`, `wayland`, `libxkbcommon`, `libx11`, `libxcursor`, `libxi`, `libxrender`, `libxkbcommon-x11`: the binary opens these at start-up (it does not link them), so `ldd` does not show them. namcap reports them as "may not be needed" for that reason. They stay.
- `libgcc`, `libstdc++`, `glibc`: linked directly.
- `pulse-native-provider`: Bramley speaks the PulseAudio protocol itself, with no `libpulse`. `pulseaudio` and `pipewire-pulse` both provide it.
- `google-chrome` (AUR): Bramley loads the Widevine CDM from `/opt/google/chrome/WidevineCdm`. Install it before building, because pacman cannot fetch AUR packages.
- `xdg-utils`: sign-in starts `xdg-open` to show the Apple Music sign-in page in the user's browser. Without it the button does nothing.
- `hicolor-icon-theme`: owns the icon directory tree.
- Optional `gnome-keyring` (or any Secret Service): keeps the Apple Music sign-in in the keyring. Without one, Bramley uses a private file under `~/.config/bramley`.
- Optional `fontconfig`: Bramley asks `fc-match` for the desktop's text hinting and antialiasing settings, and uses defaults when it is missing.

## After the v0.1.0 release exists

The `sha256sums_x86_64` in `bramley-bin/PKGBUILD` is all zeros on purpose, so a build fails until it holds the real value.

1. Install `pacman-contrib` and `namcap`.
2. `cd packaging/aur/bramley-bin`
3. `updpkgsums` downloads the release tarball and writes its checksum into the PKGBUILD. Compare it with the `.sha256` file attached to the release.
4. `makepkg --printsrcinfo > .SRCINFO`
5. Commit both files.
6. Test the build: `makepkg -si`. `google-chrome` must already be installed.

## Publishing to the AUR

Needs an AUR account with an SSH key added to its profile.

1. `git clone ssh://aur@aur.archlinux.org/bramley-bin.git` (an empty repository the first time).
2. Copy `PKGBUILD` and `.SRCINFO` into the clone and add a `# Maintainer: Name <email>` line at the top of the PKGBUILD. Regenerate `.SRCINFO`.
3. `namcap PKGBUILD` and `namcap` on the built package. Expected warnings: the dlopened libraries above, the unused `ld-linux` library, and a reference to `x86_64` in the source name.
4. `git add PKGBUILD .SRCINFO && git commit && git push` on branch `master`.

## Later releases

Set `pkgver`, reset `pkgrel=1`, run `updpkgsums`, regenerate `.SRCINFO`, and push to the AUR repository.
