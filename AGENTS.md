# Bramley agent guide

Bramley, a small native player for Apple Music: Rust + egui on crmne/fastframe, modeled on Spotifast. Linux first; Windows and macOS must keep compiling.

Read `docs/compliance.md` before touching playback, tokens, networking or anything that loads musickit.js. Its rules are binding. When a task would break one, stop and ask Nate instead of working around it.

## Budget
The whole app stays at or under 200 MiB PSS while playing. Each spike or change that affects memory records a measurement against the split in `docs/budget.md`. A number nobody measured is not a number.

## Experiments
State the pass condition in a `*-PREREG.md` before running. Prove the harness can fail (a sabotage run) before trusting a pass.

## Code
- No browser engine. MusicKit JS runs in our own JS runtime with native host objects.
- UI emits actions; network, playback and JS-runtime work runs off the UI thread.
- Platform code goes behind `cfg` or target modules.
- Never log tokens, license payloads or authorization responses.
- Each non-shipped cargo feature gets a `#[cfg(feature)] #[used]` marker, `bramley-unshipped-feature:<crate>/<feature>`, in its crate root. `scripts/check-shipped-features.sh <binary>` must exit 0 on anything shipped.
- Keep work out of `/tmp`, which runs out of inodes on the dev box. Use the repo or `$HOME`.
