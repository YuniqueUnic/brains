import { By, PageElement, PageElements } from "@serenity-js/web";

/**
 * Main workbench (design §13.1): top bar, sidebar file list, dialogs,
 * banner, search results. Semantic locators only.
 */
export const Workbench = {
  searchBox: () =>
    PageElement.located(By.css('[data-testid="search-box"]')).describedAs("search box"),
  modeLiteralButton: () =>
    PageElement.located(By.css('[data-testid="mode-literal"]')).describedAs("literal mode button"),
  modeFtsButton: () =>
    PageElement.located(By.css('[data-testid="mode-fts"]')).describedAs("fts mode button"),
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
  searchHitPaths: () =>
    PageElements.located(
      By.css('[data-testid="search-results"] ul button'),
    ).describedAs("search hit paths"),
  fileItems: () =>
    PageElements.located(
      By.css('[data-testid="file-list"] .file-path'),
    ).describedAs("listed files"),
  trashConfirmBar: () =>
    PageElement.located(By.css('[data-testid="trash-confirm"]')).describedAs("trash confirm bar"),
  trashConfirmYes: () =>
    PageElement.located(By.css('[data-testid="trash-confirm-yes"]')).describedAs("trash confirm yes"),
};
