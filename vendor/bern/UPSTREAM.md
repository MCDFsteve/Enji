# Bern source snapshot

- Upstream: https://github.com/AimesSoft/Bern
- Baseline: `89993e621da062094ea5be5f207027310f806351`, with the uncommitted `glow-blur` work on top
- License: [MIT](LICENSE)
- Snapshot date: 2026-11-02

Enji Music uses Bern's `glow` and `blur` (the cover's light and the blurred backdrop), which are not yet an upstream commit. This copy holds the framework crates exactly as Enji was built and tested with them, so the repository builds without another checkout. The root workspace omits the upstream examples, benchmarks and documents; the framework crates and their license are kept.

One change was made here after the snapshot: `bern package --target ios` makes an `.ipa` (`crates/bern-backend-swiftui/src/ios.rs`). It builds the engine, the SwiftUI frontend and the icons again for devices into the simulator bundle, marks it for iPhoneOS and signs it ad hoc; a sideloading tool signs it again to install. It belongs upstream too.

The `resources::tests` test of `bern-backend-swiftui` reads the upstream examples, so it fails in this copy.

When `glow-blur` is merged, replace this directory with the upstream release that contains it, or depend on that release directly.
