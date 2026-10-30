"""Configure GitHub deployment inputs from the private local environment."""

import json
import subprocess

from cloudflare_config import structured_setting

SECRETS = (
    "CLOUDFLARE_API_TOKEN",
    "CLOUDFLARE_ACCOUNT_ID",
    "BRAINS_ADMIN_TOKEN",
    "TOKEN_REGISTRY_JSON",
    "SESSION_SIGNING_KEY",
    "CURSOR_SIGNING_KEY",
    "BRAINS_MCP_API_TOKEN",
    "BRAINS_MCP_ACCESS_TOKEN",
)
VARIABLES = (
    "BRAINS_WORKER_NAME",
    "BRAINS_R2_BUCKET",
    "BRAINS_D1_DATABASE",
    "BRAINS_D1_DATABASE_ID",
    "BRAINS_BASE_URL",
    "BRAINS_CUSTOM_DOMAIN",
    "BRAINS_CUSTOM_DOMAIN_ZONE_ID",
    "VAULT_ID",
    "MAX_TEXT_BYTES",
    "MAX_ATTACHMENT_BYTES",
    "ALLOWED_ORIGINS",
    "SEMANTIC_SEARCH",
    "BRAINS_AI_SEARCH_NAMESPACE",
    "BRAINS_RECONCILE_CRON",
    "BRAINS_R2_LIST_PAGE_MAX",
    "BRAINS_WORKER_CPU_MS",
    "BRAINS_WORKER_SUBREQUESTS",
    "BRAINS_WORKER_LOG_SAMPLE_RATE",
    "BRAINS_RESOURCE_ACCOUNT",
    "BRAINS_OBSERVATION_TTL_SECONDS",
    "BRAINS_MCP_WORKER_NAME",
    "BRAINS_MCP_HOSTNAME",
)


def gh(*args, value=None):
    result = subprocess.run(
        ["gh", *args], input=value, text=True, capture_output=True, check=True
    )
    return result.stdout


def configure(env, repository, environment):
    """Keep secrets in stdin; only public names are printed."""
    for key in SECRETS[:6]:
        if not env.get(key):
            raise ValueError(f"Configure {key} before syncing GitHub secrets")
    endpoint = f"repos/{repository}/environments/{environment}"
    try:
        configured = json.loads(gh("api", endpoint))
    except subprocess.CalledProcessError as error:
        if "HTTP 404" not in error.stderr:
            raise
        configured = json.loads(
            gh(
                "api",
                "--method",
                "PUT",
                endpoint,
                "--input",
                "-",
                value=json.dumps(
                    {
                        "deployment_branch_policy": {
                            "protected_branches": False,
                            "custom_branch_policies": True,
                        }
                    }
                ),
            )
        )
    if (configured.get("deployment_branch_policy") or {}).get("custom_branch_policies"):
        existing = json.loads(gh("api", endpoint + "/deployment-branch-policies"))
        if not any(
            p["name"] == "main" and p.get("type", "branch") == "branch"
            for p in existing["branch_policies"]
        ):
            gh(
                "api",
                "--method",
                "POST",
                endpoint + "/deployment-branch-policies",
                "--input",
                "-",
                value=json.dumps({"name": "main", "type": "branch"}),
            )
    for key in SECRETS:
        if env.get(key):
            gh(
                "secret",
                "set",
                key,
                "--repo",
                repository,
                "--env",
                environment,
                value=env[key],
            )
            print(f"Configured secret {key}")
    values = {key: env[key] for key in VARIABLES if key in env}
    for key in ("BRAINS_CLOUD_CONFIG", "BRAINS_RESOURCE_PROFILE", "BRAINS_RESOURCE_OBSERVATIONS"):
        setting = structured_setting(env, key)
        if setting is not None:
            values[key] = json.dumps(setting, separators=(",", ":"))
        elif key in env or key + "_FILE" in env:
            values[key] = ""
    existing_variables = json.loads(gh("api", endpoint + "/variables?per_page=100"))
    names = {item["name"] for item in existing_variables["variables"]}
    for key, value in values.items():
        if value == "":
            if key in names:
                gh("api", "--method", "DELETE", endpoint + "/variables/" + key)
            continue
        target = endpoint + "/variables" + ("/" + key if key in names else "")
        gh(
            "api",
            "--method",
            "PATCH" if key in names else "POST",
            target,
            "--input",
            "-",
            value=json.dumps({"name": key, "value": value}),
        )
        print(f"Configured variable {key}")
    print(
        f"GitHub environment {repository}/{environment} configured for main deployments."
    )
