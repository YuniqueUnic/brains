import { defineConfig } from "@playwright/test";

const localUrl = "http://127.0.0.1:4187/brains/";

export default defineConfig({
  testDir: "./demo",
  timeout: 45_000,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: process.env.BRAINS_DEMO_URL ?? localUrl,
    browserName: "chromium",
    viewport: { width: 1440, height: 900 },
    locale: "zh-CN",
    permissions: ["clipboard-read", "clipboard-write"],
    screenshot: "only-on-failure",
  },
  webServer: process.env.BRAINS_DEMO_URL ? undefined : {
    command: "pnpm --dir ../../apps/web exec vite preview --host 127.0.0.1 --port 4187 --strictPort --base /brains/",
    url: localUrl,
    reuseExistingServer: !process.env.CI,
  },
  reporter: [["line"]],
});
