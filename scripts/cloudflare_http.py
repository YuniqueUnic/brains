"""Origin-pinned application verification; credentials never follow redirects."""

import urllib.parse
import urllib.request


def base_url(env, allow_loopback=False):
    if not env.get("BRAINS_BASE_URL"):
        raise ValueError("Configure BRAINS_BASE_URL before verification")
    base = env["BRAINS_BASE_URL"].rstrip("/")
    url = urllib.parse.urlsplit(base)
    local = allow_loopback and url.hostname in ("localhost", "127.0.0.1", "::1")
    if not url.hostname or not (
        url.scheme == "https" or url.scheme == "http" and local
    ):
        raise ValueError("BRAINS_BASE_URL must be an HTTPS origin")
    if url.username or url.password or url.query or url.fragment or url.path:
        raise ValueError(
            "BRAINS_BASE_URL must not contain credentials, a path, query or fragment"
        )
    return base


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None
