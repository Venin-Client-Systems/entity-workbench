import { execFileSync } from "node:child_process";

// Both direct Playwright runs and npm test:ui must start from the current Rust
// source. Individual spec files cannot depend on another spec building a helper.
export default function globalSetup() {
  execFileSync(
    "cargo",
    [
      "build",
      "-p",
      "workbench-core",
      "--locked",
      "--bin",
      "ew-dev",
      "--example",
      "native_export_session",
    ],
    { stdio: "inherit" },
  );
}
