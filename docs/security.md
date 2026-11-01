# Access tokens and browser sessions

Brains authenticates remote clients with bearer tokens. Browsers exchange a token for an HttpOnly signed cookie and a CSRF token. Local-mode libraries use filesystem permissions instead of HTTP credentials. Token rotation and session revocation operate on the authenticated token identity and preserve its scopes; they never change vault files or revision history.

## Rotate a token with bsx

Select a stored remote profile and pass the replacement token through stdin. Use a password manager or another secure input source; do not put either credential in shell arguments or shell history. Replacement tokens must contain 32–4096 printable ASCII characters without whitespace and differ from existing credentials.

```sh
# Supply the replacement token through the command's stdin, for example from
# a password manager. The CLI does not print it or accept it as an argument.
bsx --profile my-server auth rotate-token --stdin-token

# End all browser sessions without replacing the bearer token.
bsx --profile my-server auth logout-all
```

Rotation rejects the previous bearer token and all browser sessions for that token, including the browser requesting the change. After the server confirms success, bsx updates only the selected profile using its private atomic config writer. It preserves other profiles and the active-profile selection, and refuses to overwrite a selected profile that changed while the request was in flight. Clear `BRAINS_URL` and `BRAINS_TOKEN` overrides before using these commands so their target matches the profile being saved. `BRAIN_CONFIG` can select an isolated config file.

Keep the replacement token securely before sending the request. If the response is lost, or the server rotates successfully but the local config cannot be saved, sign in using the replacement token and use `bsx --profile my-server login --stdin-token` to repair the profile. An HTTP error cannot prove a mutation was never committed. Update other clients and any authenticated deployment smoke-check credentials separately; rotation cannot rewrite their private configuration.

## Web controls

Open the account menu → Token & sessions to go directly to the security controls, or open Settings → Security. Changing a token requires the current token, matching replacement fields, and confirmation that the replacement has been saved. Signing out all sessions has a separate confirmation. Save unsaved edits before either action; success returns to the sign-in screen. Tokens remain in password fields only until completion or leaving the settings surface, and are never placed in localStorage, URLs, or responses.

The public browser demo exposes the same controls with page-local synthetic credentials. Select Fill example values to try rotation without entering a real token. Rotation, session revocation, and ordinary logout return to the demo entry screen; Enter demo resumes the page without clearing saved demo files. Other tabs and real accounts are unaffected, and refreshing resets both the example credential and demo files.

The regular Log out control revokes the current cookie on the server before clearing it in the browser. Replaying a copied cookie after logout fails. Signing out all sessions invalidates every existing cookie for the token while leaving bearer clients working. Other token identities and their sessions remain unchanged. Failed logout stays visible and can be retried; it does not claim that the server revoked a session when the request failed.

## API and persistence

All routes require authentication. Cookie mutations also require the session CSRF token and an allowed Origin. `POST /api/v1/token/rotate` accepts `current_token` and `replacement_token`; knowing a browser cookie alone cannot change the bearer credential. `POST /api/v1/session/revoke` ends all sessions for the caller; `DELETE /api/v1/session` ends the current cookie session. Successful mutations return `{ "ok": true }`, clear the browser cookie, and never return a secret. The generated API/SDK artifacts are derived from `api/brains.idl`.

The shared registry owns token hashes, a random session generation for each identity, and revoked cookie nonces retained until their eight-hour absolute expiry. Rotation changes the hash and generation together. Sign-out-all changes only the generation. Current logout records the cookie nonce. Each security mutation rechecks the presented credential against the state it updates, so stale requests cannot update a newer credential state. Native writes hold the registry lock, publish the private file atomically, and change the in-memory registry only after persistence succeeds. Cloudflare writes use a conditional revision update; concurrent changes return `412 REVISION_CONFLICT` and must be reauthenticated/retried.

Native servers keep active state in `data_dir/auth.json`. The server's `--token`/`BRAINS_TOKEN` option initializes an empty owner registry; restarting with a previous bootstrap token does not replace an existing enabled owner credential. Tokens created through rotation are stored only as hashes, and their plaintext cannot be recovered from the registry.

Cloudflare stores active state in the singleton `security_state` D1 table. Apply `migrations/d1/0006_security_state.sql` before deploying the new Worker. The first successful security mutation initializes that row from `TOKEN_REGISTRY_JSON`. Once the row exists, its known identities are authoritative across requests and code deployments: deployment secrets do not reset their active tokens, scopes, or revocations. New identity IDs absent from persisted state can still be introduced from bootstrap configuration, including the first MCP reader. Keep this table when rebuilding the derived file index and include it in database backups. Missing migrations, unavailable storage, or corrupt state fail closed; they never fall back to bootstrap credentials. Authenticated security database receipts are included in resource accounting. Rejected authentication requires a bounded primary-key D1 lookup but does not enter the application ledger or call business providers; provider billing for these unauthenticated lookups is outside the application report. Self-service security actions remain available to every authenticated principal when business-operation budgets are exhausted.

## Operator recovery

Operators with filesystem or Cloudflare control-plane access can recover an inaccessible instance; a public recovery endpoint does not exist. Stop the native server before editing its private registry. Replace the intended token hash, clear its plaintext, and change its `session_epoch` to a fresh random value; retain unrelated principals and vault data. Restart and verify that the replacement works and previous credentials fail. Editing the registry without changing the epoch would leave its signed sessions valid.

For Cloudflare, back up the database and current bootstrap settings first. Set a fresh, private bootstrap registry and rotate `SESSION_SIGNING_KEY`, then deliberately remove the singleton `security_state` row through the authenticated control plane. The next request uses the new bootstrap registry. This explicit reset replaces runtime security state, so retain every principal that should survive and verify old tokens/cookies are rejected. Do not merely change `TOKEN_REGISTRY_JSON` after runtime state has been initialized. Never expose registry contents or signing keys in terminal output, issue text, PRs, or logs.

Server-side logout and credential-change reauthentication follow the [OWASP Session Management Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html). Cloudflare control-plane secret updates are described in the [Workers secrets documentation](https://developers.cloudflare.com/workers/configuration/secrets/).
