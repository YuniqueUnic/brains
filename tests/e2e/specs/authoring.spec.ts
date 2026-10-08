import { describe, it } from "@serenity-js/playwright-test";
import { Ensure, includes, equals, not } from "@serenity-js/assertions";
import { Duration } from "@serenity-js/core";
import { By, Click, Enter, PageElement, Text } from "@serenity-js/web";

import { CreateNote, SignIn, StartNewNote } from "../screenplay/tasks/Authoring";
import { Navigate } from "@serenity-js/web";
import { OpenNote } from "../screenplay/tasks/Navigation";
import { EditorArea } from "../screenplay/screens/EditorArea";
import { Instance } from "../screenplay/support/Instance";
import { Workbench } from "../screenplay/screens/Workbench";
import { EditorStatus } from "../screenplay/questions/Views";

const SAVED = equals("已发布");

/**
 * U01 (design §19): a human writes a new memory in the web app; the bytes
 * land through the same conditional PUT contract every client shares.
 */
describe("note authoring", () => {
  it("creates a note, saves it explicitly, and persists it across reloads", async ({
    actor,
  }) => {
    const path = `e2e/notes/authoring-${Date.now()}.md`;
    await actor.attemptsTo(
      SignIn(),
      CreateNote(path, "# 第一条 e2e 笔记\n\n正文 缓存策略"),
      Ensure.eventually(EditorStatus(), SAVED).timeoutAfter(Duration.ofSeconds(10)),
    );

    // A fresh navigation reads the committed bytes back through GET /file.
    await actor.attemptsTo(
      Navigate.to(Instance.baseUrl),
      OpenNote(path),
      Ensure.eventually(Text.of(EditorArea.content()), includes("第一条 e2e 笔记"))
        .timeoutAfter(Duration.ofSeconds(10)),
      Ensure.that(Text.of(EditorArea.content()), includes("缓存策略")),
    );
  });

  it("reports unsaved changes and never auto-saves them (P0 explicit save)", async ({
    actor,
  }) => {
    const path = `e2e/notes/dirty-${Date.now()}.md`;
    await actor.attemptsTo(
      SignIn(),
      StartNewNote(path),
      // Replace the empty document with the first version, unsaved.
      Enter.theValue("original body\nappended 缓存").into(EditorArea.content()),
      Ensure.that(EditorStatus(), equals("未发布草稿")),
    );

    // The server never auto-saves (P0). Locally the draft restores so a
    // crash or refresh cannot lose typing; discarding it reverts to the
    // server base — proving the unsaved text never reached the server.
    await actor.attemptsTo(
      Navigate.to(Instance.baseUrl),
      OpenNote(path),
      Ensure.eventually(Text.of(EditorArea.content()), includes("appended"))
        .timeoutAfter(Duration.ofSeconds(10)),
      Ensure.that(EditorStatus(), equals("未发布草稿")),
      Click.on(
        PageElement.located(By.css('button:has-text("丢弃")')).describedAs("discard draft"),
      ),
      Ensure.eventually(Text.of(EditorArea.content()), not(includes("appended")))
        .timeoutAfter(Duration.ofSeconds(10)),
    );
  });

  it("lists saved notes in the sidebar", async ({ actor }) => {
    const path = `e2e/notes/listed-${Date.now()}.md`;
    await actor.attemptsTo(
      SignIn(),
      CreateNote(path, "listed body 缓存"),
      Ensure.eventually(Text.of(Workbench.fileItem(path)), equals(path))
        .timeoutAfter(Duration.ofSeconds(10)),
    );
  });
});
