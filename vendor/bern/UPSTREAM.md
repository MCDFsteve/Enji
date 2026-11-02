# Bern source snapshot

- Upstream: https://github.com/AimesSoft/Bern
- Commit: `14a5b2631af1f88077b474c6ff5d566980d1de0e` (master, 2026-11-02)
- License: [MIT](LICENSE)

This copy holds Bern's framework crates exactly as that commit has them, so the repository builds without another checkout. The root workspace omits the upstream examples, benchmarks and documents; the framework crates and their license are kept. Because the examples are left out, the `resources` tests of `bern-backend-swiftui`, which read them, fail in this copy.

To update, copy `crates/` from a newer upstream commit and change the commit above.
