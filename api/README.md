# HTTP route conventions

Use `/` to separate resource names and actions: `/files/import/url`, `/resources/settings`, `/maintenance/archive/revision`. Fixed route segments use lowercase ASCII letters and digits. Do not join words with hyphens or underscores, repeat separators, or add a trailing slash. Template parameter names may use underscores inside braces.

These rules apply to API routing templates. User file names, file-path query values, header names, query parameter names and operation identifiers have their own contracts and are not renamed by this rule.

`api/brains.idl` is the route source. Update every handwritten caller and document when changing a route, then regenerate SDKs and OpenAPI with `scripts/gen-api.sh`. Do not keep an old route alias without an explicit compatibility requirement. Run `python3 scripts/check-api-paths.py`; CI rejects nonconforming IDL and generated OpenAPI paths.
