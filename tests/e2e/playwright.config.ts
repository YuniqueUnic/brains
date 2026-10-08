import type { Config } from "playwright/test";

const PORT = 18799;
const BASE_URL = process.env.BRAINS_BASE_URL ?? `http://127.0.0.1:${PORT}`;
const TOKEN = process.env.BRAINS_E2E_TOKEN ?? "brains-e2e-root-token-0000000000000000";
// Keep the e2e data dir outside the cargo target directory: nothing in the
// suite depends on cargo's build cache.
const DATA_DIR = process.env.BRAINS_E2E_DATA_DIR ?? "/tmp/brains-e2e-data";

/**
 * Browser E2E for Brains's local stack (design §18 M3): the real
 * brains-server (Rust) serves the built web app, so every scenario exercises
 * the production HTTP contract end to end.
 */
const config: Config = {
  testDir: "./specs",
  timeout: 60_000,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  use: {
    baseURL: BASE_URL,
    browserName: "chromium",
    viewport: { width: 1440, height: 900 },
  },
  webServer:
    BASE_URL !== `http://127.0.0.1:${PORT}`
      ? undefined
      : {
          command: [
          "sh -c",
          JSON.stringify(
            [
              `rm -rf ${DATA_DIR}`,
              "&&",
              process.env.BRAINS_SERVER_BIN ?? "../../target/debug/brains-server",
              `--data-dir ${DATA_DIR}`,
              `--port ${PORT}`,
              `--token ${TOKEN}`,
              `--web-dist ${process.env.BRAINS_WEB_DIST ?? "../../apps/web/dist"}`,
            ].join(" "),
          ),
        ].join(" "),
          url: `${BASE_URL}/api/v1/capabilities`,
          // 401 from the auth gate still proves the server is listening.
          reuseExistingServer: !process.env.CI,
          timeout: 30_000,
        },
  reporter: [["line"], ["html", { open: "never", outputFolder: "report" }]],
};

export default config;
export { TOKEN };
