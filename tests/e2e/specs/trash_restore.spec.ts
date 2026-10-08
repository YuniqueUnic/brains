import { describe, it } from "@serenity-js/playwright-test";
import {
  Ensure,
  containAtLeastOneItemThat,
  includes,
  not,
  equals,
} from "@serenity-js/assertions";
import { Duration } from "@serenity-js/core";
import { Click, Text } from "@serenity-js/web";

import { CreateNote, RewriteNote, SignIn } from "../screenplay/tasks/Authoring";
import { OpenNote, ToggleTrash } from "../screenplay/tasks/Navigation";
import { Workbench } from "../screenplay/screens/Workbench";
import { EditorArea } from "../screenplay/screens/EditorArea";
import { BannerText, EditorStatus, ListedFiles } from "../screenplay/questions/Views";

const SAVED = equals("已保存");

/**
 * U07/U08: soft delete keeps a recoverable tombstone; restore creates a NEW
 * revision (generation grows), never rewinds HEAD.
 */
describe("trash and restore", () => {
  it("trashes a note via the confirm bar and restores it from the trash view", async ({
    actor,
  }) => {
    const path = `e2e/trash/restore-me-${Date.now()}.md`;
    await actor.attemptsTo(
      SignIn(),
      CreateNote(path, "要被误删的内容 缓存"),
      Ensure.eventually(EditorStatus(), SAVED).timeoutAfter(Duration.ofSeconds(10)),
      Click.on(Workbench.editorMenuButton()),
      Click.on(EditorArea.trashCurrentButton()),
      Click.on(Workbench.trashConfirmYes()),
      Ensure.eventually(ListedFiles(), not(containAtLeastOneItemThat(includes(path))))
        .timeoutAfter(Duration.ofSeconds(5)),
    );

    // The trashed file shows up under the trash view with a restore button.
    await actor.attemptsTo(
      ToggleTrash(),
      Click.on(Workbench.restoreButtonFor(path)),
      Ensure.eventually(BannerText(), includes("已恢复"))
        .timeoutAfter(Duration.ofSeconds(10)),
    );
  });

  it("restores an older revision from the history panel as a new generation", async ({
    actor,
  }) => {
    const path = `e2e/history/rewind-${Date.now()}.md`;
    await actor.attemptsTo(
      SignIn(),
      CreateNote(path, "第一版内容"),
      RewriteNote("第二版内容"),
      Ensure.eventually(EditorStatus(), SAVED).timeoutAfter(Duration.ofSeconds(10)),
    );

    // History renders newest-first and refreshes after each save: once
    // "gen 3" (第二版) appears, the list reflects the latest save and
    // [gen3=第二版, gen2=第一版, gen1=空(新建占位)]. Restoring gen2 must land
    // 第一版内容 in the editor as a NEW generation — HEAD moves forward,
    // never backward.
    // TEMP-DEBUG: record mutation traffic inside the page
    await actor.attemptsTo(
      Ensure.eventually(Text.of(EditorArea.historyList()), includes("gen 3"))
        .timeoutAfter(Duration.ofSeconds(10)),
      Click.on(EditorArea.restoreRevisionButtonForGeneration(2)),
      Ensure.eventually(BannerText(), includes("已恢复"))
        .timeoutAfter(Duration.ofSeconds(10)),
      Ensure.eventually(Text.of(EditorArea.content()), includes("第一版内容"))
        .timeoutAfter(Duration.ofSeconds(10)),
    );
  });
});
