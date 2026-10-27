#!/usr/bin/env python3
"""Run a command against an isolated local Worker (Python 3.11+, macOS/Linux)."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import ssl
import subprocess
import tempfile
import time
import tomllib
import urllib.error
import urllib.request


def stop_group(process):
    # The leader can exit before its children. Always signal the whole group.
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            break
        if sig == signal.SIGTERM:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                process.poll()
                try:
                    os.killpg(process.pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
    process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--protocol", choices=("http", "https"), default="http")
    parser.add_argument(
        "--resource-profile",
        type=Path,
        help="isolated usage profile; never reads the repository .env",
    )
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("pass the verification command after --")
    repo = Path(__file__).resolve().parent.parent
    worker = repo / "apps/worker"
    wrangler = worker / "node_modules/wrangler/bin/wrangler.js"
    config = tomllib.loads((worker / "wrangler.toml").read_text())
    main_file = (worker / config["main"]).resolve()
    assets = (worker / config["assets"]["directory"]).resolve()
    if (
        not main_file.is_file()
        or not (assets / "index.html").is_file()
        or not wrangler.is_file()
    ):
        parser.error(
            "install dependencies and build apps/web and apps/worker before running"
        )

    interrupted = 0

    def interrupt(signum, _frame):
        nonlocal interrupted
        # Do not raise between Popen and registration of its process group.
        interrupted = signum

    def check_interrupt():
        if interrupted:
            raise KeyboardInterrupt

    def wait(process, timeout=None):
        deadline = time.monotonic() + timeout if timeout is not None else None
        while process.poll() is None:
            check_interrupt()
            if deadline is not None and time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired(process.args, timeout)
            time.sleep(0.1)
        check_interrupt()
        return process.returncode

    previous_handlers = {
        sig: signal.signal(sig, interrupt) for sig in (signal.SIGINT, signal.SIGTERM)
    }
    result = 1
    try:
        with tempfile.TemporaryDirectory(prefix="brains-workerd-") as directory:
            root = Path(directory)
            processes = []
            try:
                # Config AND cwd are isolated: no checkout .env/.dev.vars or shared state.
                config.pop("build", None)
                config["main"] = str(main_file)
                config["assets"]["directory"] = str(assets)
                for database in config["d1_databases"]:
                    database["migrations_dir"] = str(
                        (worker / database["migrations_dir"]).resolve()
                    )
                    database["remote"] = False
                for bucket in config["r2_buckets"]:
                    bucket["remote"] = False
                if args.resource_profile:
                    from cloudflare_config import compose_services

                    compose_services(
                        config,
                        {
                            "BRAINS_RESOURCE_PROFILE_FILE": str(
                                args.resource_profile.resolve()
                            ),
                            "BRAINS_RESOURCE_ACCOUNT": "isolated-verification",
                        },
                    )
                (root / "wrangler.json").write_text(json.dumps(config))
                token = secrets.token_urlsafe(32)
                variables = {
                    "TOKEN_REGISTRY_JSON": json.dumps(
                        {
                            "tokens": [
                                {
                                    "id": "e2e",
                                    "name": "isolated verification",
                                    "enabled": True,
                                    "token_sha256": hashlib.sha256(
                                        token.encode()
                                    ).hexdigest(),
                                    "scopes": ["read:/", "write:/", "admin:/"],
                                }
                            ]
                        }
                    ),
                    "SESSION_SIGNING_KEY": secrets.token_hex(32),
                    "CURSOR_SIGNING_KEY": secrets.token_hex(32),
                }
                # dotenv strips quotes but does not decode JSON-escaped inner quotes.
                private = root / ".dev.vars"
                private.write_text(
                    "\n".join(f"{key}='{value}'" for key, value in variables.items())
                    + "\n"
                )
                private.chmod(0o600)
                env = {
                    key: value
                    for key, value in os.environ.items()
                    if key
                    not in (
                        "CLOUDFLARE_API_TOKEN",
                        "CLOUDFLARE_ACCOUNT_ID",
                        "CF_API_TOKEN",
                        "CF_ACCOUNT_ID",
                        "CLOUDFLARE_API_KEY",
                        "CLOUDFLARE_EMAIL",
                        "CF_API_KEY",
                        "CF_EMAIL",
                        "BRAINS_URL",
                        "BRAINS_TOKEN",
                        "NO_COLOR",
                    )
                }
                env.update(
                    CI="true",
                    WRANGLER_SEND_METRICS="false",
                    WRANGLER_LOG_PATH=str(root / "logs"),
                    CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV="false",
                )
                wrangler_command = [
                    "node",
                    str(wrangler),
                    "--config",
                    str(root / "wrangler.json"),
                ]
                persistence = ["--persist-to", str(root / "state")]
                check_interrupt()
                with (root / "migration.log").open("w") as log:
                    migration = subprocess.Popen(
                        wrangler_command
                        + [
                            "d1",
                            "migrations",
                            "apply",
                            "BRAINS_INDEX",
                            "--local",
                            *persistence,
                        ],
                        cwd=root,
                        env=env,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                        start_new_session=True,
                    )
                    processes.append(migration)
                    if wait(migration, 120):
                        raise RuntimeError("local D1 migration failed")
                # Reserve both distinct ports together, immediately before spawn.
                with socket.socket() as http, socket.socket() as inspector:
                    http.bind(("127.0.0.1", 0))
                    inspector.bind(("127.0.0.1", 0))
                    port, inspector_port = (
                        http.getsockname()[1],
                        inspector.getsockname()[1],
                    )
                base = f"{args.protocol}://127.0.0.1:{port}"
                env.update(BRAINS_BASE_URL=base, BRAINS_E2E_TOKEN=token)
                check_interrupt()
                with (root / "worker.log").open("w") as log:
                    server = subprocess.Popen(
                        wrangler_command
                        + [
                            "dev",
                            "--local",
                            "--ip",
                            "127.0.0.1",
                            "--port",
                            str(port),
                            "--inspector-port",
                            str(inspector_port),
                            "--local-protocol",
                            args.protocol,
                            *persistence,
                        ],
                        cwd=root,
                        env=env,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                        start_new_session=True,
                    )
                    processes.append(server)
                # Only this loopback probe accepts Wrangler's self-signed certificate.
                context = (
                    ssl._create_unverified_context()
                    if args.protocol == "https"
                    else None
                )

                def request(route, body=None):
                    request = urllib.request.Request(
                        base + "/api/v1" + route,
                        data=json.dumps(body).encode() if body is not None else None,
                        headers={
                            "Authorization": "Bearer " + token,
                            "Content-Type": "application/json",
                        },
                    )
                    with urllib.request.urlopen(
                        request, timeout=2, context=context
                    ) as response:
                        return json.load(response)

                deadline = time.monotonic() + 60
                while True:
                    check_interrupt()
                    if server.poll() is not None:
                        raise RuntimeError("workerd exited before readiness")
                    try:
                        caps = request("/capabilities")
                        if (
                            caps["runtime"] != "cloudflare-rust"
                            or caps["vault"] != config["vars"]["VAULT_ID"]
                        ):
                            raise RuntimeError("unexpected instance capabilities")
                        request("/session")
                        request("/files/query", {"source": "index", "limit": 1})
                        break
                    except (OSError, urllib.error.URLError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError("workerd did not become ready") from None
                        time.sleep(0.2)
                print(
                    f"isolated workerd ready at {base}; authenticated API and D1 verified",
                    flush=True,
                )
                child = subprocess.Popen(
                    command, cwd=repo, env=env, start_new_session=True
                )
                processes.append(child)
                result = wait(child)
            except KeyboardInterrupt:
                result = 128 + interrupted
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                print(f"workerd verification failed: {error}", flush=True)
            finally:
                for process in reversed(processes):
                    try:
                        stop_group(process)
                    except OSError as error:
                        result = 1
                        print(
                            f"failed to clean up verification process {process.pid}: {error}",
                            flush=True,
                        )
                if result and not interrupted:
                    for name in ("migration.log", "worker.log"):
                        path = root / name
                        if path.exists():
                            print(
                                f"{name}:\n"
                                + "\n".join(
                                    path.read_text(errors="replace").splitlines()[-30:]
                                )
                            )
    finally:
        for sig, previous in previous_handlers.items():
            signal.signal(sig, previous)
    return 128 + interrupted if interrupted else result if result >= 0 else 128 - result


if __name__ == "__main__":
    raise SystemExit(main())
