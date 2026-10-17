import { By, PageElement, PageElements } from "@serenity-js/web";

/**
 * Main workbench (design §13.1): top bar, sidebar file list, dialogs,
 * banner, search results. Semantic locators only.
 */
export const Workbench = {
  searchBox: () =>
    PageElement.located(By.css('[data-testid="search-box"]')).describedAs("search box"),
  newFileButton: () =>
    PageElement.located(By.css('[data-testid="new-file"]')).describedAs("new file button"),
  newFileDialog: () =>
    PageElement.located(By.css('[data-testid="new-file-dialog"]')).describedAs("new file dialog"),
  newFilePathInput: () =>
    PageElement.located(By.css('[data-testid="new-file-path"]')).describedAs("new file path input"),
  newFileConfirm: () =>
    PageElement.located(By.css('[data-testid="new-file-confirm"]')).describedAs("new file confirm"),
  trashToggleButton: () =>
    PageElement.located(By.css('[data-testid="toggle-trash"]')).describedAs("trash toggle"),
  logoutButton: () =>
    PageElement.located(By.css('[data-testid="logout"]')).describedAs("logout button"),
  banner: () => PageElement.located(By.css('[data-testid="banner"]')).describedAs("banner"),
  toasts: () => PageElements.located(By.css('[data-sonner-toast]')).describedAs("toasts"),
  fileList: () =>
    PageElement.located(By.css('[data-testid="file-list"]')).describedAs("file list"),
  fileItem: (path: string) =>
    PageElement.located(
      By.css(`[data-testid="file-list"] .file-path:has-text("${path}")`),
    ).describedAs(`file ${path}`),
  restoreButtonFor: (path: string) =>
    PageElement.located(
      By.css(`[data-testid="file-list"] li:has-text("${path}") .restore-button`),
    ).describedAs(`restore button for ${path}`),
  searchResults: () =>
    PageElement.located(By.css('[data-testid="search-results"]')).describedAs("search results"),
  searchUnavailable: () =>
    PageElement.located(By.css('[data-testid="search-unavailable"]')).describedAs(
      "unavailable retrievers notice",
    ),
  searchHitPaths: () =>
    PageElements.located(
      By.css('[data-testid="search-results"] [data-testid="search-hit"]'),
    ).describedAs("search hit paths"),
  paletteTrigger: () =>
    PageElement.located(By.css('[data-testid="palette-trigger"]')).describedAs(
      "command palette trigger",
    ),
  accountMenuButton: () =>
    PageElement.located(By.css('[data-testid="account-button"]')).describedAs(
      "account menu trigger",
    ),
  editorMenuButton: () =>
    PageElement.located(By.css('[data-testid="editor-menu"]')).describedAs(
      "editor more-actions trigger",
    ),
  fileItems: () =>
    PageElements.located(
      By.css('[data-testid="file-list"] .file-path'),
    ).describedAs("listed files"),
  trashConfirmBar: () =>
    PageElement.located(By.css('[data-testid="trash-confirm"]')).describedAs("trash confirm bar"),
  trashConfirmYes: () =>
    PageElement.located(By.css('[data-testid="trash-confirm-yes"]')).describedAs("trash confirm yes"),
};
