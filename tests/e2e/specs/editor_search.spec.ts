import { describe, it } from "@serenity-js/playwright-test";
import { expect } from "@playwright/test";
import { CallBrainsApi } from "../screenplay/abilities/CallBrainsApi";
import { Instance } from "../screenplay/support/Instance";
import { OpenNote, SeedNoteViaApi, SearchFor } from "../screenplay/tasks/Navigation";
import { SignIn } from "../screenplay/tasks/Authoring";
import { ShowDocumentPreview, ShowSource } from "../screenplay/interactions/Preview";
import { PreviewArea } from "../screenplay/screens/PreviewArea";

describe("Source editing and keyboard search", () => {
  for (const [style, frontmatter] of [
    ["block", "---\ntitle: Photon\ntags:\n  - photon\n  - 'a,b'\n  - ' rust ' # comment\n  - 42\n  - true\n---\n"],
    ["flow", "\uFEFF---\r\ntitle: Photon\r\ntags: [photon, 'a,b', ' rust ', 42, true]\r\n...\r\n"],
    ["document-end", "---\ntitle: Photon\ntags: [photon, 'a,b', ' rust ', 42, true]\n...\n"],
  ]) {
    it(`recognises ${style} YAML tags without changing the editable source`, async ({ actor, page }, testInfo) => {
      const path = `web31-${style}-${Date.now()}.md`;
      const text = frontmatter + "\n# 正文\n\n[文档](https://example.com)";
      actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
      await actor.attemptsTo(SeedNoteViaApi(path, text), SignIn(), OpenNote(path), ShowSource());
      const tags = page.locator(".cm-metadata-tag");
      await expect(tags).toHaveCount(3);
      expect(await tags.evaluateAll(nodes => nodes.map(node => node.getAttribute("data-tag")))).toEqual(["photon", "a,b", "rust"]);
      await tags.nth(1).hover();
      const searchTag = page.getByRole("button", { name: "搜索标签 #a,b", exact: true });
      await expect(searchTag).toBeVisible();
      await searchTag.click();
      await expect(page.getByTestId("search-tag")).toHaveText("#a,b");
      await expect(page.getByTestId("search-hit").filter({ hasText: path })).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(PreviewArea.editor(page)).toContainText("' rust '");
      await tags.first().click();
      await page.keyboard.press("Alt+Enter");
      await expect(page.getByTestId("search-tag").filter({ hasText: "#photon" })).toBeVisible();
      await page.keyboard.press("Escape");
      const heading = PreviewArea.line(page, "# 正文");
      expect(await heading.locator("span").last().evaluate(node =>
        parseFloat(getComputedStyle(node).fontSize) > parseFloat(getComputedStyle(node.parentElement!).fontSize),
      )).toBe(true);
      await PreviewArea.line(page, "[文档]").locator("span").first().hover();
      await expect(PreviewArea.card(page)).toBeVisible();
      await page.mouse.move(0, 0);
      await actor.attemptsTo(ShowDocumentPreview());
      await expect(PreviewArea.metadata(page)).toMatchAriaSnapshot(`
        - heading "Photon" [level=1]
        - button "#photon"
        - button "#a,b"
        - button "#rust"
      `);
      await actor.attemptsTo(ShowSource());
      await tags.first().dblclick();
      await tags.first().hover();
      await expect(page.getByRole("button", { name: "搜索标签 #photon", exact: true })).toBeVisible();
      await page.keyboard.type("renewed");
      await expect(page.getByRole("button", { name: "搜索标签 #photon", exact: true })).toHaveCount(0);
      await expect(tags.first()).toHaveAttribute("data-tag", "renewed");
      await tags.first().hover();
      await expect(page.getByRole("button", { name: "搜索标签 #renewed", exact: true })).toBeVisible();
      await page.keyboard.press("ControlOrMeta+z");
      await expect(tags.first()).toHaveAttribute("data-tag", "photon");
      await PreviewArea.editor(page).fill("---\ntags: [\"unfinished\n---\n正文");
      await expect(tags).toHaveCount(0);
      await page.keyboard.press("ControlOrMeta+z");
      await expect(tags).toHaveCount(3);
      // Opening a search or reading the metadata must never publish or rewrite bytes.
      const response = await page.request.get(`${Instance.baseUrl}/api/v1/file?path=${encodeURIComponent(path)}&view=raw`, { headers: Instance.bearerHeaders() });
      expect(Buffer.from(await response.body())).toEqual(Buffer.from(text));
      await testInfo.attach(`${style}-source`, { body: await page.screenshot(), contentType: "image/png" });
    });
  }

  it("reflows into the actual pane width and keeps overflow an explicit preference", async ({ actor, page }, testInfo) => {
    const path = `web31-width-${Date.now()}.md`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(SeedNoteViaApi(path, "当前可用宽度决定换行，侧栏收起后正文应变宽。".repeat(80)), SignIn(), OpenNote(path), ShowSource());
    const measure = () => PreviewArea.editor(page).evaluate(element => ({
      width: element.getBoundingClientRect().width,
      pane: element.closest('[data-testid="editor-pane"]')!.getBoundingClientRect().width,
      height: element.querySelector('.cm-line')!.getBoundingClientRect().height,
      scroll: element.parentElement!.scrollWidth,
      viewport: element.parentElement!.clientWidth,
    }));
    await page.getByRole("button", { name: "属性", exact: true }).click();
    const both = await measure();
    await page.keyboard.press("ControlOrMeta+i");
    await expect.poll(async () => (await measure()).width).toBeGreaterThan(both.width + 100);
    const left = await measure();
    await page.getByRole("button", { name: "折叠侧栏", exact: true }).click();
    await expect.poll(async () => (await measure()).width).toBeGreaterThan(left.width + 100);
    const full = await measure();
    expect(full.width).toBeGreaterThan(full.pane - 35);
    expect(full.height).toBeLessThan(both.height);
    await expect(page.getByTestId("account-button")).toHaveText("管理员");
    await expect(page.locator("header").first()).not.toContainText("token:");
    await page.getByRole("button", { name: "换行", exact: true }).click();
    await expect.poll(async () => (await measure()).scroll - (await measure()).viewport).toBeGreaterThan(200);
    await page.getByRole("button", { name: "溢出", exact: true }).click();
    await expect.poll(async () => (await measure()).scroll - (await measure()).viewport).toBeLessThan(5);
    await page.setViewportSize({ width: 1920, height: 1080 });
    await expect.poll(async () => (await measure()).width).toBeGreaterThan(1800);
    await testInfo.attach("wide-source", { body: await page.screenshot(), contentType: "image/png" });
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.getByTestId("account-button")).toBeVisible();
    await page.getByTestId("account-button").click();
    await expect(page.getByTestId("logout")).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(391);
    await testInfo.attach("mobile-account", { body: await page.screenshot(), contentType: "image/png" });
  });

  it("composes mode commands with tags and sends only the intended query", async ({ actor, page }, testInfo) => {
    const stamp = Date.now(), tag = `mode${stamp}`, path = `web31-command-${stamp}.md`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(SeedNoteViaApi(path, `---\ntags: [${tag}]\n---\nkeyboardsearch`), SignIn());
    await expect(page.getByTestId("palette-trigger")).toBeVisible();
    await page.keyboard.press("ControlOrMeta+k");
    const input = page.getByTestId("search-box");
    const sent: { q: string; mode: string; tags?: string[] }[] = [];
    page.on("request", request => { const url = new URL(request.url()); if (url.pathname.endsWith("/search")) sent.push(request.postDataJSON()); });
    await input.fill(`/fts #${tag} keyboardsearch`);
    await input.press("Enter");
    await expect(page.getByTestId("search-mode-command")).toHaveText("/fts");
    await expect(page.getByTestId("search-tag")).toHaveText(`#${tag}`);
    await expect(page.getByTestId("search-hit").filter({ hasText: path })).toBeVisible();
    expect(sent.length).toBeGreaterThan(0);
    for (const request of sent) expect(request).toMatchObject({ mode: "fts", q: "keyboardsearch" });
    for (const command of ["/text", "/fts", "/literal"]) {
      await input.fill(command);
      await input.press("Tab");
      await expect(input).toBeFocused();
      await expect(input).toHaveValue("");
      await expect(page.getByTestId("search-mode-command")).toHaveText(command === "/fts" ? "/fts" : "/text");
    }
    await input.fill("/fts-extra");
    await expect(input).toHaveValue("/fts-extra");
    await expect(page.getByTestId("search-mode-command")).toHaveText("/text");
    await input.fill("");
    await input.press("Backspace");
    await expect(page.getByTestId("search-tag")).toHaveCount(0);
    await testInfo.attach("mode-command", { body: await page.screenshot(), contentType: "image/png" });
    await page.keyboard.press("Escape");
    await actor.attemptsTo(SearchFor(`keyboardsearch #${tag} `, "literal"));
    await expect(page.getByTestId("search-mode-command")).toHaveText("/text");
    await expect(page.getByTestId("search-hit").filter({ hasText: path })).toBeVisible();
    const result = page.getByTestId("search-hit").filter({ hasText: path });
    await input.press("ArrowDown");
    await input.press("Home");
    // Home selects the first result through cmdk; Enter opens that result.
    await expect(result).toHaveAttribute("aria-selected", "true");
    await input.press("Enter");
    await expect(page.getByTestId("search-box")).toHaveCount(0);
    await expect(page.getByTestId("editor-path")).toHaveText(path);
  });

  it("waits for a complete mode command and the end of input-method composition", async ({ actor, page }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(SignIn());
    await expect(page.getByTestId("palette-trigger")).toBeVisible();
    await page.keyboard.press("ControlOrMeta+k");
    const input = page.getByTestId("search-box");
    await expect(input).toBeVisible();
    await page.clock.install();
    const sent: { q: string; mode: string; tags?: string[] }[] = [];
    page.on("request", request => { const url = new URL(request.url()); if (url.pathname.endsWith("/search")) sent.push(request.postDataJSON()); });
    await input.fill("keyboardsearch /fts");
    await page.clock.runFor(650);
    expect(sent).toHaveLength(0);
    await input.press("Tab");
    await expect(page.getByTestId("search-mode-command")).toHaveText("/fts");
    await expect(input).toHaveValue("keyboardsearch ");
    await page.clock.runFor(650);
    await expect.poll(() => sent.length).toBeGreaterThan(0);
    for (const request of sent) expect(request).toMatchObject({ mode: "fts", q: "keyboardsearch" });
    sent.length = 0;
    await input.dispatchEvent("compositionstart");
    await input.fill("zhongwen");
    await page.clock.runFor(650);
    expect(sent).toHaveLength(0);
    await input.fill("中文");
    await input.dispatchEvent("compositionend", { data: "中文" });
    await page.clock.runFor(650);
    await expect.poll(() => sent.length).toBeGreaterThan(0);
    for (const request of sent) expect(request.q).toBe("中文");
    // Closing an input mid-composition need not emit compositionend.
    await input.dispatchEvent("compositionstart");
    await page.keyboard.press("ControlOrMeta+k");
    await expect(input).toHaveCount(0);
    await page.keyboard.press("ControlOrMeta+k");
    await expect(input).toBeVisible();
    sent.length = 0;
    await input.fill("/text keyboardsearch");
    await page.clock.runFor(650);
    await expect.poll(() => sent.length).toBeGreaterThan(0);
    for (const request of sent) expect(request).toMatchObject({ mode: "literal", q: "keyboardsearch" });
  });

  it("publishes the exact source with the keyboard shortcut", async ({ actor, page }) => {
    const path = `web31-publish-${Date.now()}.md`, text = "---\ntags: [photon]\n---\n正文保持原样";
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(SeedNoteViaApi(path, "original"), SignIn(), OpenNote(path), ShowSource());
    const editor = PreviewArea.editor(page);
    await editor.fill(text);
    const published = page.waitForResponse(response => response.request().method() === "PUT" && new URL(response.url()).pathname.endsWith("/file"));
    await page.keyboard.press("ControlOrMeta+Enter");
    expect((await published).status()).toBe(200);
    await expect(page.getByTestId("publish-button")).toBeDisabled();
    const response = await page.request.get(`${Instance.baseUrl}/api/v1/file?path=${encodeURIComponent(path)}&view=raw`, { headers: Instance.bearerHeaders() });
    expect(await response.text()).toBe(text);
  });
});
