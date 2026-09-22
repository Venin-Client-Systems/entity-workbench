import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./ui/tests",
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:1420",
    headless: true,
    launchOptions: { chromiumSandbox: true },
    viewport: { width: 1440, height: 1000 },
  },
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
  },
  reporter: "list",
});
