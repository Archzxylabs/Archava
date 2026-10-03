import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const npm = process.platform === "win32" ? "npm.cmd" : "npm";
for (const task of ["test", "test:backend", "test:ui", "build", "verify:artifacts"]) {
  console.log(`\nVerifying ${task}`);
  const result = spawnSync(npm, ["run", task], { cwd: root, stdio: "inherit" });
  if (result.status !== 0) { process.exitCode = result.status ?? 1; break; }
}
if (!process.exitCode) console.log("PASS local code and browser checks. Live voice and deployment checks are separate.");
