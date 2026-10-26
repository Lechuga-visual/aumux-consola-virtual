// Actual application, popup windows, 8 native inserts, context actions and multitrack controls.
const { chromium } = require("playwright-core"),
  { spawn } = require("node:child_process"),
  assert = require("node:assert/strict"),
  fs = require("node:fs/promises");
(async () => {
  const data = await fs.mkdtemp("/workspace/scratch/aumux-studio-ui-");
  const app = spawn(
    process.env.MONITOR_BINARY || "desktop/target/debug/aumux",
    ["--no-browser"],
    {
      env: { ...process.env, MONITOR_MIX_DATA_DIR: data },
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
        const match = out.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (match) {
          clearTimeout(timer);
          resolve(match[0]);
        }
      });
      app.on("exit", (n) => {
        clearTimeout(timer);
        reject(Error("app exit " + n));
      });
    });
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });
    const context = await browser.newContext({
      viewport: { width: 1640, height: 980 },
      hasTouch: true,
    });
    context.setDefaultTimeout(12000);
    const page = await context.newPage(),
      errors = [];
    context.on("page", (p) => p.on("pageerror", (e) => errors.push(e.message)));
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(url);
    await page.waitForSelector("#mixer .strip");
    await page.evaluate(async () => {
      window.popupCalls = [];
      const original = window.open;
      window.open = function (url, target, features) {
        popupCalls.push({ url, features });
        return original.call(window, url, target, features);
      };
      cfg.channels[0].inserts = Array.from({ length: 8 }, (_, i) => ({
        id: "native/mda/dither",
        path: "",
        params: {},
        name: "MDA Dither",
        instance: "studio-fx-" + i,
        bypass: false,
      }));
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -12,
        frequency: 1000,
      };
      cfg.channels[0].gain_db = 0;
      cfg.channels[0].compressor = {
        ...defaultCompressor(),
        bypass: false,
        threshold: -40,
        ratio: 8,
        attack: 0.1,
      };
      await publish();
      render();
    });
    assert.equal(
      await page.locator('#mixer .strip[data-strip="c0"] .insert-row').count(),
      8,
    );
    await page.evaluate(async () => {
      const invalid = structuredClone(cfg);
      invalid.channels[0].inserts.push({
        ...invalid.channels[0].inserts[0],
        instance: "ninth",
      });
      try {
        await api("config", invalid);
        throw Error("ninth insert unexpectedly accepted");
      } catch (e) {
        if (e.message.includes("unexpectedly")) throw e;
      }
      $("error").textContent = "";
    });
    const fxEvent = page.waitForEvent("popup");
    await page
      .locator('#mixer .strip[data-strip="c0"] .insert-slot')
      .first()
      .click();
    const fx = await fxEvent;
    await fx.waitForSelector('#fx-chain-list [data-slot="7"]');
    await fs.mkdir("docs/screenshots/0.14.0", { recursive: true });
    await fx.setViewportSize({ width: 864, height: 648 });
    await fx.screenshot({
      path: "docs/screenshots/0.14.0/00-ocho-inserts.png",
    });
    assert.equal(await fx.locator("#fx-chain-list .fx-chain-row").count(), 8);
    await fx.locator('#fx-chain-list [data-slot="7"]').click();
    await fx.waitForFunction(() => selectedFx.slot === 7);
    await fx.keyboard.press("Delete");
    try {
      await page.waitForFunction(() => cfg.channels[0].inserts.length === 7);
    } catch (e) {
      console.error(
        "main",
        await page.evaluate(() => ({
          pointer: remotePointerDown,
          text: panelTextEditing(),
          configRequests,
          publishRunning,
          structureBusy,
          syncedRemoteRevision,
          revision: lastStatus?.remote?.revision,
          focused: document.activeElement?.tagName,
          count: cfg.channels[0].inserts.length,
          server: serverConfig.channels[0].inserts.length,
          changed: JSON.stringify(cfg) !== JSON.stringify(serverConfig),
          status: $("status").textContent,
        })),
      );
      console.error(
        await fx.evaluate(() => ({
          focus: document.activeElement?.outerHTML?.slice(0, 250),
          selected: selectedFx,
          count: cfg.channels[0].inserts.length,
          server: serverConfig.channels[0].inserts.length,
          error: $("fxerror").textContent,
          panel: $("fx").open,
          errors: $("error").textContent,
        })),
      );
      throw e;
    }
    await fx.locator(".fx-chain-delete").first().click();
    await page.waitForFunction(() => cfg.channels[0].inserts.length === 6);
    await fx.locator("#fx-title").click();
    await fx.keyboard.press("Delete");
    await page.waitForFunction(() => cfg.channels[0].inserts.length === 5);
    assert.equal(await page.locator("#mixer .strip").count(), 8);
    await fx.close();
    console.log(
      "Eight inserts and one-press Delete verified; checking precise context actions",
    );
    await page
      .locator('#mixer .strip[data-strip="c0"] .insert-slot')
      .nth(3)
      .click({ button: "right" });
    assert.equal(
      await page
        .getByRole("menuitem", { name: "Eliminar canal", exact: true })
        .count(),
      0,
    );
    await page.getByRole("menuitem", { name: /^Eliminar FX:/ }).click();
    await page.waitForFunction(
      () => cfg.channels[0].inserts.length === 4 && cfg.channels.length === 8,
    );
    await page
      .locator('#mixer .strip[data-strip="c0"] .eq-module')
      .click({ button: "right" });
    assert.equal(
      await page.getByRole("menuitem", { name: /Eliminar canal/ }).count(),
      0,
    );
    await page.keyboard.press("Escape");
    await page.evaluate(async () => {
      await api("start", { mode: "demo" });
    });
    await page.waitForFunction(
      () => running && lastStatus.gain_reduction_db[0] > 0.1,
    );
    const comp = page.locator('#mixer .strip[data-strip="c0"] .comp-module');
    assert.equal(await comp.getAttribute("aria-pressed"), "true");
    assert(await comp.locator("svg .comp-response").isVisible());
    assert((await comp.getAttribute("class")).includes("modified"));
    await comp.click({ button: "right" });
    await page
      .getByRole("menuitem", { name: "Bypass del compresor", exact: true })
      .click();
    await page.waitForFunction(() =>
      $("mixer").querySelector(".comp-module").classList.contains("off"),
    );
    await comp.click({ button: "right" });
    await page
      .getByRole("menuitem", { name: "Activar compresor", exact: true })
      .click();
    await page.waitForFunction(() => lastStatus.gain_reduction_db[0] > 0.1);
    await fs.mkdir("docs/screenshots/0.14.0", { recursive: true });
    await page.screenshot({
      path: "docs/screenshots/0.14.0/01-consola-compresor.png",
    });
    const recEvent = page.waitForEvent("popup");
    await page.evaluate(() => openRecording());
    let rec = await recEvent;
    await rec.waitForSelector("#recording-tracks tr");
    await rec.setViewportSize({ width: 760, height: 690 });
    await rec.locator("#recording-none").click();
    await rec
      .locator('[data-record-kind="channel"][data-record-index="0"]')
      .check();
    await rec.locator('[data-record-point="0"]').selectOption("pre");
    await rec
      .locator('[data-record-kind="channel"][data-record-index="1"]')
      .check();
    await rec.locator('[data-record-point="1"]').selectOption("post");
    await rec
      .locator('[data-record-kind="bus"][data-record-index="0"]')
      .check();
    await rec.locator("#recording-directory").fill(data + "/takes");
    await rec.locator("#recording-start").click();
    await page.waitForFunction(() => lastStatus.recording?.active);
    assert(await page.locator("#recording-indicator").isVisible());
    const state = await page.evaluate(() => lastStatus.recording);
    assert.equal(state.tracks.length, 3);
    assert.equal(state.tracks[0].point, "pre");
    assert.equal(state.tracks[1].point, "post");
    assert.equal(state.tracks[2].point, "post");
    await rec.waitForFunction(() =>
      $("recording-status").textContent.includes("REC"),
    );
    assert(
      await rec.locator('[data-record-kind="channel"]').first().isDisabled(),
    );
    await rec.waitForFunction(
      () => lastStatus.recording?.written_frames >= 4800,
    );
    await rec.locator(".recording-track-scroll").evaluate((e) => {
      e.scrollTop = 0;
    });
    await rec.screenshot({
      path: "docs/screenshots/0.14.0/02-grabacion-multipista.png",
    });
    await page.evaluate(async () => {
      const next = structuredClone(cfg);
      next.channels[1].format = "stereo";
      let failed = false;
      try {
        await api("config", next);
      } catch (e) {
        failed = e.message.includes("grabación");
      }
      if (!failed) throw Error("Recording topology change should be rejected");
      $("error").textContent = "";
    });
    await page.evaluate(async () => {
      cfg.channels[0].inserts.reverse();
      await publish();
    });
    await page.waitForFunction(() => lastStatus.recording.written_frames > 0);
    const priorFrames = await page.evaluate(
      () => lastStatus.recording.written_frames,
    );
    await rec.close();
    await page.waitForFunction(
      (n) =>
        lastStatus.recording.active && lastStatus.recording.written_frames > n,
      priorFrames,
    );
    const reopen = page.waitForEvent("popup");
    await page.locator("#recording-indicator").click();
    rec = await reopen;
    await rec.waitForSelector("#recording-stop:not(:disabled)");
    await rec.locator("#recording-stop").click();
    await rec.waitForFunction(
      () => lastStatus.recording?.state === "completed",
    );
    const report = await rec.evaluate(() => lastStatus.recording);
    assert.equal(report.dropped_frames, 0);
    assert.equal(report.track_frames.length, 3);
    assert(
      report.track_frames.every((n) => n === report.track_frames[0] && n > 0),
    );
    const manifest = JSON.parse(
      await fs.readFile(report.directory + "/session.json", "utf8"),
    );
    assert.equal(manifest.state, "completed");
    assert.equal(
      (await fs.readdir(report.directory)).filter((n) => n.endsWith(".wav"))
        .length,
      3,
    );
    assert.equal(await page.evaluate(() => lastStatus.mode), "DEMO");
    await rec.close();
    const recoveryEvent = page.waitForEvent("popup");
    await page
      .locator('#mixer .strip[data-strip="c0"] .insert-slot')
      .first()
      .click();
    const recovery = await recoveryEvent;
    await recovery.waitForSelector("#params .rotary-control");
    await recovery.route("**/api/config", (route) => route.abort());
    await recovery.locator("#removefx").click();
    await recovery.waitForFunction(() => $("fxerror").textContent.length > 0);
    assert(
      await recovery.evaluate(
        () => !!pendingFx && cfg.channels[0].inserts.length === 4,
      ),
    );
    await recovery.unroute("**/api/config");
    await recovery.locator("#params [role=slider]").first().press("ArrowUp");
    await recovery.waitForFunction(() =>
      $("fx-edit-status").textContent.includes("Aplicado"),
    );
    await recovery.close();
    const prefsEvent = page.waitForEvent("popup");
    await page.click("#network-settings");
    const prefs = await prefsEvent;
    await prefs.waitForSelector("#network-dialog[open]");
    await prefs.setViewportSize({ width: 576, height: 800 });
    await prefs.screenshot({
      path: "docs/screenshots/0.14.0/03-preferencias-compactas.png",
    });
    await prefs.close();
    const calls = await page.evaluate(() => ({
      calls: popupCalls,
      width: screen.availWidth,
      height: screen.availHeight,
      sizes: [
        panelWindowSize("fx", 1920, 1080),
        panelWindowSize("network-dialog", 1920, 1080),
      ],
    }));
    assert.deepEqual(calls.sizes[0], [800, 580]);
    assert(calls.sizes[1][0] <= 576 && calls.sizes[1][1] <= 864);
    for (const call of calls.calls) {
      const width = Number(call.features.match(/width=(\d+)/)[1]),
        height = Number(call.features.match(/height=(\d+)/)[1]);
      assert(width <= calls.width * 0.45 + 1);
      assert(height <= calls.height * 0.8 + 1);
    }
    await page.evaluate(async () => {
      cfg.channels[1].inserts = Array.from({ length: 8 }, (_, i) => ({
        id: "native/mda/dither",
        path: "",
        instance: "touch-" + i,
        name: "MDA Dither",
        params: {},
        bypass: false,
      }));
      await publish();
      render();
      document.body.classList.add("touch-mode");
      document.querySelector(".app").classList.remove("touch-compact");
    });
    const touchRack = page.locator(
        '#mixer .strip[data-strip="c1"] .insert-rack',
      ),
      box = await touchRack.boundingBox(),
      pagesBefore = context.pages().length;
    const cdp = await context.newCDPSession(page);
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{ x: box.x + box.width / 2, y: box.y + 115, id: 1 }],
    });
    for (let i = 1; i <= 6; i++)
      await cdp.send("Input.dispatchTouchEvent", {
        type: "touchMove",
        touchPoints: [
          { x: box.x + box.width / 2, y: box.y + 115 - i * 14, id: 1 },
        ],
      });
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [],
    });
    assert(await touchRack.evaluate((e) => e.scrollTop > 0));
    assert.equal(context.pages().length, pagesBefore);
    assert.equal(
      await page.evaluate(() => document.scrollingElement.scrollTop),
      0,
    );
    await page.evaluate(() => api("stop"));
    assert.deepEqual(errors, []);
    console.log(
      "PASS: eight slots, Delete focus, explicit per-insert remove, context isolation, live compressor curve/GR, selected PRE/POST recording and post master, aligned WAVs and responsive popup dimensions",
    );
  } finally {
    await browser?.close();
    app.kill("SIGTERM");
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
