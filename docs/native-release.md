# Brains native release

This archive contains `bsx` (CLI), `brains-server` (FS + SQLite runtime), this guide and `VERSION`. Choose the archive matching your operating system and CPU. Verify the adjacent SHA-256 file before extracting; macOS builds are not signed or notarized by this workflow. `bsx --version` and `brains-server --version` report the package version and Git commit in the same format; published artifacts must not carry `-dirty`. An authenticated `bsx capabilities --json` reports the connected instance’s `build_version`, which can differ from the CLI version.

The release `bsx` ships the full local capability set: FTS search, hybrid
ranking, document conversion (PDF/DOCX/XLSX/PPTX/HTML/CSV/plain text via the
embedded Xberg backend), URL import, `check`, working copy, archive and
sync. See [capabilities](capabilities.md) for the per-runtime matrix.

This guide describes the current source. Unified version reporting was added after v0.3.0; upgrade the CLI and server/Worker together to a release containing it. The older server does not return the `build_version` field required by the new CLI. Rebuilding the old tag does not add this feature.

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
`brains-server`, if one sits next to it) atomically. Non-interactive runs
must pass `--yes`.

## Run a local instance

Run from the extracted directory. Keep the data directory outside the release directory so an upgrade does not replace it.

```bash
./bsx --version
./brains-server --version
./brains-server --data-dir "$HOME/.brains/local-data" --port 45800
```

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

`import`/`export` default to current working-tree content. `--format archive` preserves complete reachable history and requires an empty destination or an exact prefix of that same archive. `sync` maintains current content across two named profiles; it is not a historical backup. `profile use cloud --verify-with local` verifies current content and rebuilds/checks the destination index before changing the active profile. Keep source data and backups until real deployment verification has passed.

## Shell completions

`bsx completion bash|zsh|fish|powershell|nushell` writes a completion script to stdout without configuration or network access. Generate one file for your shell and install it using your shell's normal completion mechanism. Nushell also accepts `nu` as an alias. This release does not modify your shell configuration.
