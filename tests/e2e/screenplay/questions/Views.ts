import { Question } from "@serenity-js/core";
import { Text } from "@serenity-js/web";

import { EditorArea } from "../screens/EditorArea";
import { Workbench } from "../screens/Workbench";

/** The editor status line text (已发布 / 未发布草稿 / 发布中… / 版本冲突…). */
export const EditorStatus = () => Text.of(EditorArea.status());

/** The banner text, or an empty string when no banner is shown. */
export const BannerText = () => Text.of(Workbench.banner());

/**
 * Sidebar paths currently listed, read as ONE container text. Reading items
 * one-by-one (`nth=0..N`) races a concurrent list update — a trashed file
 * vanishing mid-read leaves the last locator waiting forever (Playwright
 * actions have no default timeout), which stalls the whole scenario.
 */
export const ListedFiles = () =>
  Question.about<string[]>("listed files", async (actor) => {
    const raw = await Text.of(Workbench.fileList()).answeredBy(actor);
    return raw
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
  });

/** Paths shown in the search results panel. */
export const SearchHitPaths = () => Text.ofAll(Workbench.searchHitPaths());

/** The conflict view's text, including the remote revision label. */
export const ConflictViewText = () => Text.of(EditorArea.conflictView());
