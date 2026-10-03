import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createServer } from "vite";
import react from "@vitejs/plugin-react-swc";
import { chromium } from "playwright";

const root = fileURLToPath(new URL("../", import.meta.url));
const artifacts = process.env.ARCHAVA_UI_ARTIFACTS || path.join(root, ".tmp-test/ui");
await mkdir(artifacts, { recursive: true });
const server = await createServer({
  root, configFile: false, logLevel: "error", plugins: [react()],
  resolve: { alias: [
    { find: "@livekit/components-react", replacement: path.join(root, "tests/ui/livekit-fixture.tsx") },
    { find: /^\.{1,2}\/lib\/spatius$/, replacement: path.join(root, "tests/ui/spatius-fixture.ts") },
  ] },
  server: { host: "127.0.0.1", port: 0 },
});
await server.listen();
const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await chromium.launch({executablePath: process.env.ARCHAVA_BROWSER_EXECUTABLE || chromium.executablePath(), args: ["--no-sandbox"]});
const baseConfig = {
  chainId: 97, contractAddress: "0x1111111111111111111111111111111111111111",
  paymentTokenAddress: "0x2222222222222222222222222222222222222222",
  livekitReady: true, avatarProvider: "tavus", previewEnabled: true,
  previewSeconds: 120, packMinutes: [60, 300],
};
const results = [];

async function check(name, options, run) {
  const context = await browser.newContext({viewport: {width: options.width || 1440, height: options.height || 900}, reducedMotion: "reduce"});
  if (options.unsupported) await context.addInitScript(() => Reflect.deleteProperty(globalThis, "RTCRtpScriptTransform"));
  if (options.wallet) await context.addInitScript(({ restored }) => {
    let authorized = restored;
    window.ethereum = {
      async request({method}) {
        if (method === "eth_requestAccounts") authorized = true;
        if (method === "eth_accounts" || method === "eth_requestAccounts") return authorized ? ["0x1111111111111111111111111111111111111111"] : [];
        if (method === "eth_chainId") return "0x61";
        throw new Error("UI fixture does not sign or transact");
      }, on() {}, removeListener() {},
    };
  }, { restored: Boolean(options.restored) });
  const page = await context.newPage();
  const errors = [];
  const state = { configRequests: 0, previewRequests: 0, endRequests: 0, preparations: 0, offline: Boolean(options.offline), preparationFailure: false };
  page.on("pageerror", error => errors.push(error.message));
  await context.route("**/*", async route => {
    const request = route.request(), url = new URL(request.url());
    if (url.origin !== origin) { await route.abort(); return; }
    const json = async (value, status = 200) => route.fulfill({status, contentType: "application/json", body: JSON.stringify(value)});
    if (url.pathname === "/api/config") {
      state.configRequests++;
      if (options.configDelay) await new Promise(resolve => setTimeout(resolve, options.configDelay));
      await json(state.offline ? {error: "Simulated offline"} : {...baseConfig, ...options.config}, state.offline ? 503 : 200);
    } else if (url.pathname === "/api/quote") {
      const minutes = Number(url.searchParams.get("minutes"));
      await json({minutes, wei: "5000000000000000000", tokenAmount: minutes === 300 ? "20" : "5", symbol: "mUSDT"});
    } else if (url.pathname === "/api/access") {
      await json({active: true, remainingSeconds: 3600, purchasedMinutes: 60, usedSeconds: 0, reservedSeconds: 0});
    } else if (url.pathname === "/api/preview" || url.pathname === "/api/session") {
      state.previewRequests++;
      await json(options.call ? {avatarProvider: "tavus", serverUrl: "wss://room.invalid", token: "ui-fixture", ticket: "ui-fixture", preview: true, endsAt: Math.floor(Date.now() / 1000) + (options.expiresIn || 120)} : {error: "Simulated unavailable preview"}, options.call ? 200 : 503);
    } else if (url.pathname === "/api/session/end") { state.endRequests++; await json({ok: true}); }
    else if (url.pathname === "/__ui/avatar-preparation") { state.preparations++; await json({ok: true}, state.preparationFailure ? 503 : 200); }
    else if (url.pathname.startsWith("/api/") || request.method() === "POST") await json({error: "UI fixture does not authorize this action"}, 403);
    else await route.continue();
  });
  try {
    await page.goto(origin + (options.path || "/"), {waitUntil: "domcontentloaded"});
    await page.locator("main").waitFor();
    await run(page, state);
    assert.deepEqual(errors, [], `${name}: page JavaScript errors`);
    results.push({name, passed: true});
    console.log(`PASS ${name}`);
  } catch (error) {
    await page.screenshot({path: path.join(artifacts, `${name}-failure.png`), fullPage: true});
    results.push({name, passed: false, error: error.message});
    console.error(`FAIL ${name}: ${error.message}`);
  } finally { await context.close(); }
}

try {
  for (const [width, height] of [[1440, 900], [768, 1024], [390, 844], [320, 740]]) {
    await check(`layout-${width}`, {width, height}, async page => {
      await page.locator(".hero-neon-pill-btn").filter({hasText: "Talk to Ava"}).waitFor();
      const measurements = await page.evaluate(() => {
        const box = el => ({left: el.getBoundingClientRect().left, right: el.getBoundingClientRect().right, top: el.getBoundingClientRect().top, bottom: el.getBoundingClientRect().bottom});
        return {width: innerWidth, scrollWidth: document.documentElement.scrollWidth, hints: [...document.querySelectorAll(".hero-preview-hint,.hero-access-hint,.hero-jump-link")].map(box), hud: box(document.querySelector(".hero-left-hud")), entry: box(document.querySelector(".hero-demo-entry")), hero: box(document.querySelector(".video-hero-canvas"))};
      });
      assert.equal(measurements.scrollWidth, width);
      for (const r of measurements.hints) assert.ok(r.left >= 0 && r.right <= width, "hero text is clipped");
      if (width <= 768) assert.ok(measurements.hud.bottom <= measurements.entry.top, "hero content overlaps");
      assert.equal(await page.locator("[role=button] button").count(), 0);
      assert.equal(await page.locator(".catalog-avatar-poster").count(), 1);
      assert.equal(await page.locator(".catalog-avatar-card").count(), 0);
      await page.screenshot({path: path.join(artifacts, `home-${width}.png`)});
      await page.locator("#catalog-section").scrollIntoViewIfNeeded();
      await page.screenshot({path: path.join(artifacts, `catalog-${width}.png`)});
    });
  }
  for (const key of ["Enter", "Space"]) {
    await check(`talk-keyboard-${key}`, {}, async (page, state) => {
      const button = page.locator(".catalog-demo-entry button");
      await button.filter({hasText: "Talk to Ava"}).waitFor();
      await button.focus();
      await button.press(key);
      await page.getByRole("alert").waitFor();
      assert.equal(state.previewRequests, 1);
      assert.match(await page.getByRole("alert").innerText(), /warming up/);
      const inViewport = await page.getByRole("alert").evaluate(el => el.getBoundingClientRect().top >= 0 && el.getBoundingClientRect().bottom <= innerHeight);
      assert.ok(inViewport, "error feedback must be visible");
    });
  }
  await check("wallet-error-visible", {path: "/build", width: 390, height: 844}, async page => {
    await page.getByRole("button", {name: "Connect wallet", exact: true}).click();
    await page.getByRole("alert").waitFor();
    assert.match(await page.getByRole("alert").innerText(), /Install a wallet/);
    assert.ok(await page.getByRole("alert").evaluate(el => el.getBoundingClientRect().top >= 0 && el.getBoundingClientRect().bottom <= innerHeight));
    await page.getByRole("button", {name: "Dismiss notification"}).click();
    assert.equal(await page.getByRole("alert").count(), 0);
  });
  await check("offline-retry", {offline: true, width: 390, height: 844}, async (page, state) => {
    await page.getByRole("button", {name: "Retry connection"}).waitFor();
    assert.match(await page.locator("#hero-demo-note").innerText(), /offline/);
    state.offline = false;
    await page.getByRole("button", {name: "Retry connection"}).click();
    await page.locator(".hero-neon-pill-btn").filter({hasText: "Talk to Ava"}).waitFor();
    assert.ok(state.configRequests >= 2);
    assert.equal(await page.getByRole("alert").count(), 0);
  });
  await check("checking-state", {configDelay: 500}, async page => {
    await page.locator(".hero-neon-pill-btn").filter({hasText: "Checking availability"}).waitFor();
    assert.ok(await page.locator(".hero-neon-pill-btn").isDisabled());
    await page.locator(".hero-neon-pill-btn").filter({hasText: "Talk to Ava"}).waitFor();
  });
  await check("unsupported-before-click", {unsupported: true, config: {avatarProvider: "spatius"}, width: 320, height: 740}, async (page, state) => {
    await page.locator(".hero-neon-pill-btn").filter({hasText: "Browser not supported"}).waitFor();
    assert.ok(await page.locator(".hero-neon-pill-btn").isDisabled());
    assert.match(await page.locator("#hero-demo-note").innerText(), /Chrome or Edge/);
    assert.equal(state.previewRequests, 0);
    assert.equal(state.preparations, 0);
    assert.ok(await page.evaluate(() => document.querySelector(".hero-left-hud").getBoundingClientRect().bottom <= document.querySelector(".hero-demo-entry").getBoundingClientRect().top));
    await page.screenshot({path: path.join(artifacts, "unsupported-320.png")});
  });
  await check("build-does-not-prepare-avatar", {path: "/build", config: {avatarProvider: "spatius"}}, async (page, state) => {
    await page.getByRole("button", {name: "300 min"}).click();
    await page.waitForFunction(() => document.querySelector(".build-quote")?.textContent.includes("20 mUSDT"));
    assert.equal(await page.getByRole("button", {name: "300 min"}).getAttribute("aria-pressed"), "true");
    assert.equal(state.preparations, 0);
  });
  await check("home-keeps-avatar-preload", {config: {avatarProvider: "spatius"}}, async (page, state) => {
    await page.locator(".avatar-preparation-note").filter({hasText: "ready to connect"}).waitFor();
    assert.ok(state.preparations > 0);
    assert.equal(state.previewRequests, 0, "preload must not start a session");
  });
  await check("developer-dialog-keyboard", {wallet: true}, async page => {
    await page.locator(".catalog-paid-access summary").click();
    await page.getByRole("button", {name: "Connect wallet for paid access"}).click();
    const trigger = page.getByRole("button", {name: "Open API key dashboard", exact: true});
    await trigger.click();
    const dialog = page.getByRole("dialog", {name: "Developer access."});
    await dialog.waitFor();
    assert.ok(await dialog.evaluate(el => el.matches(":modal")));
    assert.ok(await page.evaluate(() => !!document.activeElement.closest("#developer-access")));
    for (let i = 0; i < 14; i++) {
      await page.keyboard.press("Tab");
      assert.ok(await page.evaluate(() => !!document.activeElement.closest("#developer-access")), "focus escaped dialog");
    }
    await page.screenshot({path: path.join(artifacts, "developer-dialog.png")});
    await page.keyboard.press("Escape");
    await dialog.waitFor({state: "detached"});
    assert.ok(await trigger.evaluate(el => el === document.activeElement), "focus was not restored");
    assert.notEqual(await page.evaluate(() => document.body.style.overflow), "hidden");
  });
  await check("restore-authorized-wallet", {path: "/build", wallet: true, restored: true}, async page => {
    await page.getByRole("button", {name: "Open API key dashboard"}).waitFor();
    assert.equal(await page.getByRole("button", {name: "Connect wallet", exact: true}).count(), 0);
  });
  await check("copy-code-success-and-failure", {path: "/build", width: 390, height: 844}, async page => {
    await page.evaluate(() => {
      window.__copiedExample = "";
      Object.defineProperty(navigator, "clipboard", {configurable: true, value: {async writeText(value) { window.__copiedExample = value; }}});
    });
    await page.getByRole("button", {name: "Copy session API example"}).click();
    assert.match(await page.evaluate(() => window.__copiedExample), /POST|POST.*|method: "POST"/);
    assert.match(await page.getByRole("button", {name: "Copy session API example"}).innerText(), /Copied/);
    await page.getByRole("region", {name: "Session API JavaScript example"}).focus();
    assert.ok(await page.getByRole("region", {name: "Session API JavaScript example"}).evaluate(el => el === document.activeElement));
    await page.evaluate(() => Object.defineProperty(navigator, "clipboard", {configurable: true, value: {async writeText() {throw new Error("Simulated denied permission");}}}));
    await page.getByRole("button", {name: "Copy contract interface"}).click();
    await page.getByRole("alert").waitFor();
    assert.match(await page.getByRole("alert").innerText(), /copy it manually/);
    await page.screenshot({path: path.join(artifacts, "copy-feedback.png")});
  });
  await check("persistent-call-controls", {call: true, width: 390, height: 844}, async (page, state) => {
    await page.locator(".catalog-demo-entry button").filter({hasText: "Talk to Ava"}).click();
    const dock = page.getByRole("complementary", {name: "Active call controls"});
    await dock.waitFor();
    await page.waitForFunction(() => !!window.__ARCHAVA_UI_ROOM);
    await page.waitForFunction(() => {
      const room = document.getElementById("live-avatar-room").getBoundingClientRect();
      return Math.abs(room.top + room.height / 2 - innerHeight / 2) < 5;
    });
    await page.evaluate(() => window.__ARCHAVA_UI_ROOM.emit(
      "dataReceived", new TextEncoder().encode(JSON.stringify({section: "business-section"})),
      {isAgent: true}, 0, "archava.navigation",
    ));
    await page.waitForFunction(() => {
      const section = document.getElementById("business-section").getBoundingClientRect();
      return section.top >= 0 && section.top <= innerHeight * 0.4;
    });
    await page.waitForFunction(() => document.querySelector(".live-site-awareness")?.textContent.includes("AVA HAS THIS SECTION") && document.querySelector(".live-site-awareness")?.textContent.includes("For business"));
    assert.ok(await dock.evaluate(el => el.parentElement === document.body && el.getBoundingClientRect().bottom <= innerHeight));
    await dock.getByRole("button", {name: "Mute microphone", exact: true}).click();
    await dock.getByRole("button", {name: "Unmute microphone", exact: true}).waitFor();
    await dock.getByRole("button", {name: "Unmute microphone", exact: true}).click();
    await dock.getByRole("button", {name: "Mute microphone", exact: true}).waitFor();
    await page.screenshot({path: path.join(artifacts, "call-controls-mobile.png")});
    await dock.getByRole("button", {name: "Back to Ava"}).click();
    assert.ok(await page.locator("#live-avatar-room").evaluate(el => el === document.activeElement));
    await dock.getByRole("button", {name: "End conversation"}).click();
    await dock.waitFor({state: "detached"});
    await page.locator("#post-call-panel").waitFor();
    assert.equal(state.endRequests, 1, "end-call cleanup must run once");
  });
  await check("call-expiry-next-step", {call: true, expiresIn: 2}, async (page, state) => {
    await page.locator(".catalog-demo-entry button").filter({hasText: "Talk to Ava"}).click();
    await page.locator("#post-call-panel").waitFor();
    assert.equal(await page.locator(".live-call-dock").count(), 0);
    assert.equal(state.endRequests, 1);
  });
  for (const [name, route, width] of [["home", "/", 1440], ["home-mobile", "/", 390], ["build-mobile", "/build", 390]]) {
    await check(`accessibility-${name}`, {path: route, width, height: 900}, async page => {
      await page.locator(route === "/" ? ".catalog-demo-entry button" : ".build-quote").waitFor();
      await page.addScriptTag({path: fileURLToPath(import.meta.resolve("axe-core/axe.min.js"))});
      const result = await page.evaluate(async () => {
        const result = await axe.run(document, {runOnly: {type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa", "best-practice"]}});
        return {violations: result.violations.map(v => ({id: v.id, impact: v.impact, nodes: v.nodes.map(n => ({target: n.target, failureSummary: n.failureSummary}))})), incomplete: result.incomplete.map(v => ({id: v.id, count: v.nodes.length}))};
      });
      await writeFile(path.join(artifacts, `axe-${name}.json`), JSON.stringify(result, null, 2));
      assert.deepEqual(result.violations, [], "automated accessibility violations");
    });
  }
} finally {
  await browser.close();
  await server.close();
  await writeFile(path.join(artifacts, "results.json"), JSON.stringify(results, null, 2));
}
const passed = results.filter(result => result.passed).length;
console.log(`${passed}/${results.length} UI checks passed. Artifacts: ${artifacts}`);
if (passed !== results.length) process.exitCode = 1;
