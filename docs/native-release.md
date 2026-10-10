# Brains native release

This archive contains `bsx` (CLI), `brains-server` (FS + SQLite runtime), this guide and `VERSION`. Choose the archive matching your operating system and CPU. Verify the adjacent SHA-256 file before extracting; macOS builds are not signed or notarized by this workflow. `bsx --version` reports the package version and Git commit; published artifacts must not carry `-dirty`.

## Run a local instance

Run from the extracted directory. Keep the data directory outside the release directory so an upgrade does not replace it.

```bash
./bsx --version
./brains-server --data-dir "$HOME/.brains/local-data" --port 45800
```

First startup generates and prints a root token; retain it securely. A subsequent startup reuses it. The default listener is loopback, and the vault is seeded with usage notes only when completely empty. For a new target intended for complete archive restoration, start with `--no-seed`. Do not delete an existing vault to create a migration target.

The native archive does not include the web app. To use the browser UI, extract the matching `brains-web-<version>.tar.gz` and restart the server with `--web-dist /absolute/path/to/dist`, keeping the same data directory. Open `http://127.0.0.1:45800` and log in with the instance token. Serving a public instance requires your own HTTPS reverse proxy and deployment configuration.

## Connect the CLI

Add this directory to PATH yourself, or continue using `./bsx`. Store the token in a private file (0600), then log in through standard input; do not place it in a shell argument or Git.

```bash
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
