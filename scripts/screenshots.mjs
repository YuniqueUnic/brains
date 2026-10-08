/**
 * Regenerate the README screenshots against a running local server.
 * Usage: BRAIN_URL=http://127.0.0.1:8801 node scripts/screenshots.mjs
 * (run from tests/e2e/ so @playwright/test resolves, e.g.
 *  `cd tests/e2e && node ../../scripts/screenshots.mjs`)
 *
 * Seed the demo server (fresh data dir) before running:
 *   brainstore-server --data-dir /tmp/readme-demo --port 8801 \
 *     --token readme-demo-token-0000000000000000 --web-dist apps/web/dist
 *   put notes/rust-ci-cache.md     (frontmatter + sccache/mold + [[benchmark-notes]] link)
 *   put notes/benchmark-notes.md   ([[rust-ci-cache]] backlink + table)
 *   put notes/weekly/2026-W41.md   (mentions 缓存策略)
 *   put notes/inbox/idea.md
 */
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const BASE = process.env.BRAIN_URL ?? "http://127.0.0.1:8801";
const TOKEN = process.env.BRAIN_TOKEN ?? "readme-demo-token-0000000000000000";
// Output next to the repo's docs/, independent of the invocation cwd.
const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "docs", "screenshots");
mkdirSync(OUT, { recursive: true });

const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 1440, height: 860 },
  deviceScaleFactor: 2,
});

// 1. Login screen (logged out).
await page.goto(BASE);
await page.getByTestId("token-input").waitFor();
await page.screenshot({ path: `${OUT}/01-login.png` });

// 2. Sign in; open the seeded note (editor + info panel + sidebar).
await page.getByTestId("token-input").fill(TOKEN);
await page.getByTestId("login-button").click();
await page.getByTestId("file-list").waitFor();
await page.locator(".file-path", { hasText: "rust-ci-cache.md" }).first().click();
await page.waitForFunction(() =>
  document.querySelector('[data-testid="editor-status"]')?.textContent === "已保存",
);
// Let the info panel's history/links settle for the shot.
await page.waitForTimeout(600);
await page.screenshot({ path: `${OUT}/02-workspace.png` });

// 3. Search results.
await page.getByTestId("search-box").fill("缓存策略");
await page.keyboard.press("Enter");
await page.waitForSelector('[data-testid="search-results"] ul li');
await page.screenshot({ path: `${OUT}/03-search.png` });

// 4. Conflict view on a clean page (no search overlay): a rival client
//    advances HEAD behind the editor's back.
const RIVAL = {
  async put(path, text, base) {
    const meta = await fetch(`${BASE}/api/v1/file?path=${encodeURIComponent(path)}&format=meta`, {
      headers: { Authorization: `Bearer ${TOKEN}` },
    });
    const rev = base ?? (await meta.json()).revision;
    return fetch(`${BASE}/api/v1/file?path=${encodeURIComponent(path)}`, {
      method: "PUT",
      headers: {
        Authorization: `Bearer ${TOKEN}`,
        "Content-Type": "text/markdown",
        "If-Match": `"${rev}"`,
      },
      body: text,
    }).then((r) => r.status);
  },
};
await page.reload();
await page.getByTestId("file-list").waitFor();
await page.locator(".file-path", { hasText: "rust-ci-cache.md" }).first().click();
await page.waitForFunction(() =>
  document.querySelector('[data-testid="editor-status"]')?.textContent === "已保存",
);
await page.locator(".cm-content").fill("本地未保存的修改：把 sccache 换成 mold + cache 缓存");
// Rival commits a new revision.
await RIVAL.put("notes/rust-ci-cache.md", "同事已推送的远端版本：CI 缓存策略改为按分支分片。");
// Local save now 412s into the three-column conflict view.
await page.locator(".cm-content").press("ControlOrMeta+s");
await page.waitForSelector('[data-testid="conflict-view"]');
const columns = await page.evaluate(() =>
  [...document.querySelectorAll('[data-testid="conflict-view"] textarea')].map((t) => t.value.length),
);
if (columns[1] === 0 || columns[2] === 0) {
  throw new Error(`conflict columns unexpectedly empty: ${columns}`);
}
await page.screenshot({ path: `${OUT}/04-conflict.png` });

// 5. Dark theme (explicit toggle), same note open.
await page.evaluate(() => localStorage.setItem("brainstore-theme", "dark"));
await page.reload();
await page.getByTestId("file-list").waitFor();
await page.locator(".file-path", { hasText: "rust-ci-cache.md" }).first().click();
await page.waitForFunction(() =>
  document.querySelector('[data-testid="editor-status"]')?.textContent === "已保存",
);
await page.waitForTimeout(600);
await page.screenshot({ path: `${OUT}/05-dark.png` });

await browser.close();
console.log("screenshots written to", OUT);
