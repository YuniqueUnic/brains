import { describe, it } from "@serenity-js/playwright-test";
import { expect } from "@playwright/test";
import { CallBrainsApi } from "../screenplay/abilities/CallBrainsApi";
import { SignIn } from "../screenplay/tasks/Authoring";
import { Instance } from "../screenplay/support/Instance";

describe("on-demand resource settings", () => {
  for (const width of [1440, 390]) {
    it(`loads usage on first visit and preserves unsaved settings across tabs at ${width}px`, async ({ actor, page }, testInfo) => {
      await page.setViewportSize({ width, height: 900 });
      actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
      await actor.attemptsTo(SignIn());
      let reads = 0;
      page.on("request", request => {
        if (request.method() === "GET" && new URL(request.url()).pathname === "/api/v1/resources") reads++;
      });
      await page.getByTestId("open-settings").click();
      const dialog = page.getByTestId("settings-dialog");
      await expect(dialog).toBeVisible();
      await expect(page.getByRole("tab", { name: "界面设置", exact: true })).toHaveAttribute("aria-selected", "true");
      expect(reads).toBe(0);
      const resourceTab = page.getByRole("tab", { name: "用量与额度", exact: true });
      // Native profiles have no resource control. They still must not fetch it.
      if (await resourceTab.count() === 0) return;
      await resourceTab.click();
      await expect(page.getByTestId("resource-settings-body")).toBeVisible();
      expect(reads).toBe(1);
      const fraction = page.getByRole("spinbutton", { name: "默认额度预算百分比", exact: true });
      const original = await fraction.inputValue();
      await fraction.fill("73");
      await page.getByRole("tab", { name: "界面设置", exact: true }).click();
      await resourceTab.click();
      await expect(fraction).toHaveValue("73");
      expect(reads).toBe(1);
      await expect(page.getByRole("button", { name: "保存额度", exact: true })).toBeEnabled();
      await expect(page.getByRole("button", { name: "刷新用量", exact: true })).toBeDisabled();
      await page.getByRole("button", { name: "放弃修改", exact: true }).click();
      await page.getByRole("button", { name: "刷新用量", exact: true }).click();
      await expect(fraction).toHaveValue(original);
      expect(reads).toBe(2);
      await page.getByLabel("按服务筛选", { exact: true }).selectOption("d1");
      await page.screenshot({ path: testInfo.outputPath(`resource-settings-${width}.png`) });
      const bounds = await dialog.boundingBox();
      expect(bounds).not.toBeNull();
      expect(bounds!.x).toBeGreaterThanOrEqual(0);
      expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
      expect(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
      await page.keyboard.press("Escape");
      await expect(dialog).toBeHidden();
      await page.getByTestId("open-settings").click();
      await expect(page.getByTestId("resource-settings-body")).toBeVisible();
      await expect(fraction).toHaveValue(original);
      expect(reads).toBe(3);
    });
  }

  for (const example of [{ width: 1440, language: "en", tab: "Usage & allowances", refresh: "Refresh usage", absent: "No measurement", source: "Cloudflare D1 account inventory", expired: "sample expired" },
                         { width: 390, language: "zh-CN", tab: "用量与额度", refresh: "刷新用量", absent: "尚无计量数据", source: "Cloudflare D1 账号数据库清单", expired: "采样已于" }]) {
    it(`distinguishes fresh and expired provider samples in ${example.language}`, async ({ actor, page }, testInfo) => {
      await page.setViewportSize({ width: example.width, height: 900 });
      await page.addInitScript(language => localStorage.setItem("brains-lang", language), example.language);
      actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
      await actor.attemptsTo(SignIn());
      let expired = false;
      await page.route("**/api/v1/resources", async route => {
        const response = await route.fetch();
        const body = await response.json();
        const at = Math.floor(Date.now() / 1000);
        body.usage = body.usage.filter((item: { rule: string }) => item.rule !== "d1-account-storage");
        // Presentation fixture over a real API response; provider aggregation
        // itself is verified by the offline inventory and workerd scenarios.
        body.usage.push({ key: "sample", meter: "sample", rule: "d1-account-storage", unit: "byte",
          consumed: 3000000, reserved: 0, limit: 4000000000, evidence: "exact",
          observed_at: at - 120, expires_at: expired ? at - 60 : at + 3600,
          source: "cloudflare.d1.inventory" });
        await route.fulfill({ response, json: body });
      });
      await page.getByTestId("open-settings").click();
      const tab = page.getByRole("tab", { name: example.tab, exact: true });
      if (await tab.count() === 0) return;
      await tab.click();
      await page.getByLabel(example.language === "en" ? "Filter by service" : "按服务筛选", { exact: true }).selectOption("d1");
      const row = page.getByTestId("resource-rule-d1-account-storage");
      await expect(row).toContainText("0.003");
      await expect(row).toContainText(example.source);
      await expect(row).not.toContainText(example.absent);
      await row.scrollIntoViewIfNeeded();
      await page.screenshot({ path: testInfo.outputPath(`provider-fresh-${example.width}.png`) });
      expired = true;
      await page.getByRole("button", { name: example.refresh, exact: true }).click();
      await expect(row).toContainText(example.absent);
      await expect(row).toContainText(example.expired);
      await expect(row).not.toContainText(example.language === "en" ? "no usage reported by this adapter" : "当前适配器尚未报告用量");
      await expect(row).not.toContainText("0.003");
      await expect(row.locator("meter")).toHaveCount(0);
      await row.scrollIntoViewIfNeeded();
      await page.screenshot({ path: testInfo.outputPath(`provider-expired-${example.width}.png`) });
    });
  }

  it("ignores a delayed response from a closed settings dialog", async ({ actor, page }) => {
    actor.whoCan(CallBrainsApi.using(Instance.baseUrl, Instance.token));
    await actor.attemptsTo(SignIn());
    let release!: () => void;
    const held = new Promise<void>(resolve => { release = resolve; });
    let arrived!: () => void;
    const started = new Promise<void>(resolve => { arrived = resolve; });
    let reads = 0;
    await page.route("**/api/v1/resources", async route => {
      reads++;
      const response = await route.fetch();
      if (reads === 1) {
        const body = await response.json();
        body.settings.policy.fraction = 0.31;
        arrived();
        await held;
        await route.fulfill({ response, json: body });
      } else await route.fulfill({ response });
    });
    try {
      await page.getByTestId("open-settings").click();
      const tab = page.getByRole("tab", { name: "用量与额度", exact: true });
      if (await tab.count() === 0) return;
      await tab.click();
      await started;
      await expect(page.getByTestId("settings-dialog").getByRole("status")).toContainText("正在加载用量与额度");
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("settings-dialog")).toBeHidden();
      await page.getByTestId("open-settings").click();
      const fraction = page.getByRole("spinbutton", { name: "默认额度预算百分比", exact: true });
      await expect(fraction).not.toHaveValue("31");
      await expect(fraction).toBeVisible();
      const current = await fraction.inputValue();
      release();
      await page.unrouteAll({ behavior: "wait" });
      await expect(fraction).toHaveValue(current);
      expect(reads).toBe(2);
    } finally { release(); }
  });
});
