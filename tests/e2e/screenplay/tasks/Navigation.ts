import { Click, Enter, Press } from "@serenity-js/web";
import { Interaction, Task } from "@serenity-js/core";

import { CallBrainsApi } from "../abilities/CallBrainsApi";
import { Workbench } from "../screens/Workbench";

/** Click a file in the sidebar. */
export const OpenNote = (path: string) =>
  Task.where(`#actor opens ${path} from the sidebar`, Click.on(Workbench.fileItem(path)));

/** Search with the same keyboard command a user can enter. */
export const SearchFor = (query: string, mode: "literal" | "fts" = "literal") =>
  Task.where(`#actor searches for "${query}" (${mode})`,
    Click.on(Workbench.paletteTrigger()),
    Enter.theValue(`/${mode === "fts" ? "fts" : "text"} ${query}`).into(Workbench.searchBox()),
    Press.the("Enter").in(Workbench.searchBox()),
  );

/** Simulate a second client rewriting the file behind the browser's back. */
export const AnotherClientRewrites = (path: string, body: string) =>
  Task.where(`#actor's rival client rewrites ${path}`,
    Interaction.where(`#actor's rival client PUTs ${path}`, async (actor) => {
      const api = CallBrainsApi.as(actor);
      const current = await api.getText(`file?path=${encodeURIComponent(path)}`);
      const response = await api.updateText(path, body, current.revision);
      if (response.status !== 200) {
        const detail = await response.text().catch(() => "");
        throw new Error(
          `rival rewrite failed: HTTP ${response.status} with base ${current.revision} — ${detail}`,
        );
      }
    }),
  );

/** Create a file directly through the API (setup path, not the UI path). */
export const SeedNoteViaApi = (path: string, body: string) =>
  Task.where(`#actor seeds ${path} via the API`,
    Interaction.where(`#actor seeds ${path}`, async (actor) => {
      const api = CallBrainsApi.as(actor);
      const response = await api.createText(path, body);
      if (response.status !== 200) {
        throw new Error(`seed failed: HTTP ${response.status}`);
      }
      const result = await response.json();
      if (result.created !== true || result.path !== path ||
          response.headers.get("ETag") !== `"${result.revision}"`) {
        throw new Error(`seed did not create ${path} with matching revision metadata`);
      }
    }),
  );

/** Toggle the trash view in the sidebar. */
export const ToggleTrash = () =>
  Task.where("#actor toggles the trash view", Click.on(Workbench.trashToggleButton()));

/** Seed a binary through the same raw upload contract used by the browser. */
export const SeedImageViaApi = (path: string) => Task.where(`#actor seeds the image ${path}`,
  Interaction.where("#actor uploads a deterministic diagram", async actor => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="160"><rect width="640" height="160" fill="#dfede4"/><circle cx="320" cy="80" r="55" fill="#668d78"/></svg>';
    const response = await CallBrainsApi.as(actor).createBinary(path, new TextEncoder().encode(svg), "image/svg+xml");
    if (response.status !== 200) throw new Error(`image seed failed: HTTP ${response.status}`);
  }),
);
