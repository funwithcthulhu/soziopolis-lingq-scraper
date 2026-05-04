# Release Checklist

Use this checklist before publishing a Windows build of this repo.

## Before tagging

1. Update `Cargo.toml` version.
2. Update `CHANGELOG.md`.
3. Review `README.md` for any installer, GUI workflow, or storage changes.
4. Make sure the GitHub repo name, description, and topics still match what the tool actually is.

## Local verification

Run the core checks from the repository root:

```powershell
cargo test
cargo build --release
powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1
```

Optional portable build:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-portable.ps1 -NoDesktopShortcut
```

## Release packaging

1. Confirm `dist\SoziopolisReaderSetup-<version>.exe` exists.
2. Launch the release build once and spot-check:
   - `Browse Articles`
   - `My Library`
   - `LingQ Settings`
   - `Diagnostics`
3. Verify the installer opens and installs cleanly.

## GitHub release

1. Push the version commit and tag.
2. Create or update the GitHub release for `v<version>`.
3. Upload `SoziopolisReaderSetup-<version>.exe`.
4. Make sure the release title uses `Soziopolis Reader <version>`.
5. Keep the release notes plain and specific.

## Final GitHub polish

Check the GitHub repo page:

- repository name still fits the tool
- description explains Soziopolis + LingQ clearly
- topics are present
- release link works
- README renders cleanly

## Known intentional naming mismatch

The packaged app is `Soziopolis Reader`, but the internal storage path and Cargo package still use `soziopolis_lingq_tool` for upgrade compatibility. Keep that unless you are intentionally migrating existing installs.
