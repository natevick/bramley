# S15c long soak: pre-registration (2026-10-13)

Follow-up to race 3 (S14b's HTTP-cache fix, with an estimated residual of ≤ 1.6 % per duplicated preload). **This is information, not acceptance.** A soak run never counts toward v1, and it changes no bar in `docs/acceptance.md`. It is written and committed before any code for it and before any soak run.

## The mode
`scripts/acceptance.sh <run> --soak <minutes>` runs only:
1. the reference reading before;
2. one shipped-binary soak of `<minutes>` minutes, the same shape as R3: the 18-track album queue, each long item sought to 10 s before its end once it has played 150 s, the Home view with 50 covers on screen, a snapshot every 10 s and a poll every second;
3. the reference reading after.

It keeps the same isolated config, keyring and cache, the unique null sink, the routing guard, the co-tenant watch, the Apple pacing of Amendment 9 and the leak grep. Its verdict file records `mode: soak` and `countsTowardV1: false`.

**Queue refill.**
- The album holds 18 tracks. At about 2.7 min per item it ends in under an hour.
- When 4 or fewer queued items remain, the soak sends `queue add` for the 18 album tracks again.
- Queue adds don't start an item and are not paced actions. Every item change is a natural advance or a seek to the end, counted as one action by Amendment 9, exactly as in R3.
- This applies to the soak mode only. The counted 30-minute R3 never refills.

## Pass condition for the 120-minute run
All must hold:
1. **No permanent stall:** at no time is the state Loading or Playing with the position frozen for more than **30 s**.
   - The driver polls every second and records each such stretch with its start, end, item, position and state.
   - The gate also looks for runs of 10 s snapshots with an unchanged position and item that span more than 30 s.
   - A single stall of either kind FAILs the run and reopens race 3.
2. **≥ 30 item changes**, counted by the driver's 1 s poll.
3. **`outputUnderflows` Δ = 0** across the soak, and **`readinessDips` Δ = 0**. A counter that can't be read is a FAIL.
4. **No crash or restart:** every snapshot carries the session's pid and exactly 1 process.
5. **Pacing peak ≤ 20** in any 600 s window, as for every mode.
6. **The run completes:** the last snapshot reaches the requested minutes.

**Reported, with no bar:** PSS, USS and RSS growth from minute 5 to the end, and the growth per hour; the item-change count; every stall; the counters; the number of snapshots carrying a banner.

**Void conditions (as for R3):** the guard killed the app, a stream was seen off the null sink, a renderer that isn't radeonsi, a co-tenant, the sink left behind, the default sink changed, a reference reading's sha, pid or renderer.

**Missing evidence is FAIL.** A soak that dies partway writes what it has, so it fails on "ended at minute" rather than on missing evidence.

## Plan
- The 120-minute run starts only after the coordinator's counted run releases the GPU lock, from a clean worktree at the final commit.
- No counted run is made in this session, and nothing here adds to the number of runs v1 needs.

## Clarification (written with the code; no rule change)
The last snapshot of a soak can fall up to one 10 s snapshot period short of the requested minutes, so "reaches the requested minutes" is checked with a 10 s tolerance. A stall is a stretch of more than 30 s: a stretch of exactly 30 s is not one.

## Amendment (2026-10-14, S17)
Amendment 11 of `docs/s13-acceptance-PREREG.md` adds a gate check on every Playing sample, `position <= duration + 1`, that FAILs soak runs too.
