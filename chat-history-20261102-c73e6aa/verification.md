# QA c73e6aa1703ac8d4a6a9c2d6006131b5472c7e9a

Base: dev. Source-identical QA snapshot excludes the untracked Orca reference checkout. The git gate fingerprint matches the pushed source commit.

- pnpm check: exit0
- pnpm test: exit0;1955 tests in165 files
- pnpm build: exit0 (renderer and main)
- fvm flutter analyze: exit0
- fvm flutter test --no-pub --reporter json: exit0;248tests
- actual Electron pairing HTTP and History list/loadmore/detail/older/reopen: PASS on isolated19738 and fakeHOME
- Android APK: earlier build exit0, no live device journey
- iOS device install: blocked by Swift6.2 vs Firebase SDK13 tools6.2.1
- Android isolated AVD: native QEMU exit139; Maestro journey and MP4 UNVERIFIED

Screenshots use fixture conversations only. Co-Authored-By: Codex <noreply@openai.com>
