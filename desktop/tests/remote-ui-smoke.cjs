// Personal-mix UI across independent desktop/mobile browser sessions.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
(async () => {
  const app = spawn("desktop/target/debug/monitor-mix", ["--no-browser"], {
    env: {
      ...process.env,
      MONITOR_REMOTE_BIND: "127.0.0.1",
      MONITOR_LISTEN_HOST: "127.0.0.1",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let browser;
  try {
    const url = await new Promise((resolve, reject) => {
      let out = "";
      const timer = setTimeout(() => reject(Error("startup timeout")), 5000);
      app.stdout.on("data", (b) => {
        out += b;
        const m = out.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) {
          clearTimeout(timer);
          resolve(m[0]);
        }
      });
    });
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });
    const errors = [],
      operator = await browser.newPage({
        viewport: { width: 1500, height: 960 },
      });
    operator.on("pageerror", (e) => errors.push(e.message));
    await fetch(url);
    await operator.goto(url, { waitUntil: "domcontentloaded" });
    await operator.waitForSelector("#mixer .strip");
    await operator.evaluate(async () => {
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -20,
        frequency: 1000,
      };
      cfg.channels[1].generator = {
        signal: "sine",
        level_db: -24,
        frequency: 400,
      };
      cfg.channels[0].sends_db[0] = -12;
      cfg.channels[0].sends_db[1] = -18;
      await publish();
      await api("start", { mode: "demo" });
    });
    await operator.locator("#remote-settings").click();
    await operator.evaluate(async () => {
      lastStatus.remote = await api("remote", { action: "start", port: 0 });
      updateRemoteAdmin(lastStatus.remote);
    });
    await operator.locator("#pref-tab-network").click();
    await operator
      .locator("#network-host")
      .fill("studio.example-tailnet.ts.net");
    await operator.locator("#network-host-apply").click();
    await operator.waitForFunction(
      () => lastStatus.remote.host === "studio.example-tailnet.ts.net",
    );
    await operator.locator("#pref-tab-mix").click();
    await operator.locator("#remote-label").fill("Voz principal");
    await operator.locator("#remote-aux").selectOption("1");
    await operator.locator("#remote-create").click();
    await operator.waitForSelector(".remote-link");
    assert(
      (
        await operator.locator(".remote-link input").first().inputValue()
      ).includes("studio.example-tailnet.ts.net"),
    );
    await operator.locator("#pref-tab-network").click();
    await operator.locator("#network-host").fill("127.0.0.1");
    await operator.locator("#network-host-apply").click();
    await operator.waitForFunction(
      () => lastStatus.remote.host === "127.0.0.1",
    );
    await operator.locator("#pref-tab-mix").click();
    await operator.locator("#remote-label").fill("Baterista");
    await operator.locator("#remote-aux").selectOption("2");
    await operator.locator("#remote-max").fill("-6");
    await operator.locator("#remote-pre").check();
    await operator.locator("#remote-create").click();
    await operator.waitForFunction(
      () => document.querySelectorAll(".remote-link").length === 2,
    );
    const linkA = await operator
        .locator('[aria-label="Enlace de Voz principal"]')
        .inputValue(),
      linkB = await operator
        .locator('[aria-label="Enlace de Baterista"]')
        .inputValue();
    await operator.screenshot({
      path: "/workspace/scratch/monitor-mix-personal-remote-admin.png",
    });
    await operator.locator("[data-close=network-dialog]").click();
    const contextA = await browser.newContext({
        viewport: { width: 1000, height: 800 },
      }),
      contextB = await browser.newContext({
        viewport: { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
      });
    const a = await contextA.newPage(),
      b = await contextB.newPage();
    for (const p of [a, b]) p.on("pageerror", (e) => errors.push(e.message));
    await Promise.all([a.goto(linkA), b.goto(linkB)]);
    await Promise.all([
      a.waitForSelector(".strip"),
      b.waitForSelector(".strip"),
    ]);
    assert.equal(await a.locator(".strip").count(), 8);
    assert.equal(
      await a.locator(".controls button:nth-child(2)").first().isDisabled(),
      true,
    );
    assert.equal(
      await b.locator(".controls button:nth-child(2)").first().isEnabled(),
      true,
    );
    assert((await a.locator(".scale .tick").first().innerText()) === "0");
    const setFader = async (p, value) =>
      p.evaluate((v) => {
        const f = document.querySelector(".fader");
        f.value = toPos(v);
        f.dispatchEvent(new Event("input"));
      }, value);
    await setFader(a, -6);
    await a.waitForFunction(
      () => !busy && !pending.size && state.channels[0].level_db === -6,
    );
    await b.waitForFunction(() => state.channels[0].level_db === -18);
    // Prevent the operator's poll from refreshing its baseline, then edit an unrelated field.
    await operator.evaluate(() => {
      remotePointerDown = true;
    });
    await setFader(a, -9);
    await a.waitForFunction(() => state.channels[0].level_db === -9 && !busy);
    await operator.evaluate(async () => {
      cfg.channels[0].pan = 0.4;
      await publish();
      remotePointerDown = false;
    });
    assert.equal(
      await operator.evaluate(() => cfg.channels[0].sends_db[0]),
      -9,
    );
    await operator.evaluate(() => {
      remotePointerDown = true;
    });
    await setFader(a, -15);
    await a.waitForFunction(() => state.channels[0].level_db === -15 && !busy);
    const saved = operator.waitForEvent("download");
    await operator.evaluate(() => document.querySelector("#save").onclick());
    const scene = JSON.parse(
      await fs.readFile(await (await saved).path(), "utf8"),
    );
    assert.equal(scene.channels[0].sends_db[0], -15);
    await operator.evaluate(() => {
      remotePointerDown = false;
    });
    await a.locator(".controls button").first().click();
    await a.waitForFunction(() => state.channels[0].mute === true && !busy);
    await operator.waitForFunction(() => cfg.channels[0].send_mute[0] === true);
    assert.equal(await operator.evaluate(() => cfg.channels[0].mute), false);
    await a.locator(".controls button").first().click();
    await a.waitForFunction(() => state.channels[0].mute === false && !busy);
    await a.locator(".fader").first().dblclick();
    await a.waitForFunction(() => state.channels[0].level_db === 0 && !busy);
    await b.locator(".controls button:nth-child(2)").first().click();
    await b.waitForFunction(() => state.channels[0].pre === false && !busy);
    await a.screenshot({
      path: "/workspace/scratch/monitor-mix-personal-remote.png",
    });
    await b.screenshot({
      path: "/workspace/scratch/monitor-mix-personal-remote-mobile.png",
    });
    await contextA.setOffline(true);
    await a.waitForFunction(
      () => document.querySelector("#status").textContent === "Sin conexión",
    );
    assert(await a.locator(".fader").first().isDisabled());
    await operator.evaluate(async () => {
      cfg.channels[0].sends_db[0] = -24;
      await publish();
    });
    await contextA.setOffline(false);
    await a.waitForFunction(
      () => connected && state.channels[0].level_db === -24,
    );
    assert.equal(await a.locator(".value").first().inputValue(), "-24");
    await operator.locator("#remote-settings").click();
    await operator
      .locator(".remote-link")
      .filter({ hasText: "Voz principal" })
      .getByRole("button", { name: "Revocar" })
      .click();
    await a.waitForFunction(
      () => document.querySelector("#status").textContent === "Enlace revocado",
    );
    assert(await a.locator(".fader").first().isDisabled());
    assert(await b.locator(".fader").first().isEnabled());
    await operator.locator("[data-close=network-dialog]").click();
    await operator.evaluate(async () => {
      await resizeStructure(9, 4);
    });
    await b.waitForFunction(
      () => document.querySelector("#status").textContent === "Enlace revocado",
    );
    assert.deepEqual(errors, []);
    console.log(
      "PASS: operator session administration, Tailscale hostname links, two isolated personal surfaces, mobile layout, exact faders/reset/mute/PRE permissions, desktop conflict merge and current-state scene export, reconnect without replay, revocation and live topology invalidation",
    );
  } finally {
    await browser?.close();
    app.kill();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
