import { describe, it } from "@serenity-js/playwright-test";
import { expect } from "@playwright/test";
import { CallBrainsApi } from "../screenplay/abilities/CallBrainsApi";
import { SignIn } from "../screenplay/tasks/Authoring";
import { AnotherClientReplacesWithBinary, AnotherClientRewrites, OpenNote, SeedNoteViaApi, SeedRevisionHistory } from "../screenplay/tasks/Navigation";
import { holdFileResponse } from "../screenplay/support/hold-file-response";
import { Instance } from "../screenplay/support/Instance";

describe("on-demand file loading", () => {
  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    it(`reads one content snapshot and pages history only on demand at ${viewport.width}px`, async ({ actor, page }) => {
      await page.setViewportSize(viewport);
      actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
      const path = `loading-${viewport.width}-${Date.now()}.md`;
      await actor.attemptsTo(SeedRevisionHistory(path, 25), SignIn());
      const reads: URL[] = [];
      page.on("request", request => {
        const url = new URL(request.url());
        if (request.method() === "GET" && url.pathname.startsWith("/api/")) reads.push(url);
      });
      await actor.attemptsTo(OpenNote(path));
      await expect(page.locator(".cm-content")).toContainText("Revision 25");
      const fileReads = () => reads.filter(url => url.pathname === "/api/v1/file");
      const histories = () => reads.filter(url => url.pathname === "/api/v1/revisions");
      expect(fileReads().map(url => url.searchParams.get("format"))).toEqual([null]);
      expect(histories()).toHaveLength(0);
      if (viewport.width < 600) await page.getByRole("tab", { name: "信息", exact: true }).click();
      else await page.getByTestId("editor-pane").getByRole("button", { name: "属性", exact: true }).click();
      await expect(page.getByTestId("info-panel")).toBeVisible();
      await expect(page.getByTestId("history-toggle")).toHaveAttribute("aria-expanded", "false");
      expect(histories()).toHaveLength(0);
      await page.getByTestId("history-toggle").click();
      await expect(page.getByTestId("history-list").locator("li")).toHaveCount(20);
      expect(histories()).toHaveLength(1);
      await page.getByTestId("history-more").click();
      await expect(page.getByTestId("history-list").locator("li")).toHaveCount(25);
      expect(histories()).toHaveLength(2);
      await expect(page.getByTestId("history-more")).toHaveCount(0);
    });
  }

  it("keeps the local draft when a conflicting remote revision is binary", async ({ actor, page }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const path = `unreadable-conflict-${Date.now()}.md`;
    await actor.attemptsTo(SeedNoteViaApi(path, "Original text"), SignIn(), OpenNote(path));
    await expect(page.locator(".cm-content")).toContainText("Original text");
    await page.locator(".cm-content").fill("Local draft must survive");
    await actor.attemptsTo(AnotherClientReplacesWithBinary(path));
    await page.getByTestId("publish-button").click();
    await expect(page.getByTestId("conflict-view")).toBeVisible();
    await expect(page.getByTestId("conflict-take-remote")).toBeDisabled();
    await expect(page.getByTestId("conflict-view")).toContainText("Local draft must survive");
    const response = await CallBrainsApi.as(actor).get(`file?path=${encodeURIComponent(path)}`);
    expect(response.headers.get("X-Brain-Media-Type")).toBe("application/octet-stream");
    expect(new Uint8Array(await response.arrayBuffer())).toEqual(new TextEncoder().encode("Opaque remote payload"));
  });

  it("ignores a delayed conflict refresh after opening another note", async ({ actor, page }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const a = `late-conflict-a-${Date.now()}.md`, b = `late-conflict-b-${Date.now()}.md`;
    await actor.attemptsTo(SeedNoteViaApi(a, "Original A"), SeedNoteViaApi(b, "Original B"), SignIn(), OpenNote(a));
    await expect(page.locator(".cm-content")).toContainText("Original A");
    await page.locator(".cm-content").fill("Local A");
    await actor.attemptsTo(AnotherClientRewrites(a, "Remote A2"));
    await page.getByTestId("publish-button").click();
    await expect(page.getByTestId("conflict-view")).toBeVisible();
    await actor.attemptsTo(AnotherClientRewrites(a, "Remote A3"));
    const held = await holdFileResponse(page, a, "GET");
    try {
      await page.getByTestId("conflict-keep-local").click();
      await held.arrived;
      await actor.attemptsTo(OpenNote(b));
      await page.getByTestId("open-confirm").getByRole("button", { name: "放弃并打开", exact: true }).click();
      await expect(page.locator(".cm-content")).toContainText("Original B");
      await page.locator(".cm-content").fill("New draft in B");
      held.release();
      await held.finished();
      await expect(page.getByTestId("editor-path")).toHaveText(b);
      await expect(page.getByTestId("conflict-view")).toHaveCount(0);
      await expect(page.locator(".cm-content")).toContainText("New draft in B");
    } finally { held.release(); }
  });

  it("keeps typing made while a conflict resolution is being saved", async ({ actor, page }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const path = `buffered-conflict-${Date.now()}.md`;
    await actor.attemptsTo(SeedNoteViaApi(path, "Original"), SignIn(), OpenNote(path));
    await expect(page.locator(".cm-content")).toContainText("Original");
    await page.locator(".cm-content").fill("Submitted local text");
    await actor.attemptsTo(AnotherClientRewrites(path, "Remote text"));
    await page.getByTestId("publish-button").click();
    await expect(page.getByTestId("conflict-view")).toBeVisible();
    const held = await holdFileResponse(page, path, "PUT");
    try {
      await page.getByTestId("conflict-keep-local").click();
      await held.arrived;
      await page.getByTestId("conflict-view").locator('.cm-content[contenteditable="true"]').fill("Typing during save");
      held.release();
      await held.finished();
      await expect(page.getByTestId("conflict-view")).toHaveCount(0);
      await expect(page.locator(".cm-content")).toContainText("Typing during save");
      await expect(page.getByTestId("editor-status")).toHaveText("未发布草稿");
      const remote = await CallBrainsApi.as(actor).getText(`file?path=${encodeURIComponent(path)}`);
      expect(remote.body).toBe("Submitted local text");
    } finally { held.release(); }
  });

});
