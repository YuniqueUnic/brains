import { describe, it } from "@serenity-js/playwright-test";
import { expect, type Page } from "@playwright/test";
import { Click, Press } from "@serenity-js/web";
import { CallBrainsApi } from "../screenplay/abilities/CallBrainsApi";
import { Instance } from "../screenplay/support/Instance";
import {
  SeedNoteViaApi,
  SeedImageViaApi,
  AnotherClientRewrites,
  OpenNote,
  ToggleTrash,
} from "../screenplay/tasks/Navigation";
import { SignIn } from "../screenplay/tasks/Authoring";
import { PreviewArea } from "../screenplay/screens/PreviewArea";
import { Workbench } from "../screenplay/screens/Workbench";
import {
  HoverReference,
  LeavePreview,
  ResizePreview,
  ShowDocumentPreview,
  ShowSource,
} from "../screenplay/interactions/Preview";

const source = `---
title: 可阅读、可连接的知识库
description: |
  把笔记里的线索连起来。
  在本地整理，与团队共享。
tags: [设计, 'a,b']
links:
  关联笔记: ./target.md
  官方文档: https://example.com/?q=a|b
---

## 正文

这是正文；属性和正文各有清楚的位置。

[关联笔记](./target.md)

![示意图](./diagram.svg)

[带空格](<./有 空格.md> "说明")

[[target|别名]]

[参考链接][ref]

[ref]: ./target.md

\`[代码中的假链接](./target.md)\`

[页内跳转](#正文)
`;

const metadataSnapshot = `
- heading "可阅读、可连接的知识库" [level=1]
- paragraph: 把笔记里的线索连起来。 在本地整理，与团队共享。
- button "#设计"
- button "#a,b"
- list "链接":
  - listitem:
    - link "关联笔记"
  - listitem:
    - link "官方文档"
`;

// Each scenario owns its vault paths. No production data or external website is needed.
describe("Markdown metadata and reference previews", () => {
  const browserErrors = new WeakMap<Page, string[]>();
  it.beforeEach(async ({ page }) => {
    const errors: string[] = [];
    browserErrors.set(page, errors);
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("console", (message) => {
      if (
        message.type() === "error" &&
        !message.text().includes("401 (Unauthorized)")
      )
        errors.push(message.text());
    });
  });
  it.afterEach(async ({ page }) => {
    expect(browserErrors.get(page)).toEqual([]);
  });

  it("keeps a trashed image readable from its last contentful revision", async ({
    actor,
    page,
  }) => {
    const path = `web29-trash-${Date.now()}/diagram.svg`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(SeedImageViaApi(path), SignIn(), OpenNote(path));
    // Binary files stay in the file tree rather than the text editor.
    await expect(page.getByTestId("banner")).toContainText(path);
    await actor.attemptsTo(
      Press.the("Backspace").in(Workbench.fileItem(path)),
      Click.on(Workbench.trashConfirmYes()),
    );
    await expect(
      page.getByTestId("file-list").getByText(path, { exact: true }),
    ).toBeHidden();
    await actor.attemptsTo(ToggleTrash(), OpenNote(path));
    const image = page
      .getByTestId("trash-viewer")
      .getByRole("img", { name: path });
    await expect
      .poll(() =>
        image.evaluate((img) => (img as HTMLImageElement).naturalWidth),
      )
      .toBe(640);
  });

  it("renders properties, local images and precise tag filtering, then follows a metadata link", async ({
    actor,
    page,
  }, testInfo) => {
    const dir = `web29-${Date.now()}`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(`${dir}/source.md`, source),
      SeedNoteViaApi(
        `${dir}/target.md`,
        "---\ntitle: 关联笔记\ntags: 'a,b'\n---\n\n目标正文",
      ),
      SeedNoteViaApi(`${dir}/other.md`, "---\ntags: [a, b]\n---\n不应匹配"),
    );
    await actor.attemptsTo(
      SeedImageViaApi(`${dir}/diagram.svg`),
      SignIn(),
      OpenNote(`${dir}/source.md`),
      ShowDocumentPreview(),
    );
    const preview = PreviewArea.document(page);
    await expect(PreviewArea.metadata(page).getByRole("heading")).toHaveText(
      "可阅读、可连接的知识库",
    );
    await expect(preview).not.toContainText("description:");
    await expect(
      preview.getByRole("link", { name: "官方文档" }),
    ).toHaveAttribute("href", "https://example.com/?q=a|b");
    await expect
      .poll(() =>
        preview
          .getByAltText("示意图")
          .evaluate((img) => (img as HTMLImageElement).naturalWidth),
      )
      .toBe(640);
    await expect(PreviewArea.metadata(page)).toMatchAriaSnapshot(
      metadataSnapshot,
    );
    await testInfo.attach("metadata-desktop", {
      body: await preview.screenshot(),
      contentType: "image/png",
    });
    await preview.getByRole("button", { name: "#a,b", exact: true }).click();
    await page.getByTestId("search-box").fill(dir);
    await expect(page.getByTestId("search-results")).toContainText("2 个命中");
    await expect(page.getByTestId("search-hit")).toHaveCount(2);
    await page.keyboard.press("Escape");
    await PreviewArea.metadata(page)
      .getByRole("link", { name: "关联笔记", exact: true })
      .hover();
    await expect(PreviewArea.card(page)).toContainText("目标正文");
    const initialCard = (await PreviewArea.card(page).boundingBox())!;
    await actor.attemptsTo(ResizePreview(80, 40));
    await expect
      .poll(async () => (await PreviewArea.card(page).boundingBox())!.width)
      .toBeGreaterThan(initialCard.width + 60);
    await expect(page.getByTestId("editor-path")).toHaveText(
      `${dir}/source.md`,
    );
    const expanded = (await PreviewArea.card(page).boundingBox())!;
    const corner = (await PreviewArea.resize(page).getAttribute("data-corner"))!;
    await actor.attemptsTo(ResizePreview(-60, -40));
    const shrunk = (await PreviewArea.card(page).boundingBox())!;
    expect(shrunk.width).toBeCloseTo(expanded.width - 60, 0);
    expect(shrunk.height).toBeCloseTo(expanded.height - 40, 0);
    expect(corner.endsWith("w") ? shrunk.x + shrunk.width : shrunk.x).toBeCloseTo(corner.endsWith("w") ? expanded.x + expanded.width : expanded.x, 0);
    expect(corner.startsWith("n") ? shrunk.y + shrunk.height : shrunk.y).toBeCloseTo(corner.startsWith("n") ? expanded.y + expanded.height : expanded.y, 0);
    await PreviewArea.card(page)
      .getByRole("button", { name: "打开", exact: true })
      .click();
    await expect(page.getByTestId("editor-path")).toHaveText(
      `${dir}/target.md`,
    );
  });

  it("previews complete link syntax, image markers and reference definitions without previewing code", async ({
    actor,
    page,
  }) => {
    const dir = `web29-links-${Date.now()}`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(`${dir}/source.md`, source),
      SeedNoteViaApi(
        `${dir}/target.md`,
        "---\ntitle: 悬停目标\n---\n\n内容来自正确的文档",
      ),
      SeedNoteViaApi(`${dir}/有 空格.md`, "# 空格目标"),
      SeedImageViaApi(`${dir}/diagram.svg`),
      SignIn(),
      OpenNote(`${dir}/source.md`),
    );
    for (const expression of [
      "[关联笔记](./target.md)",
      "[[target|别名]]",
      "[参考链接][ref]",
    ]) {
      await actor.attemptsTo(HoverReference(expression, 0));
      await expect(PreviewArea.card(page)).toContainText("悬停目标");
      await actor.attemptsTo(LeavePreview());
      await expect(PreviewArea.card(page)).toBeHidden();
    }
    await actor.attemptsTo(HoverReference("![示意图](./diagram.svg)", 0));
    await expect
      .poll(() =>
        PreviewArea.card(page)
          .locator("img")
          .evaluate((img) => (img as HTMLImageElement).naturalWidth),
      )
      .toBe(640);
    await actor.attemptsTo(LeavePreview());
    await actor.attemptsTo(
      HoverReference('[带空格](<./有 空格.md> "说明")', 2),
    );
    await expect(PreviewArea.card(page)).toContainText("空格目标");
    await actor.attemptsTo(
      LeavePreview(),
      HoverReference("`[代码中的假链接](./target.md)`", 3),
    );
    // A negative hover must remain absent after the documented 350 ms open delay.
    await page.waitForTimeout(500);
    await expect(PreviewArea.card(page)).toBeHidden();
  });

  it("shows a loading card before delayed content arrives and keeps it when the pointer moves within the link", async ({
    actor,
    page,
  }) => {
    const dir = `web29-slow-${Date.now()}`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(`${dir}/source.md`, "[慢速加载的链接](./target.md)"),
      SeedNoteViaApi(`${dir}/target.md`, "# 加载完成"),
      SignIn(),
      OpenNote(`${dir}/source.md`),
    );
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route(
      (url) =>
        url.pathname === "/api/v1/file" &&
        url.searchParams.get("path") === `${dir}/target.md`,
      async (route) => {
        await gate;
        await route.continue();
      },
    );
    await actor.attemptsTo(HoverReference("[慢速加载的链接](./target.md)", 1));
    await expect(PreviewArea.card(page)).toContainText("预览加载中");
    await actor.attemptsTo(HoverReference("[慢速加载的链接](./target.md)", 5));
    release();
    await expect(PreviewArea.card(page)).toContainText("加载完成");
  });

  it("resizes a card without navigating and remembers its dimensions across reload", async ({
    actor,
    page,
  }) => {
    const dir = `web29-size-${Date.now()}`;
    const expression = "[调整大小](./target.md)";
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(`${dir}/source.md`, `# 尺寸\n\n${expression}`),
      SeedNoteViaApi(
        `${dir}/target.md`,
        "# 可调整的预览\n\n" + "正文内容\n\n".repeat(30),
      ),
      SignIn(),
      OpenNote(`${dir}/source.md`),
      HoverReference(expression),
    );
    await expect(PreviewArea.card(page)).toContainText("可调整的预览");
    const before = (await PreviewArea.card(page).boundingBox())!;
    await actor.attemptsTo(ResizePreview(100, 70));
    await expect
      .poll(async () => (await PreviewArea.card(page).boundingBox())!.width)
      .toBeCloseTo(before.width + 100, 0);
    await expect(page.getByTestId("editor-path")).toHaveText(
      `${dir}/source.md`,
    );
    const saved = await page.evaluate(() =>
      JSON.parse(localStorage.getItem("brains.preview.size")!),
    );
    await page.reload();
    await actor.attemptsTo(
      OpenNote(`${dir}/source.md`),
      HoverReference(expression),
    );
    await expect
      .poll(async () => (await PreviewArea.card(page).boundingBox())?.width)
      .toBeCloseTo(saved.width, 0);
    await expect
      .poll(async () => (await PreviewArea.card(page).boundingBox())?.height)
      .toBeCloseTo(saved.height, 0);
  });

  it("keeps malformed metadata visible and does not reopen a dirty document for an in-page link", async ({
    actor,
    page,
  }) => {
    const dir = `web29-invalid-${Date.now()}`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(
        `${dir}/source.md`,
        "---\ntags: [broken\n---\n\n## 正文\n\n[返回正文](#正文)",
      ),
      SignIn(),
      OpenNote(`${dir}/source.md`),
    );
    await PreviewArea.editor(page).fill(
      "---\ntags: [broken\n---\n\n## 正文\n\n[返回正文](#正文)\n\n未发布",
    );
    await actor.attemptsTo(ShowDocumentPreview());
    await expect(PreviewArea.document(page)).toContainText("元信息格式有误");
    await expect(PreviewArea.document(page)).toContainText("tags: [broken");
    await PreviewArea.document(page)
      .getByRole("link", { name: "返回正文" })
      .click();
    await expect(page.getByRole("alertdialog")).toBeHidden();
    await expect(page.getByTestId("editor-status")).toContainText("未发布");
    await actor.attemptsTo(ShowSource());
    await expect(PreviewArea.editor(page)).toContainText("未发布");
  });

  it("chooses all four resize corners and constrains small windows without replacing the preferred size", async ({
    actor,
    page,
  }) => {
    const dir = `web29-corners-${Date.now()}`;
    const expressions = [
      "[左上](./target.md)",
      "[右上](./target.md)",
      "[左下](./target.md)",
      "[右下](./target.md)",
    ];
    await page.addInitScript(() =>
      localStorage.setItem("brains-editor", JSON.stringify({ wrap: false })),
    );
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(
        `${dir}/source.md`,
        expressions[0] +
          "\n\n" +
          "a".repeat(120) +
          " " +
          expressions[1] +
          "\n\n" +
          "普通正文\n\n".repeat(7) +
          expressions[2] +
          "\n\n" +
          "a".repeat(120) +
          " " +
          expressions[3],
      ),
      SeedNoteViaApi(
        `${dir}/target.md`,
        "# 四角预览\n\n" + "可滚动正文\n\n".repeat(30),
      ),
      SignIn(),
      OpenNote(`${dir}/source.md`),
    );
    const corners = ["se", "sw", "ne", "nw"];
    for (const [i, expression] of expressions.entries()) {
      await actor.attemptsTo(HoverReference(expression, 1));
      await expect(PreviewArea.card(page)).toContainText("四角预览");
      await expect(PreviewArea.resize(page)).toHaveAttribute(
        "data-corner",
        corners[i],
      );
      const before = (await PreviewArea.card(page).boundingBox())!;
      for (const [dw, dh] of [[36, 20], [-70, -55], [34, 35]]) {
        const initial = (await PreviewArea.card(page).boundingBox())!;
        await actor.attemptsTo(ResizePreview(dw, dh));
        await expect.poll(async () => (await PreviewArea.card(page).boundingBox())!.width).toBeCloseTo(initial.width + dw, 0);
        await expect.poll(async () => (await PreviewArea.card(page).boundingBox())!.height).toBeCloseTo(initial.height + dh, 0);
        const after = (await PreviewArea.card(page).boundingBox())!;
        expect(corners[i].endsWith("w") ? after.x + after.width : after.x).toBeCloseTo(corners[i].endsWith("w") ? initial.x + initial.width : initial.x, 0);
        expect(corners[i].startsWith("n") ? after.y + after.height : after.y).toBeCloseTo(corners[i].startsWith("n") ? initial.y + initial.height : initial.y, 0);
      }
      const settled = (await PreviewArea.card(page).boundingBox())!;
      expect(settled.width).toBeCloseTo(before.width, 0);
      expect(settled.height).toBeCloseTo(before.height, 0);
      await actor.attemptsTo(LeavePreview());
    }
    const preferred = await page.evaluate(() =>
      localStorage.getItem("brains.preview.size"),
    );
    await page.setViewportSize({ width: 1024, height: 380 });
    await actor.attemptsTo(HoverReference(expressions[0], 1));
    await expect(PreviewArea.card(page)).toContainText("四角预览");
    await expect
      .poll(async () => {
        const box = (await PreviewArea.card(page).boundingBox())!;
        return box.y + box.height;
      })
      .toBeLessThanOrEqual(380);
    expect(
      await page.evaluate(() => localStorage.getItem("brains.preview.size")),
    ).toBe(preferred);
    await actor.attemptsTo(LeavePreview());
    await page.setViewportSize({ width: 1440, height: 900 });
    await actor.attemptsTo(HoverReference(expressions[0], 1));
    await expect
      .poll(async () => (await PreviewArea.card(page).boundingBox())!.height)
      .toBeCloseTo(JSON.parse(preferred!).height, 0);
  });

  it("resizes in screen coordinates under UI zoom and keeps metadata readable on a phone", async ({
    actor,
    page,
  }, testInfo) => {
    const dir = `web29-zoom-${Date.now()}`;
    await page.addInitScript(() =>
      localStorage.setItem("brains-ui", JSON.stringify({ fontSize: 16 })),
    );
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(`${dir}/source.md`, "[缩放](./target.md)"),
      SeedNoteViaApi(`${dir}/target.md`, source),
      SeedImageViaApi(`${dir}/diagram.svg`),
      SignIn(),
      OpenNote(`${dir}/source.md`),
      HoverReference("[缩放](./target.md)", 1),
    );
    await expect(PreviewArea.card(page)).toContainText("可阅读、可连接");
    const before = (await PreviewArea.card(page).boundingBox())!;
    await actor.attemptsTo(ResizePreview(100, 40));
    await expect
      .poll(async () => (await PreviewArea.card(page).boundingBox())!.width)
      .toBeCloseTo(before.width + 100, 0);
    await actor.attemptsTo(LeavePreview());
    await actor.attemptsTo(OpenNote(`${dir}/target.md`), ShowDocumentPreview());
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole("tab", { name: "编辑", exact: true }).click();
    await actor.attemptsTo(ShowDocumentPreview());
    await expect(PreviewArea.metadata(page)).toBeVisible();
    await expect(PreviewArea.metadata(page)).toMatchAriaSnapshot(
      metadataSnapshot,
    );
    await testInfo.attach("metadata-phone", {
      body: await PreviewArea.document(page).screenshot(),
      contentType: "image/png",
    });
  });

  it("uses the same metadata and remembered card size for completion and source hover", async ({
    actor,
    page,
  }) => {
    const name = `completion-${Date.now()}`;
    const dir = `web29-${name}`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedNoteViaApi(`${dir}/source.md`, "开始编辑"),
      SeedNoteViaApi(
        `${dir}/${name}.md`,
        "---\ntitle: 补全目标\ndescription: 与悬停使用同一个预览\n---\n\n正文",
      ),
      SignIn(),
      OpenNote(`${dir}/source.md`),
    );
    await PreviewArea.editor(page).fill(`[[${name}`);
    await page.keyboard.press("Control+Space");
    await expect(PreviewArea.card(page)).toContainText("补全目标");
    await actor.attemptsTo(ResizePreview(60, 20));
    const expanded = (await PreviewArea.card(page).boundingBox())!;
    const corner = (await PreviewArea.resize(page).getAttribute("data-corner"))!;
    await actor.attemptsTo(ResizePreview(-80, -40));
    const shrunk = (await PreviewArea.card(page).boundingBox())!;
    expect(shrunk.width).toBeCloseTo(expanded.width - 80, 0);
    expect(shrunk.height).toBeCloseTo(expanded.height - 40, 0);
    expect(corner.endsWith("w") ? shrunk.x + shrunk.width : shrunk.x).toBeCloseTo(corner.endsWith("w") ? expanded.x + expanded.width : expanded.x, 0);
    expect(corner.startsWith("n") ? shrunk.y + shrunk.height : shrunk.y).toBeCloseTo(corner.startsWith("n") ? expanded.y + expanded.height : expanded.y, 0);
    const size = (await PreviewArea.card(page).boundingBox())!;
    await page.keyboard.press("Enter");
    await expect(PreviewArea.editor(page)).toHaveText(`[[${name}]]`);
    await actor.attemptsTo(HoverReference(`[[${name}]]`, 2));
    await expect(PreviewArea.card(page)).toContainText("补全目标");
    await expect
      .poll(async () => (await PreviewArea.card(page).boundingBox())!.width)
      .toBeCloseTo(size.width, 0);
  });

  it("renders a historical image using that revision's media type in both the card and dialog", async ({
    actor,
    page,
  }) => {
    const path = `web29-history-${Date.now()}/changed.md`;
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(
      SeedImageViaApi(path),
      AnotherClientRewrites(path, "---\ntitle: 现在是文档\n---\n\n现在的正文"),
      SignIn(),
      OpenNote(path),
    );
    await page
      .getByTestId("editor-pane")
      .getByRole("button", { name: "属性", exact: true })
      .click();
    await page
      .getByTestId("history-list")
      .getByText("gen 1", { exact: true })
      .hover();
    await expect
      .poll(() =>
        PreviewArea.card(page)
          .locator("img")
          .evaluate((img) => (img as HTMLImageElement).naturalWidth),
      )
      .toBe(640);
    await actor.attemptsTo(ResizePreview(40, 20), LeavePreview());
    await page.getByTestId("view-revision-gen-1").click();
    await expect
      .poll(() =>
        page
          .getByTestId("revision-view-dialog")
          .locator("img")
          .evaluate((img) => (img as HTMLImageElement).naturalWidth),
      )
      .toBe(640);
  });
});

it("anchors a wrapped source link at the hovered visual line near the editor edge", async ({ actor, page }) => {
  const dir = `web30-wrap-${Date.now()}`;
  const label = "跨行链接标题".repeat(24), expression = `[${label}](./target.md)`;
  actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
  await page.setViewportSize({ width: 1200, height: 850 });
  await actor.attemptsTo(SeedNoteViaApi(`${dir}/source.md`, "# 定位\n\n" + "普通正文内容。".repeat(20) + expression), SeedNoteViaApi(`${dir}/target.md`, "# 第二行附近的预览"), SignIn(), OpenNote(`${dir}/source.md`));
  const characters = await PreviewArea.line(page, expression).evaluate((element) => {
    const from = element.textContent!.indexOf("["), walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    const result: { offset: number; left: number; top: number; bottom: number }[] = [];
    let index = 0;
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      for (let i = 0; i < node.textContent!.length; i++) {
        if (index + i < from) continue;
        const range = document.createRange(); range.setStart(node, i); range.setEnd(node, i + 1);
        const rect = range.getBoundingClientRect();
        result.push({ offset: index + i - from, left: rect.left, top: rect.top, bottom: rect.bottom });
      }
      index += node.textContent!.length;
    }
    return result;
  });
  const target = characters.find(r => r.top > characters[0].top + 5 && r.left < 400)!;
  expect(target).toBeTruthy();
  await actor.attemptsTo(HoverReference(expression, target.offset));
  await expect(PreviewArea.card(page)).toContainText("第二行附近的预览");
  const card = (await PreviewArea.card(page).boundingBox())!;
  expect(card.x).toBeCloseTo(target.left, 0);
  expect(card.y).toBeCloseTo(target.bottom, 0);
  await actor.attemptsTo(ResizePreview(-60, -50));
  const resized = (await PreviewArea.card(page).boundingBox())!;
  expect(resized.x).toBeCloseTo(card.x, 0);
  expect(resized.y).toBeCloseTo(card.y, 0);
});

it("moves the resize handle when a reading preview flips after a viewport change", async ({ actor, page }) => {
  const dir = `web30-flip-${Date.now()}`;
  actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await actor.attemptsTo(SeedNoteViaApi(`${dir}/source.md`, "---\ntitle: 页面\ndescription: |\n" + "  前置说明\n".repeat(8) + "links:\n  翻转链接: ./target.md\n---\n\n正文"), SeedNoteViaApi(`${dir}/target.md`, "# 浮卡避让"), SignIn(), OpenNote(`${dir}/source.md`), ShowDocumentPreview());
  const link = PreviewArea.document(page).getByRole("link", { name: "翻转链接" });
  await link.hover();
  await expect(PreviewArea.card(page)).toContainText("浮卡避让");
  await expect(page.locator(".preview-popup")).toHaveAttribute("data-side", "bottom");
  const initial = (await PreviewArea.card(page).boundingBox())!;
  const anchor = (await link.boundingBox())!;
  await page.setViewportSize({ width: 1440, height: Math.ceil(anchor.y + 100) });
  await expect(page.locator(".preview-popup")).toHaveAttribute("data-side", "top");
  await expect(PreviewArea.resize(page)).toHaveAttribute("data-corner", /^n/);
  const flipped = (await PreviewArea.card(page).boundingBox())!;
  expect(flipped.width).toBeCloseTo(initial.width, 0);
  expect(flipped.height).toBeCloseTo(initial.height, 0);
  await actor.attemptsTo(ResizePreview(-40, -40));
  const shrunk = (await PreviewArea.card(page).boundingBox())!;
  expect(shrunk.y + shrunk.height).toBeCloseTo(flipped.y + flipped.height, 0);
});
