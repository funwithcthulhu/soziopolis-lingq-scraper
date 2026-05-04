# Soziopolis Reader

Soziopolis Reader is a Rust desktop app for Windows. It browses articles from `soziopolis.de`, saves them in a local SQLite library, and uploads selected articles to LingQ.

It is intentionally GUI-first and GUI-only: browsing, importing, review, diagnostics, and LingQ uploads all happen inside the desktop app.

## Highlights

- Browse Soziopolis sections and paginate through article listings
- Extract clean article text from individual pages
- Keep a searchable local SQLite library with filters, preview, and stats
- Upload saved articles to LingQ with stored credentials and collection selection
- Retry failed imports/uploads and manage a persisted job queue
- Build a portable folder or a normal Windows installer
- Generate diagnostics bundles with logs, settings, database files, and queue snapshots

## Download

The latest Windows build is published on GitHub:

- Releases: <https://github.com/funwithcthulhu/soziopolis-reader/releases>
- Latest installer: <https://github.com/funwithcthulhu/soziopolis-reader/releases/latest>

## Quick Start

### Install from GitHub Releases

1. Open the latest release page.
2. Download `SoziopolisReaderSetup-<version>.exe`.
3. Run the installer.
4. Launch `Soziopolis Reader` from Start Menu or the desktop shortcut if you enabled it.

### First-run flow

1. Open `Browse Articles`.
2. Choose a Soziopolis section and load articles.
3. Select the articles you want and click `Fetch & Save`.
4. Open `LingQ Settings` and connect your LingQ account or token.
5. Go to `My Library`, choose a LingQ collection, select saved articles, and upload them.

The local SQLite library keeps imported articles around, so you can preview, filter, and upload them later without fetching them again.

## Build From Source

### 1. Clone the repository

```powershell
git clone https://github.com/funwithcthulhu/soziopolis-reader.git
cd soziopolis-reader
```

If you downloaded a ZIP instead, extract it somewhere convenient such as:

`C:\projects\soziopolis_reader`

### 2. Install Rust

Install Rust with `rustup`, then reopen PowerShell and verify:

```powershell
rustc --version
cargo --version
```

### 3. Run the desktop app

```powershell
cargo run
```

This starts the desktop app.

### 4. Build an optimized executable

```powershell
cargo build --release
```

Cargo produces:

`target\release\soziopolis_lingq_tool.exe`

Packaged Windows builds rename that executable to `Soziopolis Reader.exe`.

## Windows Packaging

### Portable bundle

To refresh a portable folder build:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-portable.ps1 -NoDesktopShortcut
```

The portable copy stores:

- the local article database
- app settings
- queue and job history in SQLite
- logs
- diagnostics support bundles

On a new PC, LingQ usually needs to be reconnected once because the token lives in Windows Credential Manager for that machine.

### Installer build

To build a normal Windows installer, install [Inno Setup 6](https://jrsoftware.org/isinfo.php) and run:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1
```

That script:

- builds the release executable
- stages the installer files
- compiles `installer\SoziopolisReader.iss`
- writes `dist\SoziopolisReaderSetup-<version>.exe`

You can also point it at a specific compiler:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1 -IsccPath "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
```

## Storage

By default the SQLite database lives at:

`%LOCALAPPDATA%\soziopolis_lingq_tool\soziopolis_lingq_tool.db`

The app also supports a portable layout automatically. If the executable sits beside a folder named `data` or `portable_data`, it stores settings and the SQLite database there instead of `%LOCALAPPDATA%`.

Expected portable structure:

```text
Soziopolis Reader.exe
data/
  soziopolis_lingq_tool/
    settings.json
    soziopolis_lingq_tool.db
    logs/
      soziopolis-reader.log
    support_bundles/
      support-bundle-<timestamp>/
```

On Windows, LingQ tokens are stored in Windows Credential Manager rather than `settings.json`.

Queued import and upload jobs, recent job history, and retry lists are stored in SQLite. The `Diagnostics` screen can also build a timestamped support bundle with the current settings, logs, database files, queue snapshot, and recent task failures.

The internal storage folder keeps the historical `soziopolis_lingq_tool` name so existing installs and upgrades continue to find the same data.

If you want the app and its data in a custom location, use the portable layout instead of the default `%LOCALAPPDATA%` location.

## Scope and Assumptions

- The app is packaged and tested as a Windows desktop application.
- The scraper is tuned for Soziopolis article pages and section listings as they existed on April 16, 2026.
- If the Soziopolis site layout changes, the scraping selectors may need an update.

Additional project docs:

- [Architecture](docs/architecture.md)
- [Data Model](docs/data-model.md)
- [Reliability Notes](docs/reliability.md)
- [ADRs](docs/adr)
- [Release Checklist](docs/release-checklist.md)
