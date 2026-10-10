import { Interaction } from "@serenity-js/core";
import { PlaywrightPage } from "@serenity-js/playwright";
import { PreviewArea } from "../screens/PreviewArea";

export const ShowDocumentPreview = () =>
  Interaction.where("#actor reads the rendered document", async (actor) => {
    const page = await actor.answer(PlaywrightPage.current().nativePage());
    await PreviewArea.mode(page, "预览").click();
  });
export const ShowSource = () =>
  Interaction.where("#actor returns to the source", async (actor) => {
    const page = await actor.answer(PlaywrightPage.current().nativePage());
    await PreviewArea.mode(page, "编辑").click();
  });

/** Point at a character within the full source expression, including ! and labels. */
export const HoverReference = (text: string, offset = 0) =>
  Interaction.where(`#actor previews ${text}`, async (actor) => {
    const page = await actor.answer(PlaywrightPage.current().nativePage());
    const line = PreviewArea.line(page, text);
    await line.scrollIntoViewIfNeeded();
    const point = await line.evaluate(
      (element, { text, offset }) => {
        const from = (element.textContent ?? "").indexOf(text) + offset;
        const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
        let left = from;
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          const length = node.textContent?.length ?? 0;
          if (left < length) {
            const range = document.createRange();
            range.setStart(node, left);
            range.setEnd(node, left + 1);
            const rect = range.getBoundingClientRect();
            return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
          }
          left -= length;
        }
        throw new Error("Reference character is not visible");
      },
      { text, offset },
    );
    await page.mouse.move(point.x, point.y);
  });

export const ResizePreview = (widthDelta: number, heightDelta: number) =>
  Interaction.where(
    "#actor resizes the preview from its available corner",
    async (actor) => {
      const page = await actor.answer(PlaywrightPage.current().nativePage());
      const handle = PreviewArea.resize(page);
      await handle.hover();
      const corner = await handle.getAttribute("data-corner");
      const box = (await handle.boundingBox())!;
      const x = box.x + box.width / 2,
        y = box.y + box.height / 2;
      await page.mouse.move(x, y);
      await page.mouse.down();
      await page.mouse.move(
        x + widthDelta * (corner!.endsWith("w") ? -1 : 1),
        y + heightDelta * (corner!.startsWith("n") ? -1 : 1),
        { steps: 12 },
      );
      await page.mouse.up();
    },
  );

export const LeavePreview = () =>
  Interaction.where("#actor dismisses the preview", async (actor) => {
    const page = await actor.answer(PlaywrightPage.current().nativePage());
    await page.mouse.move(30, 30);
    await page.keyboard.press("Escape");
  });
