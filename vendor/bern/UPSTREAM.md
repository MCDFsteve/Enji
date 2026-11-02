# Bern source snapshot

- Upstream: https://github.com/AimesSoft/Bern
- Commit: `14a5b2631af1f88077b474c6ff5d566980d1de0e` (master, 2026-11-02)
- License: [MIT](LICENSE)

This copy holds Bern's framework crates exactly as that commit has them, so the repository builds without another checkout. The root workspace omits the upstream examples, benchmarks and documents; the framework crates and their license are kept. Because the examples are left out, the `resources` tests of `bern-backend-swiftui`, which read them, fail in this copy.

Added here since, to go upstream once every platform has it:

- `bern::now_playing` (`crates/bern-command/src/now_playing.rs`): the system's now playing and the media commands it sends; macOS and iOS through MediaPlayer, Windows through the system media transport controls, Linux through MPRIS, Android through the host.
- `[app].background_audio`: iOS plays the application's own audio on behind the lock screen.

To update, copy `crates/` from a newer upstream commit and change the commit above.
