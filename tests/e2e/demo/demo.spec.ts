import { describe, it } from "@serenity-js/playwright-test";
import { expect } from "@playwright/test";
import { Click } from "@serenity-js/web";
import { CreateNote, RewriteNote } from "../screenplay/tasks/Authoring";
import { OpenNote, SearchFor, ToggleTrash } from "../screenplay/tasks/Navigation";
import { ShowDocumentPreview } from "../screenplay/interactions/Preview";
import { UploadFile } from "../screenplay/interactions/Upload";
import { EditorArea } from "../screenplay/screens/EditorArea";
import { Workbench } from "../screenplay/screens/Workbench";

const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64");

describe("Public browser demo", () => {
  it("opens seed links and copied URLs without a backend or credentials", async ({ actor, page, context }, testInfo) => {
    const backendRequests: string[] = [];
    const errors: string[] = [];
    page.on("request", request => { if (new URL(request.url()).pathname.startsWith("/api/")) backendRequests.push(request.url()); });
    page.on("pageerror", error => errors.push(error.message));
    await page.goto("./");
    await expect(page.getByTestId("demo-notice")).toMatchAriaSnapshot('- paragraph: 在线演示：文件仅保存在当前页面，刷新后重置。');
    await expect(page.getByTestId("token-input")).toHaveCount(0);
    await expect(page.getByTestId("github-link")).toHaveAttribute("href", "https://github.com/YuniqueUnic/brains");
    await actor.attemptsTo(OpenNote("notes/rust-ci-cache.md"));
    await page.getByRole("button", { name: "属性", exact: true }).click();
    await expect(page.getByTestId("info-panel")).toContainText("出链 1 · 反链 2");
    await actor.attemptsTo(ShowDocumentPreview());
    await page.getByTestId("markdown-preview").getByRole("link", { name: "benchmark-notes", exact: true }).click();
    await expect(page.getByTestId("editor-path")).toHaveText("notes/benchmark-notes.md");
    await page.getByTestId("file-back").click();
    await expect(page.getByTestId("editor-path")).toHaveText("notes/rust-ci-cache.md");
    await page.getByTestId("file-list").getByText("notes/rust-ci-cache.md", { exact: true }).click({ button: "right" });
    await page.getByRole("menuitem", { name: "复制网络路径", exact: true }).click();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(new URL(copied).pathname).toBe("/brains/");
    expect(new URL(copied).searchParams.get("file")).toBe("notes/rust-ci-cache.md");
    const other = await context.newPage();
    await other.goto(copied);
    await expect(other.getByTestId("editor-path")).toHaveText("notes/rust-ci-cache.md");
    await page.screenshot({ path: testInfo.outputPath("desktop-demo.png") });
    expect(backendRequests).toEqual([]);
    expect(errors).toEqual([]);
  });

  it("combines filename search, mode commands and block-style tags", async ({ actor, page }) => {
    await page.goto("./");
    await actor.attemptsTo(SearchFor("rust-ci-cache.md"));
    await expect(page.getByTestId("search-hit")).toHaveCount(1);
    await expect(page.getByTestId("search-hit")).toContainText("notes/rust-ci-cache.md");
    await page.keyboard.press("Escape");
    await actor.attemptsTo(CreateNote("demo/tagged.md", "---\ntitle: Demo tags\ntags:\n  - demo-search\n  - example\n---\nKeyboard searching works"));
    await expect(page.getByTestId("editor-status")).toHaveText("已发布");
    await actor.attemptsTo(SearchFor("#demo-search Keyboard", "fts"));
    await expect(page.getByTestId("search-hit")).toHaveCount(1);
    await expect(page.getByTestId("search-hit")).toContainText("demo/tagged.md");
    await expect(page.getByTestId("search-tag")).toHaveText("#demo-search");
  });

  it("preserves history while editing, trashing and restoring a note", async ({ actor, page }) => {
    await page.goto("./");
    await actor.attemptsTo(CreateNote("demo/recover.md", "第一版内容"));
    await expect(page.getByTestId("editor-status")).toHaveText("已发布");
    await actor.attemptsTo(RewriteNote("第二版内容"));
    await expect(page.getByTestId("editor-status")).toHaveText("已发布");
    await page.getByRole("button", { name: "属性", exact: true }).click();
    await page.getByTestId("history-toggle").click();
    await expect(page.getByTestId("history-list")).toContainText("gen 3");
    await page.getByTestId("restore-revision-gen-2").click();
    await page.getByTestId("restore-revision-confirm-yes").click();
    await expect(page.locator(".cm-content")).toContainText("第一版内容");
    await actor.attemptsTo(Click.on(Workbench.editorMenuButton()), Click.on(EditorArea.trashCurrentButton()), Click.on(Workbench.trashConfirmYes()), ToggleTrash());
    await actor.attemptsTo(Click.on(Workbench.restoreButtonFor("demo/recover.md")));
    await expect(page.getByTestId("file-list").getByText("demo/recover.md", { exact: true })).toHaveCount(0);
    await page.getByRole("button", { name: "所有文件", exact: true }).click();
    await actor.attemptsTo(OpenNote("demo/recover.md"));
    await expect(page.getByTestId("editor-path")).toHaveText("demo/recover.md");
    await expect(page.locator(".cm-content")).toContainText("第一版内容");
  });

  it("keeps uploaded PNG bytes intact through copying and trash recovery", async ({ actor, page }) => {
    await page.goto("./");
    await actor.attemptsTo(UploadFile("demo-image.png", "image/png", png));
    const imageRow = page.getByTestId("file-list").getByText("demo-image.png", { exact: true });
    await expect(imageRow).toBeVisible();
    await imageRow.click({ button: "right" });
    await page.getByRole("menuitem", { name: "创建副本", exact: true }).click();
    const copyRow = page.getByTestId("file-list").getByText("demo-image-copy.png", { exact: true });
    await expect(copyRow).toBeVisible();
    await actor.attemptsTo(CreateNote("demo/images.md", "# Uploaded images\n\n![[demo-image.png]]\n\n![[demo-image-copy.png]]"), ShowDocumentPreview());
    const images = page.getByTestId("markdown-preview").locator("img");
    await expect(images).toHaveCount(2);
    for (const image of await images.all()) {
      await expect.poll(() => image.evaluate(node => (node as HTMLImageElement).naturalWidth)).toBe(1);
      const bytes = await image.evaluate(async node => Array.from(new Uint8Array(await (await fetch((node as HTMLImageElement).src)).arrayBuffer())));
      expect(Buffer.from(bytes)).toEqual(png);
    }
    await copyRow.click({ button: "right" });
    await page.getByRole("menuitem", { name: "移入回收站", exact: true }).click();
    await page.getByTestId("trash-confirm-yes").click();
    await expect(copyRow).toHaveCount(0);
    await imageRow.click({ button: "right" });
    await page.getByRole("menuitem", { name: "创建副本", exact: true }).click();
    await expect(page.getByTestId("file-list").getByText("demo-image-copy-1.png", { exact: true })).toBeVisible();
    await actor.attemptsTo(ToggleTrash());
    await page.getByTestId("file-list").getByText("demo-image-copy.png", { exact: true }).click();
    const image = page.getByTestId("trash-viewer").getByRole("img");
    await expect.poll(() => image.evaluate(node => (node as HTMLImageElement).naturalWidth)).toBe(1);
    const bytes = await image.evaluate(async node => Array.from(new Uint8Array(await (await fetch((node as HTMLImageElement).src)).arrayBuffer())));
    expect(Buffer.from(bytes)).toEqual(png);
    await page.getByTestId("trash-viewer-restore").click();
    await expect(page.locator("[data-sonner-toast]").filter({ hasText: "已恢复" })).toBeVisible();
  });

  it("deduplicates resolved links and protects unpublished text during navigation", async ({ actor, page }) => {
    await page.goto("./");
    await actor.attemptsTo(CreateNote("demo/links.md", "[[rust-ci-cache]] and [same file](../notes/rust-ci-cache.md)"));
    await expect(page.getByTestId("editor-status")).toHaveText("已发布");
    await page.getByRole("button", { name: "属性", exact: true }).click();
    await expect(page.getByTestId("info-panel")).toContainText("出链 1 · 反链 0");
    await actor.attemptsTo(OpenNote("notes/rust-ci-cache.md"));
    await expect(page.getByTestId("info-panel")).toContainText("出链 1 · 反链 3");
    await expect(page.getByTestId("editor-status")).toHaveText("已发布");
    await page.locator(".cm-content").fill("未发布的草稿");
    await expect(page.getByTestId("editor-status")).toHaveText("未发布草稿");
    await actor.attemptsTo(OpenNote("notes/rust-ci-cache.md"));
    await expect(page.locator(".cm-content")).toHaveText("未发布的草稿");
    await page.getByTestId("file-back").click();
    await page.getByTestId("open-confirm").getByRole("button", { name: "取消", exact: true }).click();
    await expect(page.locator(".cm-content")).toHaveText("未发布的草稿");
    await page.getByTestId("file-back").click();
    await page.getByTestId("open-confirm-yes").click();
    await expect(page.getByTestId("editor-path")).toHaveText("demo/links.md");
    await page.getByTestId("file-forward").click();
    await expect(page.getByTestId("editor-path")).toHaveText("notes/rust-ci-cache.md");
    await expect(page.locator(".cm-content")).not.toContainText("未发布的草稿");
    await actor.attemptsTo(OpenNote("demo/links.md"), OpenNote("notes/rust-ci-cache.md"), ToggleTrash());
    await page.evaluate(() => history.go(-2));
    await expect(page.getByTestId("editor-path")).toHaveText("notes/rust-ci-cache.md");
    await expect(page.locator(".cm-content")).toBeVisible();
  });

  it("explains temporary storage on mobile and never asks visitors for a real token", async ({ page }, testInfo) => {
    const errors: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("./");
    await expect(page.getByTestId("demo-notice")).toBeVisible();
    await page.getByTestId("account-button").click();
    await page.getByTestId("logout").click();
    await expect(page.getByTestId("token-input")).toHaveCount(0);
    await expect(page.getByTestId("login-button")).toHaveText("进入演示");
    await page.getByTestId("login-button").click();
    await expect(page.getByTestId("demo-notice")).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(391);
    await page.screenshot({ path: testInfo.outputPath("mobile-demo.png") });
    expect(errors).toEqual([]);
  });
});
