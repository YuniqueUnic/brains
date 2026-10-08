import { Ensure, includes } from "@serenity-js/assertions";
import { Duration } from "@serenity-js/core";
import { Enter, Click, Navigate, Press, Text } from "@serenity-js/web";
import { Task } from "@serenity-js/core";

import { LoginScreen } from "../screens/LoginScreen";
import { Workbench } from "../screens/Workbench";
import { EditorArea } from "../screens/EditorArea";
import { Instance } from "../support/Instance";

const OpenApp = () =>
  Task.where("#actor opens the BrainStore workbench", Navigate.to(Instance.baseUrl));

const LogIn = () =>
  Task.where("#actor signs in with the instance token",
    Enter.theValue(Instance.token).into(LoginScreen.tokenInput()),
    Click.on(LoginScreen.loginButton()),
  );

/** Sign in and land on the workbench. */
export const SignIn = () => Task.where("#actor signs in to the workbench", OpenApp(), LogIn());

/** Open the app without signing in (login screen scenarios). */
export const Visit = () => Task.where("#actor visits the app", OpenApp());

/** Open the 新建 dialog, create an empty note, and wait until it is open:
 *  the create round-trip is async, and typing into a not-yet-bound editor
 *  would silently discard the text. */
export const StartNewNote = (path: string) =>
  Task.where(`#actor starts a new note at ${path}`,
    Click.on(Workbench.newFileButton()),
    Enter.theValue(path).into(Workbench.newFilePathInput()),
    Click.on(Workbench.newFileConfirm()),
    Ensure.eventually(Text.of(EditorArea.path()), includes(path))
      .timeoutAfter(Duration.ofSeconds(10)),
  );

/** Replace the open note's whole content and save it (Ctrl/Cmd+S). */
export const RewriteNote = (content: string) =>
  Task.where("#actor rewrites and saves the open note",
    Enter.theValue(content).into(EditorArea.content()),
    Press.the("ControlOrMeta+s").in(EditorArea.content()),
  );

/** Create a note end to end through the UI. */
export const CreateNote = (path: string, content: string) =>
  Task.where(`#actor creates a note at ${path}`,
    StartNewNote(path),
    RewriteNote(content),
  );
