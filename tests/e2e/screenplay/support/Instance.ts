/** Target instance facts shared by abilities and tasks. */
export const Instance = {
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:18799",
  token: process.env.BRAIN_E2E_TOKEN ?? "e2e-root-token-0000000000000000",
  bearerHeaders(): Record<string, string> {
    return {
      Authorization: `Bearer ${this.token}`,
      "Content-Type": "text/markdown; charset=utf-8",
    };
  },
};
