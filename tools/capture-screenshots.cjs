// Capture the running application in DEMO mode; no audio hardware is opened.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const assert = require("node:assert/strict");

(async () => {
  const root = path.resolve(__dirname, "..");
  const manifest = await fs.readFile(
    path.join(root, "desktop/Cargo.toml"),
    "utf8",
  );
  const version = manifest.match(/^version = "([^"]+)"/m)[1];
  const output = path.join(root, "docs/screenshots", version);
  const data = await fs.mkdtemp(path.join(os.tmpdir(), "aumux-screenshots-"));
  await fs.mkdir(output, { recursive: true });
  const app = spawn(
    process.env.AUMUX_BINARY || path.join(root, "desktop/target/debug/aumux"),
    ["--no-browser"],
    {
      cwd: root,
      env: {
        ...process.env,
        MONITOR_MIX_DATA_DIR: data,
        MONITOR_REMOTE_BIND: "127.0.0.1",
        MONITOR_LISTEN_HOST: "127.0.0.1",
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  app.stderr.on("data", () => {});
  let browser;
  const errors = [];
  try {
    const origin = await new Promise((resolve, reject) => {
      const timeout = setTimeout(
        () => reject(Error("AuMux startup timeout")),
        10000,
      );
      let text = "";
      app.stdout.on("data", (bytes) => {
        text += bytes;
        const match = text.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (match) {
          clearTimeout(timeout);
          resolve(match[0]);
        }
      });
      app.once("error", (error) => {
        clearTimeout(timeout);
        reject(error);
      });
      app.once("exit", (code) => {
        clearTimeout(timeout);
        reject(Error(`AuMux exited: ${code}`));
      });
    });
    browser = await chromium.launch({
      executablePath: process.env.CHROMIUM || "/usr/bin/chromium",
      headless: true,
      args: [
        "--no-sandbox",
        "--no-proxy-server",
        "--disable-dev-shm-usage",
        "--autoplay-policy=no-user-gesture-required",
      ],
    });
    const page = await browser.newPage({
      viewport: { width: 1640, height: 980 },
    });
    page.on("pageerror", (error) => errors.push(error.message));
    const shot = async (target, name, options = {}) => {
      await target.evaluate(() => document.fonts.ready);
      await target.screenshot({
        path: path.join(output, name + ".png"),
        ...options,
      });
      console.log(`Captured ${name}.png`);
    };
    await page.goto(origin, { waitUntil: "domcontentloaded" });
    await page.waitForSelector("#mixer .strip");
    assert.equal((await page.locator(".version").innerText()).trim(), version);
    await page.evaluate(async () => {
      const names = [
        "Voz principal",
        "Voz invitada",
        "Guitarra",
        "Teclado L/R",
        "Bajo",
        "Percusión",
        "Ambiente",
        "Retorno PC",
      ];
      cfg.channels.forEach((ch, index) => {
        ch.name = names[index];
        ch.gain_db = [-6, -9, -12, -9, -12, -15, -24, -18][index];
        ch.pan = [0, 0, -0.3, 0, 0, 0.2, 0, 0][index];
        ch.generator = {
          signal: index % 2 ? "pink" : "sine",
          level_db: -24,
          frequency: 220 + index * 110,
        };
        ch.sends_db = [-9 - index, -15 + index, -18, -24];
      });
      cfg.channels[3].format = "stereo";
      cfg.buses.forEach((bus, index) => {
        bus.name = [
          "Master / OBS",
          "IEM voces",
          "IEM banda",
          "Control Room",
          "Reserva",
        ][index];
        bus.gain_db = index ? -6 : -3;
      });
      const eq = cfg.channels[0].eq;
      eq.bands[0].kind = 3;
      eq.bands[0].frequency = 80;
      eq.bands[2].gain_db = -4;
      eq.bands[2].q = 1.3;
      eq.bands[5].gain_db = 3;
      eq.bands[5].q = 0.8;
      cfg.channels[0].compressor.bypass = false;
      cfg.channels[0].compressor.threshold = -34;
      cfg.channels[0].compressor.source = { type: "channel", index: 1 };
      await publish();
      render();
    });
    // Add a real native insert through the editor before starting the graph.
    await page.evaluate(() => openFx("channel", 0, 0));
    await page
      .locator('#builtin-fx option[value="native/mda/dynamics"]')
      .waitFor({ state: "attached" });
    await page.locator("#builtin-fx").selectOption("native/mda/dynamics");
    await page.locator(".generic-native-panel").waitFor();
    await page.locator("#applyfx").click();
    await page.locator("#closefx").click();
    await page.evaluate(async () => {
      await api("start", { mode: "demo" });
      lastStatus.remote = await api("remote", { action: "start", port: 0 });
      await api("remote", { action: "access", scope: "console", mode: "open" });
      await api("remote", { action: "access", scope: "mix", mode: "open" });
      await api("network-host", { host: "127.0.0.1" });
    });
    await page.waitForFunction(
      () => lastStatus?.mode === "DEMO" && lastStatus?.system?.ram_bytes > 0,
    );
    await page.waitForFunction(() =>
      /CPU: \d/.test(document.querySelector("#system-cpu").textContent),
    );
    await shot(page, "01-consola");
    await page.evaluate(() => openEq("channel", 0));
    await page.getByRole("tab", { name: "Banda EQ 3", exact: true }).click();
    await shot(page, "02-eq");
    await page.locator('[data-close="eq-dialog"]').click();
    await page.evaluate(() => openCompressor("channel", 0));
    await page.waitForFunction(
      () =>
        parseFloat(document.querySelector("#comp-gr-value").textContent) > 0,
    );
    await shot(page, "03-compresor");
    await page.locator('[data-close="compressor-dialog"]').click();
    await page.evaluate(() => openFx("channel", 0, 0));
    await page.locator(".generic-native-panel").waitFor();
    await shot(page, "04-efectos");
    await page.locator("#closefx").click();
    await page.evaluate(() => openRouting(0));
    await shot(page, "05-envios");
    await page.locator('[data-close="routing-dialog"]').click();
    await page.locator("#network-settings").click();
    await page
      .locator("#network-interface option")
      .nth(1)
      .waitFor({ state: "attached" });
    await page.locator("#network-interface").selectOption("127.0.0.1");
    await shot(page, "06-preferencias-redes");
    await page.locator("#pref-tab-mix").click();
    await page.waitForFunction(() =>
      document.querySelector("#mix-generic-url").value.startsWith("http"),
    );
    const mixUrl = await page.locator("#mix-generic-url").inputValue();
    await shot(page, "07-preferencias-aux");
    await page.locator('[data-close="network-dialog"]').click();
    const consoleUrl = await page.evaluate(() => lastStatus.remote.console_url);
    const tablet = await browser.newContext({
      viewport: { width: 1100, height: 820 },
      hasTouch: true,
    });
    const touch = await tablet.newPage();
    touch.on("pageerror", (error) => errors.push(error.message));
    await touch.goto(consoleUrl, { waitUntil: "domcontentloaded" });
    await touch.waitForSelector("#mixer .strip");
    await touch.locator("#tools-menu > summary").click();
    await touch.locator("#touch-mode").selectOption("on");
    await touch.locator("#tools-menu > summary").click();
    await shot(touch, "08-consola-remota-touch");
    const mobile = await browser.newContext({
      viewport: { width: 390, height: 844 },
      hasTouch: true,
      isMobile: true,
    });
    const aux = await mobile.newPage();
    aux.on("pageerror", (error) => errors.push(error.message));
    await aux.goto(mixUrl, { waitUntil: "domcontentloaded" });
    await aux.waitForFunction(() => connected && state?.shared);
    await aux.locator("#mix-select").selectOption("1");
    await aux.waitForFunction(
      () => state.bus.index === 1 && state.channels[0].name === "Voz principal",
    );
    await aux.waitForFunction(() =>
      /control RTT \d/.test(
        document.querySelector("#remote-latency-summary").textContent,
      ),
    );
    await shot(aux, "09-mezcla-aux-movil");
    const listenUrl = await page.evaluate(async () => {
      await api("listen-access", { mode: "open" });
      return (await api("listen", { enabled: true, port: 0 })).url;
    });
    const listen = await mobile.newPage();
    listen.on("pageerror", (error) => errors.push(error.message));
    await listen.goto(listenUrl, { waitUntil: "domcontentloaded" });
    await listen.waitForFunction(
      () => document.querySelector("#bus").options.length === 5,
    );
    await listen.locator("#bus").selectOption("1");
    await listen.locator("#play").click();
    await listen.waitForFunction(
      () =>
        sources.size > 0 &&
        document.querySelector("#state").textContent.startsWith("Escuchando"),
    );
    await listen.locator("details summary").click();
    await shot(listen, "10-escucha-movil", { fullPage: true });
    await listen.locator("#stop").click();
    await page.evaluate(() => openLatency());
    await shot(page, "11-latencia");
    assert.deepEqual(errors, [], "screenshots must show a functioning UI");
    console.log(
      `PASS: 11 current AuMux ${version} screens captured from the running app, DEMO only.`,
    );
  } finally {
    if (browser) await browser.close();
    const stopped = new Promise((resolve) => app.once("exit", resolve));
    if (app.exitCode === null && !app.signalCode) {
      app.kill();
      await stopped;
    }
    await fs.rm(data, { recursive: true, force: true });
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
