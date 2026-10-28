# Brains native release

This archive contains `bsx` (CLI), `brains-server` (FS + SQLite runtime), this guide and `VERSION`. Choose the archive matching your operating system and CPU. Verify the adjacent SHA-256 file before extracting; macOS builds are not signed or notarized by this workflow. `bsx --version` and `brains-server --version` report the package version and Git commit in the same format; published artifacts must not carry `-dirty`. An authenticated `bsx capabilities --json` reports the connected instance’s `build_version`, which can differ from the CLI version.

| Platform | Archive |
| --- | --- |
| macOS Apple Silicon / Intel | `brains-<version>-macos-arm64.tar.gz` / `macos-x64.tar.gz` |
| Linux x64 / ARM64 (musl) | `brains-<version>-linux-x64.tar.gz` / `linux-arm64.tar.gz` |
| Windows x64 (MSVC) | `brains-<version>-windows-x64.zip` |

The release `bsx` ships the full local capability set: FTS search, hybrid
ranking, document conversion (PDF/DOCX/XLSX/PPTX/HTML/CSV/plain text via the
embedded Xberg backend), URL import, `check`, working copy, archive and
sync. See [capabilities](capabilities.md) for the per-runtime matrix.

Upgrade the CLI and the server/Worker it connects to together: the remote side must return the `build_version` field that `bsx capabilities --json` reports for the connected instance. Servers from before unified version reporting cannot serve a current CLI, and rebuilding an old tag does not add the field.

## Install and upgrade without compiling

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) installs the
release binaries straight from GitHub releases, for either package:

```bash
cargo binstall --git https://github.com/YuniqueUnic/brains brains-cli
cargo binstall --git https://github.com/YuniqueUnic/brains brains-server
```

An installed `bsx` upgrades itself the same way: `bsx update --check`
reports the newest release, `bsx update` downloads the platform archive,
verifies the published SHA-256, replaces the running binary (and a sibling
`brains-server`, if one sits next to it). Each binary is replaced separately; stop the server before updating on Windows. A replacement error is reported rather than claiming that both binaries were updated. Non-interactive runs must pass `--yes`.

## Run a local instance

Run from the extracted directory. Keep the data directory outside the release directory so an upgrade does not replace it.

```bash
./bsx --version
./brains-server --version
./brains-server --data-dir "$HOME/.brains/local-data" --port 45800
```

On Windows x64, install the [Microsoft Visual C++ x64 Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist) if it is not already present. The MSVC build uses the shared Microsoft C runtime. Extract the ZIP into a writable directory and use PowerShell:

```powershell
.\bsx.exe --version
.\brains-server.exe --data-dir "$env:USERPROFILE\.brains\local-data" --port 45800
```

The CLI stores profiles in `%USERPROFILE%\.brains\config.json`; `BRAIN_CONFIG` overrides that location. `bsx edit` uses `EDITOR`, or Notepad when it is unset. Logical vault paths always use `/`, including in PowerShell. Use a local NTFS data directory: immutable publication uses hard links on the same volume. Windows rejects device names, drive-qualified paths, alternate data streams, trailing dots/spaces and aliases with a different filename spelling instead of overwriting another note. Files are flushed before publication; parent-directory fsync is a Unix durability guarantee, not a Windows power-loss guarantee. Run as your normal user and keep the data directory private with Windows filesystem permissions.

First startup generates and prints a root token; retain it securely. A subsequent startup reuses it. The default listener is loopback, and the vault is seeded with usage notes only when completely empty. For a new target intended for complete archive restoration, start with `--no-seed`. Do not delete an existing vault to create a migration target.

The native archive does not include the web app. To use the browser UI, extract the matching `brains-web-<version>.tar.gz` and restart the server with `--web-dist /absolute/path/to/dist`, keeping the same data directory. Open `http://127.0.0.1:45800` and log in with the instance token. Serving a public instance requires your own HTTPS reverse proxy and deployment configuration.

## Connect the CLI

Keep the server running and open another terminal. Add this directory to PATH yourself, or continue using `./bsx`. Store the token in a private file (0600 on Unix), then log in through standard input; do not place it in a shell argument or Git.

```bash
unset BRAINS_URL BRAINS_TOKEN
./bsx profile add local --url http://127.0.0.1:45800
./bsx --profile local login --stdin-token < /absolute/private/local-token.txt
./bsx profile use local
./bsx --profile local whoami
./bsx --profile local ls
./bsx --help
```

In PowerShell, remove overrides and pipe the token from a private file:

```powershell
Remove-Item Env:BRAINS_URL, Env:BRAINS_TOKEN -ErrorAction SilentlyContinue
.\bsx.exe profile add local --url http://127.0.0.1:45800
Get-Content -Raw "$env:USERPROFILE\private\brains-token.txt" | .\bsx.exe --profile local login --stdin-token
.\bsx.exe --profile local whoami
```

Use Ctrl+C to stop the console server gracefully before moving or replacing its files. The release does not install a Windows service or change your PATH.

`import`/`export` default to current working-tree content. `--format archive` preserves complete reachable history and requires an empty destination or an exact prefix of that same archive. `sync` maintains current content across two named profiles; it is not a historical backup. `profile use cloud --verify-with local` verifies current content and rebuilds/checks the destination index before changing the active profile. Keep source data and backups until real deployment verification has passed.

## Shell completions

`bsx completion bash|zsh|fish|powershell|nushell` writes a completion script to stdout without configuration or network access. Generate one file for your shell and install it using your shell's normal completion mechanism. Nushell also accepts `nu` as an alias. This release does not modify your shell configuration.
