# Bern source snapshot

- Upstream: https://github.com/AimesSoft/Bern
- Baseline: `89993e621da062094ea5be5f207027310f806351`, with the uncommitted `glow-blur` work on top
- License: [MIT](LICENSE)
- Snapshot date: 2026-11-02

Enji Music uses Bern's `glow` and `blur` (the cover's light and the blurred backdrop), which are not yet an upstream commit. This copy holds the framework crates exactly as Enji was built and tested with them, so the repository builds without another checkout. The root workspace omits the upstream examples, benchmarks and documents; the framework crates and their license are kept.

When `glow-blur` is merged, replace this directory with the upstream release that contains it, or depend on that release directly.
