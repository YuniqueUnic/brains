import { Click, Enter, Press, Select } from "@serenity-js/web";
import { Interaction, Task } from "@serenity-js/core";

import { CallBrainsApi } from "../abilities/CallBrainsApi";
import { Workbench } from "../screens/Workbench";

/** Click a file in the sidebar. */
export const OpenNote = (path: string) =>
  Task.where(`#actor opens ${path} from the sidebar`, Click.on(Workbench.fileItem(path)));

/** Search and wait for the results panel via the mode selector. */
export const SearchFor = (query: string, mode: "literal" | "fts" = "literal") =>
  Task.where(`#actor searches for "${query}" (${mode})`,
    Click.on(Workbench.paletteTrigger()),
    Click.on(mode === "fts" ? Workbench.modeFtsButton() : Workbench.modeLiteralButton()),
    Enter.theValue(query).into(Workbench.searchBox()),
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
      if (response.status !== 201) {
        throw new Error(`seed failed: HTTP ${response.status}`);
      }
    }),
  );

/** Toggle the trash view in the sidebar. */
export const ToggleTrash = () =>
  Task.where("#actor toggles the trash view", Click.on(Workbench.trashToggleButton()));
