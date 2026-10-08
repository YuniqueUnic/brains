import { describe, it } from "@serenity-js/playwright-test";
import { Ensure, EnsureEventually, includes, equals } from "@serenity-js/assertions";
import { Duration } from "@serenity-js/core";
import { Text } from "@serenity-js/web";

import { SignIn } from "../screenplay/tasks/Authoring";
import {
  AnotherClientRewrites,
  OpenNote,
  SeedNoteViaApi,
} from "../screenplay/tasks/Navigation";
import { KeepLocalVersion } from "../screenplay/tasks/ConflictResolution";
import { Navigate } from "@serenity-js/web";
import { Enter, Press } from "@serenity-js/web";
import { EditorArea } from "../screenplay/screens/EditorArea";
import { ConflictViewText, EditorStatus } from "../screenplay/questions/Views";
import { CallBrainsApi } from "../screenplay/abilities/CallBrainsApi";
import { Instance } from "../screenplay/support/Instance";

const SAVED = equals("已保存");
const CONFLICT = equals("版本冲突");

/**
 * U03 + §13.2: two clients write the same base revision; the browser loses
 * the CAS race, shows the base/local/remote conflict view, and only saves
 * again when the human explicitly keeps the local text. Nothing auto-retries.
 */
describe("conflict handling", () => {
  it("shows the conflict view and keeps the local text after an explicit choice", async ({
    actor,
  }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const api = CallBrainsApi.as(actor);
    const path = `e2e/conflict/duel-${Date.now()}.md`;
    await actor.attemptsTo(
      SignIn(),
      SeedNoteViaApi(path, "初始版本"),
      Navigate.to(Instance.baseUrl),
      OpenNote(path),
      AnotherClientRewrites(path, "远端同事改过的版本"),
      Enter.theValue("初始版本\n浏览器里的本地修改 缓存").into(EditorArea.content()),
      Press.the("ControlOrMeta+s").in(EditorArea.content()),
      Ensure.eventually(EditorStatus(), CONFLICT).timeoutAfter(Duration.ofSeconds(10)),
    );

    // The view labels the remote revision (textarea values are not part of
    // textContent, so assert on the revision prefix the header shows).
    const rival = await api.getText(`file?path=${encodeURIComponent(path)}`);
    await actor.attemptsTo(
      Ensure.that(ConflictViewText(), includes(rival.revision.slice(0, 8))),
    );

    // Nothing auto-retried: the server still holds the rival text.
    await expectServerBody(api, path, "远端同事改过的版本");

    // Keep-local saves against the revision the page currently shows.
    await actor.attemptsTo(
      KeepLocalVersion(),
      Ensure.eventually(EditorStatus(), SAVED).timeoutAfter(Duration.ofSeconds(10)),
    );
    const final = await api.getText(`file?path=${encodeURIComponent(path)}`);
    if (!final.body.includes("浏览器里的本地修改") || !final.body.includes("缓存")) {
      throw new Error(`keep-local did not persist local text: ${final.body}`);
    }
  });

  it("re-conflicts instead of overwriting when the remote moves again", async ({
    actor,
  }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const path = `e2e/conflict/racing-${Date.now()}.md`;
    await actor.attemptsTo(
      SignIn(),
      SeedNoteViaApi(path, "初始"),
      Navigate.to(Instance.baseUrl),
      OpenNote(path),
    );

    // First conflict…
    await actor.attemptsTo(
      AnotherClientRewrites(path, "远端 v2"),
      Enter.theValue("初始\n本地 v2").into(EditorArea.content()),
      Press.the("ControlOrMeta+s").in(EditorArea.content()),
      Ensure.eventually(EditorStatus(), CONFLICT).timeoutAfter(Duration.ofSeconds(10)),
    );

    // …the rival moves the remote forward while the human is deciding…
    await actor.attemptsTo(AnotherClientRewrites(path, "远端 v3 抢先写入"));

    // …and keep-local must surface the renewed conflict, not overwrite.
    await actor.attemptsTo(KeepLocalVersion());
    const api = CallBrainsApi.as(actor);
    const latest = await api.getText(`file?path=${encodeURIComponent(path)}`);
    await actor.attemptsTo(
      Ensure.eventually(ConflictViewText(), includes(latest.revision.slice(0, 8)))
        .timeoutAfter(Duration.ofSeconds(10)),
    );
    await expectServerBody(api, path, "远端 v3 抢先写入");
  });
});

async function expectServerBody(
  api: CallBrainsApi,
  path: string,
  expected: string,
): Promise<void> {
  const current = await api.getText(`file?path=${encodeURIComponent(path)}`);
  if (current.body !== expected) {
    throw new Error(`server holds "${current.body}", expected "${expected}"`);
  }
}
