# Spike 3a-GPU: UI baseline on the real GPU (pre-registered 2026-10-09)

This re-runs spike 3a's UI baseline, unchanged, on the real GPU. That became possible once `norm` joined the render group.

- **Compositor:** weston 14 headless with `--renderer=gl`, run under `sg render`.
- **Binaries:** the app connects over Wayland (`WAYLAND_DISPLAY`, `DISPLAY` unset), running the same `bare` and `fastframe` binaries.
- **Measure:** the same `measure.py`: 5 s warm-up, then 30 s sampled.

**Bar (unchanged from 3a):** the fastframe build is ≤ 70 MiB PSS for PASS, 70–100 for MARGINAL, and > 100 for FAIL.

**Did-it-run checks:**
- The logged `gl_renderer` must NOT contain `llvmpipe`/`softpipe`. If it does, the run is VOID.
- The frame count must be > 0.
- Weston must report the GL renderer on a real DRM device in its log.

**Named false positive:** Mesa can fall back silently to llvmpipe under EGL. The renderer-string check guards against this.
