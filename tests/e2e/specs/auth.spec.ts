import { describe, it } from "@serenity-js/playwright-test";
import { Ensure, includes, equals } from "@serenity-js/assertions";
import { Text } from "@serenity-js/web";

import { SignIn, Visit } from "../screenplay/tasks/Authoring";
import { Workbench } from "../screenplay/screens/Workbench";

/** U-Auth: the browser session exchange (design §15.1). */
describe("Brains login", () => {
  it("rejects a wrong token with a visible error", async ({ actor }) => {
    await actor.attemptsTo(Visit());
    await actor.attemptsTo(
      Enter.theValue("wrong-token").into(LoginScreen.tokenInput()),
      Click.on(LoginScreen.loginButton()),
      Ensure.eventually(Text.of(LoginScreen.error()), includes("失败")),
    );
  });

  it("admits the owner token into the workbench", async ({ actor }) => {
    await actor.attemptsTo(SignIn());
    await actor.attemptsTo(
      Ensure.eventually(Text.of(Workbench.logoutButton()), equals("退出")),
    );
  });
});

import { LoginScreen } from "../screenplay/screens/LoginScreen";
import { Click, Enter } from "@serenity-js/web";
