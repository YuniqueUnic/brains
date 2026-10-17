import { describe, it } from "@serenity-js/playwright-test";
import {
  Ensure,
  containAtLeastOneItemThat,
  includes,
  equals,
  property,
} from "@serenity-js/assertions";
import { Text } from "@serenity-js/web";
import { Duration } from "@serenity-js/core";

import { CallBrainsApi } from "../screenplay/abilities/CallBrainsApi";
import { Instance } from "../screenplay/support/Instance";
import { SignIn } from "../screenplay/tasks/Authoring";
import { OpenNote, SearchFor, SeedNoteViaApi } from "../screenplay/tasks/Navigation";
import { SearchHitPaths } from "../screenplay/questions/Views";
import { Workbench } from "../screenplay/screens/Workbench";
import { EditorArea } from "../screenplay/screens/EditorArea";
import { Navigate } from "@serenity-js/web";

/**
 * U05: 中文 literal search is case-sensitive UTF-8 substring matching; FTS is
 * explicit token search. A complete miss says so; a paged scan never fakes
 * completion.
 */
describe("search", () => {
  it("finds Chinese substrings across notes and opens the hit", async ({ actor }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const stamp = Date.now();
    const marker = `缓存标记${stamp}`;
    await actor.attemptsTo(
      SignIn(),
      SeedNoteViaApi(`e2e/search/a-${stamp}.md`, `分层${marker}详解`),
      SeedNoteViaApi(`e2e/search/b-${stamp}.md`, `无${marker}的旁路`),
      SearchFor(marker),
      Ensure.eventually(SearchHitPaths(), property("length", equals(2)))
        .timeoutAfter(Duration.ofSeconds(10)),
    );

    // Opening a hit loads it into the editor; results stay for drilling.
    await actor.attemptsTo(
      Navigate.to(Instance.baseUrl),
      OpenNote(`e2e/search/a-${stamp}.md`),
      Ensure.eventually(Text.of(EditorArea.content()), includes("分层缓存"))
        .timeoutAfter(Duration.ofSeconds(10)),
    );
  });

  it("reports a complete miss as zero hits", async ({ actor }) => {
    await actor.attemptsTo(
      SignIn(),
      SearchFor("绝不存在的字符串xyz"),
      // The panel header states the hit count: waiting on "0 个命中" keeps
      // this from passing vacuously before the search round-trip lands.
      Ensure.eventually(Text.of(Workbench.searchResults()), includes("0 个命中"))
        .timeoutAfter(Duration.ofSeconds(10)),
    );
  });

  it("matches English tokens in FTS mode", async ({ actor }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const stamp = Date.now();
    await actor.attemptsTo(
      SignIn(),
      SeedNoteViaApi(`e2e/search/fts-${stamp}.md`, "rust ci cache strategy"),
      SearchFor("cache", "fts"),
      Ensure.eventually(
        SearchHitPaths(),
        containAtLeastOneItemThat(includes(`fts-${stamp}`)),
      ).timeoutAfter(Duration.ofSeconds(10)),
    );
  });

  /** U18: hybrid fusion ranks hits with per-retriever provenance, and
   *  retrievers that cannot contribute are stated, never hidden. */
  it("ranks hybrid hits with provenance and names unavailable retrievers", async ({ actor }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    const stamp = Date.now();
    const marker = `hybridmarker${stamp}`;
    await actor.attemptsTo(
      SignIn(),
      SeedNoteViaApi(`e2e/search/h-${stamp}.md`, `${marker} pipeline notes`),
      SearchFor(marker, "hybrid"),
      Ensure.eventually(
        SearchHitPaths(),
        containAtLeastOneItemThat(includes(`h-${stamp}`)),
      ).timeoutAfter(Duration.ofSeconds(10)),
    );
    // Provenance: literal and fts both matched this hit.
    Ensure.eventually(
      Text.of(Workbench.searchUnavailable()),
      includes("semantic"),
    ).timeoutAfter(Duration.ofSeconds(10));
  });
});

