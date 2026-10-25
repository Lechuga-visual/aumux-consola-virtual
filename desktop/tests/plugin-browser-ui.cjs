// End-to-end checks against real native DSP and a real VST3 fixture; no mocked APIs.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const assert = require("node:assert/strict");
const path = require("node:path");
const fs = require("node:fs/promises");
(async () => {
  const data = await fs.mkdtemp("/workspace/scratch/aumux-plugin-browser-");
  const app = spawn(
    process.env.MONITOR_BINARY || path.resolve("desktop/target/debug/aumux"),
    ["--no-browser"],
    {
      env: {
        ...process.env,
        MONITOR_MIX_DATA_DIR: data,
        VST3_PATH: process.env.MONITOR_VST3_FIXTURE || "",
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  let browser;
  try {
    const url = await new Promise((resolve, reject) => {
      let out = "";
      const timer = setTimeout(() => reject(Error("startup timeout")), 10000);
      app.stdout.on("data", (b) => {
        out += b;
        const m = out.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) {
          clearTimeout(timer);
          resolve(m[0]);
        }
      });
      app.on("exit", (n) => {
        clearTimeout(timer);
        reject(Error("exit " + n));
      });
    });
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });
    const context = await browser.newContext({
      viewport: { width: 1440, height: 980 },
    });
    context.setDefaultTimeout(12000);
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(url);
    await page.waitForSelector("#mixer .strip");
    const empty = (area, index, slot) =>
      page
        .locator(
          `${area} .strip[data-strip="${area === "#mixer" ? "c" : "b"}${index}"]`,
        )
        .locator(".insert-slot")
        .nth(slot);
    console.log(
      "Quick menu: categories, no inspection during search, direct native insertion",
    );
    let inspected = 0;
    page.on("request", (r) => {
      if (r.url().endsWith("/api/inspect")) inspected++;
    });
    await empty("#mixer", 0, 0).click();
    await page.waitForFunction(() => nativePluginEntries.length === 21);
    assert(!(await page.locator("#fx").isVisible()));
    assert.equal(
      await page
        .locator("#quick-fx")
        .evaluate((e) => e.classList.contains("has-results")),
      false,
    );
    await page
      .locator('#quick-fx-categories [data-category="dynamics"]')
      .hover();
    assert.equal(await page.locator("#quick-fx-results button").count(), 3);
    assert.equal(inspected, 0);
    await fs.mkdir("docs/screenshots/0.13.3", { recursive: true });
    await page.screenshot({
      path: "docs/screenshots/0.13.3/01-menu-rapido.png",
    });
    await page
      .locator('#quick-fx-results [data-plugin-id="native/mda/dynamics"]')
      .click();
    await page.waitForFunction(
      () =>
        cfg.channels[0].inserts[0]?.id === "native/mda/dynamics" &&
        !quickFx.open,
    );
    assert(
      !(await page.locator("#fx").isVisible()),
      "quick add must not open editor",
    );
    console.log(
      "Full browser in an independent FX window: filters, favorites, recategorization and real VST3",
    );
    const popupEvent = page.waitForEvent("popup");
    await empty("#mixer", 0, 0).click();
    const fx = await popupEvent;
    fx.on("pageerror", (e) => errors.push(e.message));
    await fx.waitForSelector("#params .rotary-control");
    await fx.locator("#fx-browser > summary").click();
    await fx.waitForFunction(() => pluginEntries.length >= 21);
    await fx.locator('#plugin-categories [data-category="dynamics"]').click();
    assert.equal(await fx.locator(".plugin-result").count(), 3);
    await fx
      .locator(".plugin-result")
      .filter({ hasText: "MDA Limiter" })
      .locator(".plugin-favorite")
      .click();
    await fx.locator('#plugin-categories [data-category="favorites"]').click();
    assert.equal(await fx.locator(".plugin-result").count(), 1);
    await fx.locator(".plugin-result-name").click();
    await fx.locator("#plugin-category-override").selectOption("tools");
    await fx.locator('#plugin-categories [data-category="tools"]').click();
    assert(
      await fx
        .locator(".plugin-result-name")
        .filter({ hasText: "MDA Limiter" })
        .isVisible(),
    );
    await fx.locator('#plugin-categories [data-category="all"]').click();
    await fx.locator("#plugin-format").selectOption("native");
    await fx.locator("#plugin-vendor").selectOption("MDA");
    await fx.locator("#plugin-search").fill("limiter");
    assert.equal(await fx.locator(".plugin-result").count(), 1);
    await fx.locator("#plugin-search").fill("");
    await fx.locator("#plugin-vendor").selectOption("");
    await fx.locator("#plugin-format").selectOption("all");
    await fx.screenshot({
      path: "docs/screenshots/0.13.3/02-navegador-plugins.png",
    });
    await fx.locator("#plugin-search").fill("limiter");
    await fx.locator(".plugin-result-name").click();
    assert.equal(
      await fx.locator("#plugin-use").textContent(),
      "Reemplazar insert",
    );
    await fx.locator("#plugin-use").click();
    await page.waitForFunction(
      () => cfg.channels[0].inserts[0]?.id === "native/mda/limiter",
    );
    await fx.waitForFunction(() => $("fx-title").textContent === "MDA Limiter");
    const closed = fx.waitForEvent("close");
    await fx.click("#closefx");
    await closed;
    await empty("#buses", 1, 0).click();
    await page.fill("#quick-fx-search", "overdrive");
    await page
      .locator('#quick-fx-results [data-plugin-id="native/mda/overdrive"]')
      .click();
    try {
      await page.waitForFunction(
        () => cfg.buses[1].inserts[0]?.id === "native/mda/overdrive",
      );
    } catch (e) {
      console.error(
        await page.evaluate(() => ({
          context: quickFxContext,
          buses: cfg.buses.map((b) => ({ name: b.name, inserts: b.inserts })),
          status: $("quick-fx-status").textContent,
          error: $("error").textContent,
        })),
      );
      throw e;
    }
    await empty("#buses", 0, 0).click();
    await page.fill("#quick-fx-search", "freevox");
    await page
      .locator('#quick-fx-results [data-plugin-id="native/freevox8"]')
      .click();
    await page.waitForFunction(
      () => cfg.buses[0].inserts[0]?.id === "native/freevox8",
    );
    if (process.env.MONITOR_VST3_FIXTURE) {
      await empty("#mixer", 1, 0).click();
      await page.fill("#quick-fx-search", "Fixture");
      await page.waitForSelector("#quick-fx-results button");
      await page.locator("#quick-fx-results button").first().click();
      await page.waitForFunction(() =>
        cfg.channels[1].inserts[0]?.id.startsWith("vst3:"),
      );
      assert.equal(inspected, 1);
    }
    // Independent windows share browser preferences, including overrides/favorites.
    await empty("#mixer", 2, 0).click();
    await page
      .locator('#quick-fx-categories [data-category="favorites"]')
      .click();
    assert.match(
      await page.locator("#quick-fx-results").textContent(),
      /MDA Limiter/,
    );
    await page
      .locator('#quick-fx-categories [data-category="recents"]')
      .click();
    assert.match(
      await page.locator("#quick-fx-results").textContent(),
      /FreeVox8/,
    );
    // An open menu must never overwrite a chain changed by another surface.
    await page.fill("#quick-fx-search", "dynamics");
    await page.evaluate(async () => {
      cfg.channels[2].inserts = [structuredClone(cfg.channels[0].inserts[0])];
      await publish();
    });
    await page
      .locator('#quick-fx-results [data-plugin-id="native/mda/dynamics"]')
      .click();
    await page.waitForFunction(() =>
      $("quick-fx-status").textContent.includes("cadena cambió"),
    );
    assert.equal(
      await page.evaluate(() => cfg.channels[2].inserts[0].id),
      "native/mda/limiter",
    );
    await page.keyboard.press("Escape");
    // Mobile layout and More plugins launch preserve the exact target.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.evaluate(() => {
      document.body.classList.add("touch-mode");
      openQuickFx($("mixer").querySelector(".insert-slot"), "channel", 3, 0);
    });
    await page.locator('#quick-fx-categories [data-category="delay"]').click();
    const rect = await page.locator("#quick-fx").boundingBox();
    assert(
      rect.x >= 0 && rect.x + rect.width <= 391 && rect.y + rect.height <= 845,
    );
    await page.fill("#quick-fx-search", "delay");
    await page
      .locator('#quick-fx-results [data-plugin-id="native/mda/delay"]')
      .click();
    await page.waitForFunction(
      () => cfg.channels[3].inserts[0]?.id === "native/mda/delay",
    );
    await page.setViewportSize({ width: 1440, height: 980 });
    await empty("#mixer", 4, 0).click();
    const moreEvent = page.waitForEvent("popup");
    await page.click("#quick-fx-more");
    const more = await moreEvent;
    await more.waitForSelector("#plugin-library");
    await more.locator("#plugin-search").fill("overdrive");
    await more.locator(".plugin-result-name").dblclick();
    await page.waitForFunction(
      () => cfg.channels[4].inserts[0]?.id === "native/mda/overdrive",
    );
    await more.close();
    await page.evaluate(async () => {
      await api("start", { mode: "demo" });
    });
    await page.waitForFunction(() => running);
    await page.evaluate(() =>
      openQuickFx($("mixer").querySelector(".insert-slot"), "channel", 5, 0),
    );
    await page.fill("#quick-fx-search", "dynamics");
    assert(
      !(await page.locator("#quick-fx-results button").first().isDisabled()),
    );
    await page
      .locator('#quick-fx-results [data-plugin-id="native/mda/dynamics"]')
      .click();
    await page.waitForFunction(
      () => cfg.channels[5].inserts[0]?.id === "native/mda/dynamics" && running,
    );
    await page.keyboard.press("Escape");
    const blocksBefore = await page.evaluate(
      () => lastStatus.performance.blocks,
    );
    await empty("#mixer", 5, 1).click();
    await page.fill("#quick-fx-search", "delay");
    await page.screenshot({ path: "docs/aumux-0.13.3-fx-en-vivo.png" });
    await page
      .locator('#quick-fx-results [data-plugin-id="native/mda/delay"]')
      .click();
    await page.waitForFunction(() => cfg.channels[5].inserts.length === 2);
    const liveEvent = page.waitForEvent("popup");
    await empty("#mixer", 5, 0).click();
    const live = await liveEvent;
    live.on("pageerror", (e) => errors.push(e.message));
    await live.waitForSelector("#params .rotary-control");
    assert(!(await live.locator("#fxdown").isDisabled()));
    await live.locator("#fxdown").click();
    await page.waitForFunction(
      () => cfg.channels[5].inserts[0].id === "native/mda/delay" && running,
    );
    await live.waitForFunction(() => selectedFx.slot === 1);
    await live.locator("#removefx").click();
    await page.waitForFunction(
      () => cfg.channels[5].inserts.length === 1 && running,
    );
    await live.close();
    if (process.env.MONITOR_VST3_FIXTURE) {
      await empty("#mixer", 6, 0).click();
      await page.fill("#quick-fx-search", "Fixture");
      await page.locator("#quick-fx-results button").first().click();
      await page.waitForFunction(
        () => cfg.channels[6].inserts[0]?.id.startsWith("vst3:") && running,
      );
      await empty("#buses", 1, 1).click();
      await page.fill("#quick-fx-search", "Fixture");
      await page.locator("#quick-fx-results button").first().click();
      await page.waitForFunction(
        () => cfg.buses[1].inserts[1]?.id.startsWith("vst3:") && running,
      );
    }
    await page.waitForFunction(
      (before) =>
        lastStatus.performance.blocks > before && lastStatus.mode === "DEMO",
      blocksBefore,
    );
    await page.evaluate(() => api("stop"));
    await page.reload();
    await page.waitForSelector("#mixer .strip");
    await empty("#mixer", 7, 0).click();
    await page
      .locator('#quick-fx-categories [data-category="favorites"]')
      .click();
    await page.waitForSelector(
      '#quick-fx-results [data-plugin-id="native/mda/limiter"]',
    );
    await page.keyboard.press("Escape");
    assert.deepEqual(errors, []);
    console.log(
      "PASS: quick and full plugin browsers, actual native/VST3 insertion, shared preferences, stale-chain guard and mobile bounds",
    );
  } finally {
    await browser?.close();
    app.kill("SIGTERM");
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
