"""Small account-scoped client shared by Cloudflare operator commands."""

import json
import urllib.error
import urllib.request


class CloudflareApi:
    def __init__(self, env):
        for name in ("CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"):
            if not env.get(name):
                raise ValueError(f"{name} is required for Cloudflare API commands")
        self.account = env["CLOUDFLARE_ACCOUNT_ID"]
        self.token = env["CLOUDFLARE_API_TOKEN"]

    def request(self, method, path, body=None, missing=False):
        suffix = path if path == "/graphql" else f"/accounts/{self.account}{path}"
        request = urllib.request.Request(
            "https://api.cloudflare.com/client/v4" + suffix,
            method=method,
            headers={
                "Authorization": "Bearer " + self.token,
                "Content-Type": "application/json",
                "User-Agent": "brains-cloudflare-check",
            },
            data=None if body is None else json.dumps(body).encode(),
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                data = json.load(response)
        except urllib.error.HTTPError as error:
            if missing and error.code == 404:
                return None
            raise ValueError(f"Cloudflare {method} {path}: HTTP {error.code}; check resource permissions") from None
        if path == "/graphql":
            if data.get("errors"):
                raise ValueError("Cloudflare Analytics query failed; check Account Analytics Read permission")
            return data["data"]
        if not data.get("success"):
            raise ValueError(f"Cloudflare rejected {method} {path}")
        return data["result"]
