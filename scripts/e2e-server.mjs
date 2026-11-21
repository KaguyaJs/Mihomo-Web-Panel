import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const data = path.join(root, ".tools", `e2e-${Date.now()}`);
await mkdir(data, { recursive: true });
await writeFile(path.join(root, ".tools", "e2e-current.txt"), data, "utf8");
const config = path.join(data, "config.toml");
await writeFile(
  config,
  `listen = "127.0.0.1:8787"\npublic_origin = "http://127.0.0.1:8787"\ndata_dir = ${JSON.stringify(data.replaceAll("\\", "/"))}\nsecure_cookies = false\n`,
  "utf8",
);
const suffix = process.platform === "win32" ? ".exe" : "";
const children = [
  spawn(
    path.join(root, "target", "debug", "examples", `mock_mihomo${suffix}`),
    [],
    { cwd: root, stdio: "inherit", windowsHide: true },
  ),
  spawn(
    path.join(root, "target", "debug", `mihomo-web-panel${suffix}`),
    ["--config", config, "serve"],
    { cwd: root, stdio: "inherit", windowsHide: true },
  ),
];
for (const child of children)
  child.on("error", (error) => {
    console.error(error.message);
    stop(1);
  });
for (const child of children)
  child.on("exit", (code) => {
    if (code !== null && code !== 0) stop(code);
  });
let stopping = false;
function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  for (const child of children) child.kill();
  process.exit(code);
}
process.on("SIGTERM", () => stop());
process.on("SIGINT", () => stop());
