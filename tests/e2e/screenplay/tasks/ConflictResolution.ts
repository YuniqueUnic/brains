import { Click } from "@serenity-js/web";
import { Task } from "@serenity-js/core";

import { EditorArea } from "../screens/EditorArea";

/** In the conflict view, keep the local text against the shown remote revision. */
export const KeepLocalVersion = () =>
  Task.where("#actor keeps the local version in the conflict view",
    Click.on(EditorArea.conflictKeepLocal()),
  );

/** In the conflict view, discard local changes and take the remote text. */
export const TakeRemoteVersion = () =>
  Task.where("#actor takes the remote version", Click.on(EditorArea.conflictTakeRemote()));
