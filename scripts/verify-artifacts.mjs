import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = fileURLToPath(new URL("../", import.meta.url));
const dist = path.join(root, "dist");
try {
  const html = await readFile(path.join(dist, "index.html"), "utf8");
  const scripts = [...html.matchAll(/<script[^>]*src="([^"]+)"/g)].map(match => match[1]);
  assert.ok(scripts.length, "Missing frontend script");
  for (const script of scripts) {
    assert.ok(script.startsWith("/assets/") && !script.includes(".."), "Unexpected frontend script path");
    await readFile(path.join(dist, script));
  }
  for (const name of await readdir(path.join(dist, "assets"))) {
    if (!/\.(js|css)$/.test(name)) continue;
    const source = await readFile(path.join(dist, "assets", name), "utf8");
    assert.doesNotMatch(source, /__ARCHAVA_UI_ROOM|ui-fixture|__ui\/avatar-preparation/, "UI fixture in production bundle");
    assert.doesNotMatch(source, /VITE_(?:LIVEKIT_API_SECRET|SPATIUS_API_KEY|GEMINI_API_KEY|TAVUS_API_KEY)/, "Private provider env in browser code");
  }
  console.log("PASS production entrypoints and fixture-free browser assets. Operator env files were not read.");
} catch (error) {
  console.error(`FAIL artifact verification: ${error.message}`);
  process.exitCode = 1;
}
