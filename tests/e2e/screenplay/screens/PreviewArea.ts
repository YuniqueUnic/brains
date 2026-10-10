import type { Page } from "@playwright/test";

export const PreviewArea = {
  document: (page: Page) => page.getByTestId("markdown-preview"),
  metadata: (page: Page) =>
    page.getByTestId("markdown-preview").getByTestId("document-metadata"),
  card: (page: Page) => page.getByTestId("preview-card").last(),
  resize: (page: Page) =>
    page
      .getByTestId("preview-card")
      .last()
      .getByRole("button", { name: /调整预览大小/ }),
  line: (page: Page, text: string) =>
    page.locator(".cm-line").filter({ hasText: text }),
  editor: (page: Page) => page.locator(".cm-content"),
  mode: (page: Page, mode: "预览" | "编辑") =>
    page
      .getByTestId("editor-pane")
      .getByRole("button", { name: mode, exact: true }),
};
