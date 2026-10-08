import { Ability } from "@serenity-js/core";

/**
 * Technical capability to talk to BrainStore's HTTP API directly — the
 * "second client" in conflict scenarios (design §8.3) and a fast path for
 * test-data setup. No business logic lives here. Access via
 * `actor.abilityTo(CallBrainStoreApi)`.
 */
export class CallBrainStoreApi extends Ability {
  constructor(private readonly baseUrl: string, private readonly token: string) {
    super();
  }

  static using(baseUrl: string, token: string): CallBrainStoreApi {
    return new CallBrainStoreApi(baseUrl, token);
  }

  async get(path: string): Promise<Response> {
    const route = path.startsWith("/") ? path : `/${path}`;
    return fetch(`${this.baseUrl}/api/v1${route}`, {
      headers: { Authorization: `Bearer ${this.token}` },
    });
  }

  async getText(path: string): Promise<{ revision: string; body: string }> {
    const response = await this.get(path);
    if (!response.ok) {
      throw new Error(`GET ${path} failed: HTTP ${response.status}`);
    }
    const revision = (response.headers.get("ETag") ?? "").replaceAll('"', "");
    return { revision, body: await response.text() };
  }

  async putText(path: string, body: string, revision?: string): Promise<Response> {
    return fetch(`${this.baseUrl}/api/v1/file?path=${encodeURIComponent(path)}`, {
      method: "PUT",
      headers: {
        Authorization: `Bearer ${this.token}`,
        "Content-Type": "text/markdown; charset=utf-8",
        ...(revision ? { "If-Match": `"${revision}"` } : { "If-None-Match": "*" }),
      },
      body,
    });
  }

  async createText(path: string, body: string): Promise<Response> {
    return this.putText(path, body);
  }

  async updateText(path: string, body: string, baseRevision: string): Promise<Response> {
    return this.putText(path, body, baseRevision);
  }
}
