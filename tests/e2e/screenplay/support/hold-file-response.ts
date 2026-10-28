import type { Page } from "@playwright/test";

/** Hold one real response after the server has processed it, without sleeping. */
export async function holdFileResponse(page: Page, path: string, method: "GET" | "PUT") {
  let arrive!: () => void;
  let release!: () => void;
  const arrived = new Promise<void>(resolve => { arrive = resolve; });
  const gate = new Promise<void>(resolve => { release = resolve; });
  let held = false;
  const matches = (raw: string, verb: string) => {
    const url = new URL(raw);
    return verb === method && url.pathname === "/api/v1/file" &&
      url.searchParams.get("path") === path && !url.searchParams.has("format");
  };
  const response = page.waitForResponse(result => matches(result.url(), result.request().method()));
  await page.route("**/api/v1/file?**", async route => {
    if (held || !matches(route.request().url(), route.request().method())) return route.continue();
    held = true;
    const result = await route.fetch();
    arrive();
    await gate;
    await route.fulfill({ response: result });
  });
  return { arrived, release, finished: async () => (await response).finished() };
}
