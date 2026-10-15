// Integration checks in a real Chromium window against the running audio app.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");

(async () => {
  const binary =
    process.env.MONITOR_BINARY ||
    path.resolve("desktop/target/debug/monitor-mix");
  const app = spawn(binary, ["--no-browser"], {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, VST3_PATH: process.env.MONITOR_VST3_FIXTURE || "" },
  });
  let browser;
  try {
    const url = await new Promise((resolve, reject) => {
      let out = "";
      const timeout = setTimeout(
        () => reject(Error("App startup timed out")),
        5000,
      );
      app.stdout.on("data", (b) => {
        out += b;
        const m = out.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) {
          clearTimeout(timeout);
          resolve(m[0]);
        }
      });
      app.on("exit", (code) => {
        clearTimeout(timeout);
        reject(Error(`App exited: ${code}`));
      });
    });
    browser = await chromium.launch({
      executablePath: process.env.CHROMIUM || "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    });
    const page = await browser.newPage({
      viewport: { width: 1640, height: 980 },
      acceptDownloads: true,
    });
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(url);
    await page.waitForSelector("#mixer .strip");
    assert.equal(await page.locator("#mixer .strip").count(), 8);
    assert.equal(await page.locator("#buses .strip").count(), 5);
    assert.equal(await page.locator("#mixer .insert-row").count(), 32);
    assert.equal(await page.locator("#buses .insert-row").count(), 20);
    // Printed detents, physical pointer position and exact numeric entry must agree.
    for (const viewport of [
      { width: 1640, height: 980 },
      { width: 1280, height: 800 },
    ]) {
      await page.setViewportSize(viewport);
      for (const value of [0, -6, -12, -24]) {
        const mark = page
          .locator("#mixer .strip")
          .first()
          .locator(".fader-scale span")
          .filter({ hasText: new RegExp(`^${value}$`) });
        await mark.click();
        await page.waitForFunction(
          async (value) =>
            (await (await fetch("/api/config")).json()).channels[0].gain_db ===
            value,
          value,
        );
        assert.equal(
          await page.locator("#fader-value-c0").inputValue(),
          value.toFixed(1),
        );
        // Click the native track at the same vertical coordinate as the printed mark.
        const rect = await page
          .locator("#mixer .fader input")
          .first()
          .boundingBox();
        const tick = await mark.boundingBox();
        await page.mouse.click(
          rect.x + rect.width / 2,
          tick.y + tick.height / 2,
        );
        await page.waitForFunction(
          async (value) =>
            (await (await fetch("/api/config")).json()).channels[0].gain_db ===
            value,
          value,
        );
      }
    }
    await page.setViewportSize({ width: 1640, height: 980 });
    await page.locator("#fader-value-c0").fill("-5.3");
    await page.locator("#fader-value-c0").press("Enter");
    await page.waitForFunction(
      async () =>
        (await (await fetch("/api/config")).json()).channels[0].gain_db ===
        -5.3,
    );
    await page.locator("#fader-value-c0").fill("0");
    await page.locator("#fader-value-c0").press("Enter");
    await page.waitForFunction(
      async () =>
        (await (await fetch("/api/config")).json()).channels[0].gain_db === 0,
    );
    await page.locator("#mixer .fader input").first().press("ArrowDown");
    await page.waitForFunction(
      async () =>
        (await (await fetch("/api/config")).json()).channels[0].gain_db === -1,
    );
    await page.locator("#mixer .fader input").first().press("Shift+ArrowUp");
    await page.waitForFunction(
      async () =>
        (await (await fetch("/api/config")).json()).channels[0].gain_db ===
        -0.9,
    );
    await page.locator("#fader-value-c0").fill("0");
    await page.locator("#fader-value-c0").press("Enter");
    await page.waitForFunction(
      async () =>
        (await (await fetch("/api/config")).json()).channels[0].gain_db === 0,
    );
    assert(
      await page
        .locator("#mixer .strip")
        .first()
        .evaluate((e) => e.getBoundingClientRect().width <= 84),
    );
    await page.click("#demo");
    await page.waitForFunction(
      () => document.getElementById("status").textContent === "DEMO",
    );
    await page.waitForFunction(async () => {
      const s = await (await fetch("/api/status")).json();
      return s.channel_meters.every((p) => p[0] === 0 && p[1] === 0);
    });
    await page.click("#generator-toggle-0");
    await page.waitForFunction(async () => {
      const s = await (await fetch("/api/status")).json();
      return (
        s.channel_meters[0][0] > 0 &&
        s.channel_meters[1][0] === 0 &&
        s.meters_stereo[0][0] > 0
      );
    });
    await page
      .getByRole("button", { name: "Faders de Monitor 1", exact: true })
      .click();
    await page
      .locator("#mixer .fader input")
      .first()
      .evaluate((e) => {
        e.value = "500";
        e.dispatchEvent(new Event("input", { bubbles: true }));
      });
    await page.waitForFunction(async () => {
      const c = await (await fetch("/api/config")).json();
      return c.channels[0].sends_db[0] === -12;
    });
    await page.waitForFunction(async () => {
      const s = await (await fetch("/api/status")).json();
      return s.meters_stereo[1][0] > 0;
    });
    await page.locator("#mixer .strip").nth(1).locator(".strip-header").click();
    await page.selectOption("#generator-source", "pink");
    await page.waitForFunction(async () => {
      const s = await (await fetch("/api/status")).json();
      return (
        s.channel_rms[0][0] > 0 &&
        s.channel_rms[1][0] > 0 &&
        s.channel_rms[2][0] === 0
      );
    });
    assert.match(
      await page.locator("#generator-button-1").textContent(),
      /ROSA/,
    );
    await page.selectOption("#pc-monitor", "1");
    await page.waitForFunction(async () => {
      const c = await (await fetch("/api/config")).json();
      return c.pc_monitor === 1;
    });
    await page.click("#panic");
    await page.waitForFunction(() =>
      document.getElementById("status").textContent.includes("SILENCIO TOTAL"),
    );
    await page.waitForFunction(async () => {
      const s = await (await fetch("/api/status")).json();
      return s.rms_stereo.every((p) => p[0] === 0 && p[1] === 0);
    });
    await page.click("#stop");
    await page.waitForFunction(() =>
      document.getElementById("status").textContent.startsWith("DETENIDO"),
    );
    if (process.env.MONITOR_VST3_FIXTURE) {
      for (let slot = 0; slot < 2; slot++) {
        await page
          .locator("#mixer .strip")
          .first()
          .locator(".insert-slot")
          .nth(slot)
          .click();
        await page.waitForFunction(
          () => document.getElementById("vst-installed").options.length > 0,
        );
        await page.fill("#vst-search", "Fixture");
        await page.selectOption(
          "#vst-installed",
          process.env.MONITOR_VST3_FIXTURE,
        );
        await page.waitForFunction(
          () => document.getElementById("classes").options.length > 0,
        );
        assert.match(
          await page.locator("#classes option").first().textContent(),
          /Fixture Gain/,
        );
        const gain = page
          .locator("#params label")
          .filter({ hasText: "Gain" })
          .locator("input");
        await gain.fill("0.5");
        await gain.dispatchEvent("change");
        await page.click("#applyfx");
        await page.waitForFunction(
          (n) =>
            document.querySelectorAll(
              "#mixer .strip:first-child .insert-row .bypass",
            ).length === n,
          slot + 1,
        );
      }
      assert.equal(
        await page
          .locator("#mixer .strip")
          .first()
          .locator(".insert-name")
          .filter({ hasText: "Fixture Gain" })
          .count(),
        2,
      );
    }
    await page.click("#demo");
    await page.waitForFunction(
      () => document.getElementById("status").textContent === "DEMO",
    );
    if (process.env.MONITOR_VST3_FIXTURE) {
      await page
        .locator("#mixer .strip")
        .first()
        .locator(".bypass")
        .first()
        .click();
      await page.waitForFunction(async () => {
        const c = await (await fetch("/api/config")).json();
        return c.channels[0].inserts[0].bypass === true;
      });
    }
    const downloadPromise = page.waitForEvent("download");
    await page.click("#save");
    const download = await downloadPromise;
    const scene = JSON.parse(await fs.readFile(await download.path(), "utf8"));
    assert.equal(scene.channels[0].sends_db[0], -12);
    assert.equal(scene.channels[1].generator.signal, "pink");
    const unauthorised = await page.request.post(url + "/api/panic", {
      data: {},
    });
    assert.equal(unauthorised.status(), 403);
    await page.screenshot({
      path:
        process.env.MONITOR_SCREENSHOT ||
        "/workspace/scratch/monitor-mix-0.3.png",
      fullPage: true,
    });
    for (const viewport of [
      { width: 1280, height: 800 },
      { width: 1640, height: 980 },
    ]) {
      await page.setViewportSize(viewport);
      assert(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
        "console must not overflow the window",
      );
      assert(
        await page.evaluate(() =>
          [...document.querySelectorAll(".strip-footer")].every(
            (e) => e.getBoundingClientRect().bottom <= innerHeight - 24,
          ),
        ),
        "all channel faders and their readouts must fit above the status bar",
      );
      assert(await page.locator("#panic").isVisible());
    }
    await page.click("#stop");
    await page.waitForFunction(() =>
      document.getElementById("status").textContent.startsWith("DETENIDO"),
    );
    // Loading a 0.1 scene must migrate its insert and default its new generator to off.
    const legacy = structuredClone(scene);
    delete legacy.pc_monitor;
    legacy.channels.forEach((c) => {
      c.fx = c.inserts[0] || null;
      delete c.inserts;
      delete c.generator;
    });
    legacy.buses.forEach((b) => {
      b.fx = b.inserts[0] || null;
      delete b.inserts;
    });
    await page.locator("#scene").setInputFiles({
      name: "legacy-0.1.json",
      mimeType: "application/json",
      buffer: Buffer.from(JSON.stringify(legacy)),
    });
    await page.waitForFunction(async () => {
      const c = await (await fetch("/api/config")).json();
      return c.channels[0].generator.signal === "off" && c.pc_monitor === 0;
    });
    assert.deepEqual(errors, []);
    console.log(
      "PASS: exact printed and numeric faders, installed VST3 discovery, compact DAW strips, 52 insert slots, independent generators, stereo meters, monitor sends, live VST3 bypass, PC audition selection, emergency silence, legacy scenes, export and responsive layout",
    );
  } finally {
    if (browser) await browser.close();
    app.kill("SIGTERM");
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
