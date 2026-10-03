import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

const root = fileURLToPath(new URL("../", import.meta.url));
const venv = resolve(root, ".venv/bin/python");
const python = process.env.ARCHAVA_PYTHON || (existsSync(venv) ? venv : "python3");
// Only OS/runtime settings cross into tests. Operator provider credentials do not.
const env = Object.fromEntries(["PATH", "LANG", "LC_ALL", "TMPDIR", "TMP", "TEMP", "SYSTEMROOT", "WINDIR"]
  .filter(key => process.env[key] !== undefined).map(key => [key, process.env[key]]));
const result = spawnSync(python, ["-m", "unittest", "discover", "-s", ".", "-p", "test_*.py", "-q"], {
  cwd: resolve(root, "backend"), env, stdio: "inherit",
});
if (result.error) console.error("Backend tests could not start. Install backend/requirements.txt and select Python with ARCHAVA_PYTHON.");
process.exitCode = result.status ?? 1;
