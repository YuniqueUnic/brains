import { ApiError, type SessionInfo } from "./client";

/** Synthetic credentials and sessions live only in this page; refresh resets them. */
const session: SessionInfo = { actor: "demo", scopes: ["read:/", "write:/", "admin:/"] };
let active = true;
let token = "brains-demo-example-token-not-a-real-secret";

export function getDemoSession(): SessionInfo {
  if (!active) throw new ApiError(401, "UNAUTHORIZED", "Enter the demo again to continue.");
  return session;
}

export function createDemoSession(candidate: string): SessionInfo {
  // The public demo keeps its account-free entry; nonempty credentials can also
  // exercise rejected-token and replacement-token behavior without a server.
  if (candidate && candidate !== token) {
    throw new ApiError(401, "UNAUTHORIZED", "The example token is no longer valid.");
  }
  active = true;
  return session;
}

export function endDemoSession(): void {
  active = false;
}

export function rotateDemoToken(current: string, replacement: string): void {
  getDemoSession();
  if (current !== token) throw new ApiError(403, "FORBIDDEN", "The current example token was rejected.");
  if (!/^[!-~]{32,4096}$/.test(replacement) || replacement === current) {
    throw new ApiError(400, "INVALID_ARGUMENT", "Use a different example token with 32–4096 printable ASCII characters and no spaces.");
  }
  token = replacement;
  endDemoSession();
}

export function demoSecurityExample(): { current: string; replacement: string } {
  return { current: token, replacement: `brains-demo-example-${crypto.randomUUID()}` };
}
