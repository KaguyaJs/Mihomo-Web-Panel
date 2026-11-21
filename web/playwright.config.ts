import { defineConfig, devices } from "@playwright/test";
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 45000,
  use: {
    baseURL: "http://127.0.0.1:8787",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1440, height: 1000 },
      },
    },
  ],
  webServer: {
    command: "node ../scripts/e2e-server.mjs",
    url: "http://127.0.0.1:8787/api/health",
    reuseExistingServer: false,
    timeout: 45000,
  },
  reporter: [["list"], ["html", { open: "never" }]],
});
