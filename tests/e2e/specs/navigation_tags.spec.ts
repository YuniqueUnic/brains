import { describe, it } from "@serenity-js/playwright-test";
import { expect } from "@playwright/test";
import { CallBrainsApi } from "../screenplay/abilities/CallBrainsApi";
import { Instance } from "../screenplay/support/Instance";
import { OpenNote, SeedNoteViaApi } from "../screenplay/tasks/Navigation";
import { SignIn } from "../screenplay/tasks/Authoring";
import {
  ShowDocumentPreview,
  ShowSource,
} from "../screenplay/interactions/Preview";
import { PreviewArea } from "../screenplay/screens/PreviewArea";

describe("File navigation and composable tag search", () => {
  it("shares browser and toolbar history, survives reload and protects unsaved text", async ({
    actor,
    page,
  }) => {
    const dir = `web30-history-${Date.now()}`;
    const a = `${dir}/a.md`,
      b = `${dir}/有 空格.md`,
      c = `${dir}/c.md`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(a, "[阅读下一篇](./有%20空格.md)"),
      SeedNoteViaApi(b, "第二篇"),
      SeedNoteViaApi(c, "第三篇"),
      SignIn(),
      OpenNote(a),
      ShowDocumentPreview(),
    );
    await PreviewArea.document(page)
      .getByRole("link", { name: "阅读下一篇" })
      .click();
    await expect(page.getByTestId("editor-path")).toHaveText(b);
    expect(new URL(page.url()).searchParams.get("file")).toBe(b);
    await page.goBack();
    await expect(page.getByTestId("editor-path")).toHaveText(a);
    await page.getByTestId("file-forward").click();
    await expect(page.getByTestId("editor-path")).toHaveText(b);
    await page.reload();
    await expect(page.getByTestId("editor-path")).toHaveText(b);
    await actor.attemptsTo(ShowSource());
    await PreviewArea.editor(page).fill("第二篇还有未保存的修改");
    await page.goBack();
    await expect(page.getByTestId("open-confirm")).toBeVisible();
    await page
      .getByTestId("open-confirm")
      .getByRole("button", { name: "取消", exact: true })
      .click();
    await expect
      .poll(() => new URL(page.url()).searchParams.get("file"))
      .toBe(b);
    await expect(PreviewArea.editor(page)).toContainText("未保存的修改");
    await page.getByTestId("file-back").click();
    await expect(page.getByTestId("open-confirm")).toBeVisible();
    expect(new URL(page.url()).searchParams.get("file")).toBe(b);
    await page.reload();
    await expect(page.getByTestId("editor-path")).toHaveText(b);
    await expect(PreviewArea.editor(page)).toContainText("未保存的修改");
    await page.getByTestId("file-back").click();
    await page.getByTestId("open-confirm-yes").click();
    await expect(page.getByTestId("editor-path")).toHaveText(a);
    await page.getByTestId("file-forward").click();
    await expect(page.getByTestId("editor-path")).toHaveText(b);
    await expect(PreviewArea.editor(page)).toHaveText("第二篇");
    await page.getByTestId("file-back").click();
    await expect(page.getByTestId("editor-path")).toHaveText(a);
    await actor.attemptsTo(OpenNote(c));
    await expect(page.getByTestId("editor-path")).toHaveText(c);
    await expect(page.getByTestId("file-forward")).toBeDisabled();
    await page.goBack();
    await expect(page.getByTestId("editor-path")).toHaveText(a);
  });

  it("creates multiple tag chips from typing or paste, deletes with Backspace and searches their intersection", async ({
    actor,
    page,
  }, testInfo) => {
    const dir = `web30-tags-${Date.now()}`,
      unique = `photon-${Date.now()}`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(
        `${dir}/both.md`,
        `---\ntags: [agent-memory, ${unique}, 'a,b']\n---\nneedle body`,
      ),
      SeedNoteViaApi(
        `${dir}/only.md`,
        `---\ntags: [${unique}]\n---\nneedle body`,
      ),
      SeedNoteViaApi(
        `${dir}/other.md`,
        "---\ntags: [agent-memory]\n---\nneedle body",
      ),
      SignIn(),
      OpenNote(`${dir}/both.md`),
      ShowDocumentPreview(),
    );
    await PreviewArea.metadata(page)
      .getByRole("button", { name: `#${unique}`, exact: true })
      .click();
    const input = page.getByTestId("search-box"),
      chips = page.getByTestId("search-tag");
    await expect(chips).toHaveCount(1);
    await input.press("Backspace");
    await expect(chips).toHaveCount(0);
    await input.fill(`#${unique} #agent-memory `);
    await expect(chips).toHaveCount(2);
    await expect(page.getByTestId("search-hit")).toHaveCount(1);
    await expect(page.getByTestId("search-hit")).toContainText(
      `${dir}/both.md`,
    );
    await input.fill("#a,b");
    await input.press("Tab");
    await expect(chips).toHaveCount(3);
    await expect(chips.filter({ hasText: "agent-memory" })).toMatchAriaSnapshot(
      '- button "取消标签筛选：agent-memory": "#agent-memory"',
    );
    await expect(input).toBeFocused();
    await input.fill("#agent-memory");
    await input.press("Enter");
    await expect(chips).toHaveCount(3);
    await input.fill("needle");
    await input.press("Enter");
    await expect(page.getByTestId("search-hit")).toHaveCount(1);
    await expect(page.getByTestId("search-hit")).toContainText("needle body");
    await input.press("Backspace");
    await expect(chips).toHaveCount(3);
    await input.fill("");
    await input.press("Backspace");
    await expect(chips).toHaveCount(2);
    await chips.filter({ hasText: "agent-memory" }).click();
    await expect(chips).toHaveCount(1);
    await expect(input).toBeFocused();
    await expect(page.getByTestId("search-hit")).toHaveCount(2);
    await page.setViewportSize({ width: 390, height: 844 });
    await input.fill("#agent-memory #a,b ");
    await expect(chips).toHaveCount(3);
    await expect(page.getByTestId("search-hit")).toHaveCount(1);
    await expect(page.getByTestId("search-hit")).toContainText(`${dir}/both.md`);
    // Capture actual compact chip wrapping, independent of dynamic fixture names.
    await page.screenshot({ path: testInfo.outputPath("tag-chips-phone.png") });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
  });
});
