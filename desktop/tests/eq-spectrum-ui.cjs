// Real engine spectra, calibrated levels, three views, leases, and remote access.
const { chromium } = require("playwright-core"),
  { spawn } = require("node:child_process"),
  fs = require("node:fs/promises"),
  assert = require("node:assert/strict");
(async () => {
  const data = await fs.mkdtemp("/workspace/scratch/aumux-spectrum-ui-");
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
      const timer = setTimeout(() => reject(Error("startup")), 10000);
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
      viewport: { width: 1600, height: 1000 },
    });
    context.setDefaultTimeout(15000);
    const page = await context.newPage(),
      errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(url);
    await page.waitForSelector("#mixer .strip");
    await page.evaluate(async () => {
      panelMode = "floating";
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -20,
        frequency: 750,
      };
      cfg.channels[0].gain_db = -24;
      cfg.channels[0].mute = true;
      cfg.channels[0].eq.bypass = false;
      Object.assign(cfg.channels[0].eq.bands[0], {
        kind: 0,
        frequency: 750,
        gain_db: -12,
        q: 1,
      });
      cfg.channels[0].compressor = {
        ...defaultCompressor(),
        bypass: false,
        threshold: -50,
        ratio: 20,
      };
      cfg.channels[0].inserts = [
        {
          id: "native/mda/overdrive",
          path: "",
          instance: "spectrum-fx",
          params: {},
          name: "MDA Overdrive",
          bypass: false,
        },
      ];
      await publish();
      await api("start", { mode: "demo" });
      openEq("channel", 0);
    });
    await page.waitForFunction(
      () =>
        eqSpectrum.post.length === 256 &&
        Math.abs(Math.max(...eqSpectrum.post) + 32) < 0.3,
    );
    assert.equal(await page.evaluate(() => eqSpectrumSettings.mode), "compact");
    assert.equal(await page.locator("#eq-spectrum-pre").getAttribute("d"), "");
    assert(
      (await page.locator("#eq-spectrum-post").getAttribute("d")).endsWith("Z"),
    );
    await fs.mkdir("docs/screenshots/0.14.1", { recursive: true });
    await page
      .locator("#eq-dialog")
      .screenshot({ path: "docs/screenshots/0.14.1/03-espectro-compacto.png" });
    async function menu(pattern) {
      await page
        .locator("#eq-graph svg")
        .click({ button: "right", position: { x: 90, y: 60 } });
      await page.getByRole("menuitem", { name: pattern }).click();
    }
    await menu(/B · Comparar/);
    await page.waitForFunction(
      () =>
        eqSpectrum.pre.length === 256 &&
        Math.abs(Math.max(...eqSpectrum.pre) + 20) < 0.3,
    );
    assert(
      Math.abs(
        (await page.evaluate(
          () => Math.max(...eqSpectrum.pre) - Math.max(...eqSpectrum.post),
        )) - 12,
      ) < 0.3,
    );
    // PRE/POST are adjacent to the EQ, independent of mute/fader/compressor/inserts.
    assert(
      (await page.locator("#eq-spectrum-pre").getAttribute("d")).length > 100,
    );
    await page.locator("#eq-dialog").screenshot({
      path: "docs/screenshots/0.14.1/04-espectro-comparacion.png",
    });
    const beforeRecording = await page.evaluate(() => eqSpectrum.sequence);
    await page.evaluate(async (directory) => {
      await api("recording", {
        action: "start",
        directory,
        channels: cfg.channels.map((_, i) => ({
          enabled: i === 0,
          point: "pre",
        })),
        buses: [],
      });
    }, data + "/takes");
    await page.waitForFunction(
      (before) => eqSpectrum.sequence > before + 20,
      beforeRecording,
    );
    const recording = await page.evaluate(
      async () => await api("recording", { action: "stop" }),
    );
    assert(recording.written_frames > 0 && !recording.active);
    await menu(/C · Espectro/);
    await page.waitForFunction(() => {
      const pixels = eqSpectrum.canvas
        .getContext("2d")
        .getImageData(0, 0, 256, 100).data;
      return pixels.some((v, i) => i % 4 === 3 && v > 0);
    });
    await page.locator("#eq-dialog").screenshot({
      path: "docs/screenshots/0.14.1/05-espectro-historial.png",
    });
    await menu("Congelar imagen");
    const frozen = await page.evaluate(() => eqSpectrum.sequence);
    await page.evaluate(async () => {
      cfg.channels[0].eq.bands[0].gain_db = -6;
      await publish();
    });
    await page.waitForTimeout(350);
    assert.equal(await page.evaluate(() => eqSpectrum.sequence), frozen);
    await menu("Reanudar imagen");
    await page.waitForFunction(
      () => Math.abs(Math.max(...eqSpectrum.post) + 26) < 0.3,
    );
    await menu("Activar retención de picos");
    await page.waitForFunction(
      () =>
        document.getElementById("eq-spectrum-peaks").getAttribute("d").length >
        100,
    );
    await page.evaluate(async () => {
      cfg.channels[0].eq.bands[0].gain_db = -18;
      await publish();
    });
    await page.waitForFunction(() => Math.max(...eqSpectrum.post) < -37.5);
    assert(await page.evaluate(() => Math.max(...eqSpectrum.peak) > -26.5));
    await menu("Apagar analizador");
    assert.equal(await page.locator("#eq-spectrum-post").getAttribute("d"), "");
    await page.reload();
    await page.waitForSelector("#mixer .strip");
    await page.evaluate(() => {
      panelMode = "floating";
      openEq("channel", 0);
    });
    assert.deepEqual(
      await page.evaluate(() => [
        eqSpectrumSettings.mode,
        eqSpectrumSettings.enabled,
      ]),
      ["history", false],
    );
    await menu("Encender analizador");
    await page.waitForFunction(() => eqSpectrum.post.length === 256);
    // Compact mode and actual controls remain usable on a narrow console.
    await page.setViewportSize({ width: 390, height: 780 });
    await page.locator("#eq-dialog .panel-compact").click();
    const bounds = await page.locator("#eq-dialog").boundingBox();
    assert(
      bounds.width <= 374 && bounds.x >= 7 && bounds.x + bounds.width <= 383,
    );
    await page.setViewportSize({ width: 1600, height: 1000 });
    await page.locator("#eq-dialog .panel-compact").click();
    // Bus EQ uses its own tap, not an arbitrary channel or the postfader output.
    await page.evaluate(async () => {
      cfg.channels[0].mute = false;
      cfg.channels[0].gain_db = 0;
      cfg.channels[0].inserts = [];
      cfg.channels[0].compressor.bypass = true;
      cfg.channels[0].eq.bands[0].gain_db = 0;
      cfg.buses[0].gain_db = -30;
      cfg.buses[0].eq.bypass = false;
      Object.assign(cfg.buses[0].eq.bands[0], {
        kind: 0,
        frequency: 750,
        gain_db: 6,
        q: 1,
      });
      await publish();
      openEq("bus", 0);
      saveSpectrumSettings({ mode: "compare" });
    });
    await page.waitForFunction(
      () =>
        eqSpectrum.pre.length === 256 &&
        Math.abs(
          Math.max(...eqSpectrum.post) - Math.max(...eqSpectrum.pre) - 6,
        ) < 0.3,
    );
    const remoteUrl = await page.evaluate(async () => {
      await api("network-host", { host: "127.0.0.1" });
      const r = await api("remote", { action: "start", port: 0 });
      await api("remote", { action: "access", scope: "console", mode: "open" });
      return r.console_url;
    });
    const remote = await context.newPage();
    remote.on("pageerror", (e) => errors.push(e.message));
    await remote.goto(remoteUrl);
    await remote.waitForSelector("#mixer .strip");
    await remote.evaluate(() => {
      panelMode = "floating";
      openEq("channel", 0);
    });
    await remote.waitForFunction(
      () =>
        eqSpectrum.post.length === 256 &&
        Math.abs(Math.max(...eqSpectrum.post) + 20) < 0.3,
    );
    await remote.locator('[data-close="eq-dialog"]').click();
    await remote.close();
    await page.evaluate(async () => {
      await api("stop");
    });
    await page.waitForFunction(() =>
      document
        .querySelector(".eq-spectrum-bar span")
        .textContent.includes("Motor detenido"),
    );
    assert.equal(await page.locator("#eq-spectrum-post").getAttribute("d"), "");
    assert.deepEqual(errors, []);
    console.log(
      "PASS: actual calibrated EQ PRE/POST spectra, default A, right-click B/C, real waterfall, freeze/peaks/off persistence, mono/bus taps, mobile bounds and remote console",
    );
  } finally {
    await browser?.close();
    app.kill("SIGTERM");
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
