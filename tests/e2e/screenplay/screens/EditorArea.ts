import { By, PageElement, PageElements } from "@serenity-js/web";

/** Editor pane (design §13.2): CodeMirror host, status line, actions. */
export const EditorArea = {
  pane: () =>
    PageElement.located(By.css('[data-testid="editor-pane"]')).describedAs("editor pane"),
  path: () =>
    PageElement.located(By.css('[data-testid="editor-path"]')).describedAs("editor path"),
  status: () =>
    PageElement.located(By.css('[data-testid="editor-status"]')).describedAs("editor status"),
  publishButton: () =>
    PageElement.located(By.css('[data-testid="publish-button"]')).describedAs("publish button"),
  content: () =>
    PageElement.located(By.css(".cm-content")).describedAs("editor content"),
  trashCurrentButton: () =>
    PageElement.located(By.css('[data-testid="trash-current"]')).describedAs("trash current button"),
  historyList: () =>
    PageElement.located(By.css('[data-testid="history-list"]')).describedAs("history list"),
  restoreRevisionButtons: () =>
    PageElements.located(By.css('[data-testid^="restore-revision-gen-"]')).describedAs(
      "restore revision buttons",
    ),
  restoreRevisionButtonForGeneration: (generation: number) =>
    PageElement.located(By.css(`[data-testid="restore-revision-gen-${generation}"]`)).describedAs(
      `restore revision gen-${generation} button`,
    ),
  conflictView: () =>
    PageElement.located(By.css('[data-testid="conflict-view"]')).describedAs("conflict view"),
  conflictKeepLocal: () =>
    PageElement.located(By.css('[data-testid="conflict-keep-local"]')).describedAs(
      "keep local button",
    ),
  conflictTakeRemote: () =>
    PageElement.located(By.css('[data-testid="conflict-take-remote"]')).describedAs(
      "take remote button",
    ),
};
