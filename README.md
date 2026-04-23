# Soziopolis LingQ Tool

A Rust-based desktop and CLI app for working with articles from `soziopolis.de`:

- browse Soziopolis sections
- extract clean full article text
- save articles into a local SQLite library
- upload saved articles to LingQ

## Getting Started

### 1. Clone the repo

```powershell
git clone <YOUR_GIT_URL_HERE>
cd soziopolis_lingq_tool
```

If you already downloaded the repo as a ZIP instead of cloning, extract it somewhere like:

`C:\projects\soziopolis_lingq_tool`

### 2. Install Rust

This app is built with Rust. On Windows, install Rust from `rustup` and then reopen PowerShell.

After installation, verify:

```powershell
rustc --version
cargo --version
```

### 3. Build and run the desktop app

From the project folder:

```powershell
cargo run
```

That launches the Soziopolis Reader GUI.

To build an optimized executable:

```powershell
cargo build --release
```

The main executable will be created at:

`target\release\soziopolis_lingq_tool.exe`

### 4. Use it to save Soziopolis articles and upload to LingQ

Basic first-run flow:

1. Launch the app with `cargo run` or the release executable.
2. Go to `Browse Articles`.
3. Pick a Soziopolis section and load articles.
4. Select the articles you want and click `Fetch & Save`.
5. Open `LingQ Settings` from the left sidebar and connect your LingQ account or token.
6. Go to `My Library`, choose the LingQ course/collection you want, select saved articles, and upload them.

The app keeps a local article library in SQLite, so after importing once you can browse, preview, filter, and upload later without re-fetching everything.

### 5. Portable Windows build

If you want a folder you can move to another Windows PC, run:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-portable.ps1
```

That refreshes the portable app folders and Desktop shortcut. The portable copy stores:

- the local article database
- settings
- queue and job history in SQLite
- logs
- diagnostics support bundles

On a new PC, LingQ will usually need to be reconnected once because the token is stored in Windows Credential Manager on each machine.

## Commands

```powershell
cargo run -- sections
cargo run -- browse --section essays --limit 15
cargo run -- browse-url --url https://www.soziopolis.de/texte/essay.html --limit 15
cargo run -- fetch --url https://www.soziopolis.de/die-themen-liegen-auf-der-strasse.html
cargo run -- fetch --url https://www.soziopolis.de/die-themen-liegen-auf-der-strasse.html --save
cargo run -- library --limit 20
cargo run -- upload --id 1 --api-key YOUR_LINGQ_API_KEY
cargo run -- --data-dir C:\soziopolis-data library --limit 20
```

You can also provide the LingQ token through `LINGQ_API_KEY`.
If you have already connected LingQ in the desktop app on Windows, the CLI can also reuse the
token stored in Windows Credential Manager.

## Storage

The SQLite database is created at:

`%LOCALAPPDATA%\soziopolis_lingq_tool\soziopolis_lingq_tool.db`

You can override the data directory for either the GUI or CLI with:

`--data-dir C:\path\to\your\data`

The app also supports a simple portable layout automatically. If the executable sits beside a
folder named `data` or `portable_data`, it will store settings and the SQLite database there
instead of `%LOCALAPPDATA%`. The expected portable structure is:

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
That means a portable folder carries the library and app settings, but LingQ will need to be
reconnected once on each new PC.

Queued import/upload jobs, recent job history, and retry lists are persisted inside the SQLite database.
The Diagnostics screen can also generate a timestamped support bundle folder with logs, settings,
an exported queue snapshot, and a diagnostic summary for troubleshooting.
Queue execution can be paused and resumed from Diagnostics, and you can force-start the next
queued LingQ upload without resuming the whole queue.

To refresh the portable folders and Desktop shortcut in one step, use:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-portable.ps1
```

## Notes

The scraper is tuned for Soziopolis article pages and section listings as they exist on April 16, 2026. If the site layout changes later, the scraping selectors may need a refresh.
