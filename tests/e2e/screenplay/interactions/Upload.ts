import { Interaction } from "@serenity-js/core";
import { PlaywrightPage } from "@serenity-js/playwright";

export const UploadFile = (name: string, mimeType: string, buffer: Buffer) =>
  Interaction.where(`#actor uploads ${name}`, async actor => {
    const page = await actor.answer(PlaywrightPage.current().nativePage());
    await page.locator('input[type="file"]').first().setInputFiles({ name, mimeType, buffer });
  });
