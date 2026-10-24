// Configure a real named auxiliary sender through Preferences and receive its UDP audio.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const dgram = require("node:dgram");
const assert = require("node:assert/strict");
(async () => {
  const app = spawn("desktop/target/debug/aumux", ["--no-browser"], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  app.stderr.on("data", () => {});
  const socket = dgram.createSocket("udp4");
  await new Promise((resolve) => socket.bind(0, "127.0.0.1", resolve));
  let packets = 0,
    signal = false;
  socket.on("message", (data) => {
    assert.equal(data.subarray(0, 4).toString(), "AUM1");
    assert(data.length < 1200);
    assert.equal(data.subarray(40, 72).toString().split("\0")[0], "IEM-VOCES");
    for (let i = 80; i < data.length; i += 4)
      if (Math.abs(data.readFloatLE(i)) > 0.001) signal = true;
    packets++;
  });
  let browser;
  try {
    const origin = await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(Error("startup timeout")), 5000);
      let text = "";
      app.stdout.on("data", (b) => {
        text += b;
        const m = text.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) {
          clearTimeout(timeout);
          resolve(m[0]);
        }
      });
    });
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });
    require("./floating-panels.cjs")(browser);
    const page = await browser.newPage({
        viewport: { width: 1440, height: 980 },
      }),
      errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(origin, { waitUntil: "domcontentloaded" });
    await page.waitForSelector("#mixer .strip");
    await page.evaluate(async () => {
      cfg.buses[1].name = "IEM voces";
      cfg.buses[1].gain_db = 0;
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -18,
        frequency: 1000,
      };
      cfg.channels[0].sends_db[0] = 0;
      await publish();
      await api("start", { mode: "demo" });
    });
    await page.locator("#network-settings").click();
    await page.locator("#pref-tab-audio-network").click();
    await page.locator("#audio-network-add").click();
    const card = page.locator(".audio-network-route").first();
    await card.waitFor();
    await card.getByLabel("Transmitir", { exact: true }).check();
    await card.getByLabel("Identificador", { exact: false }).fill("IEM-VOCES");
    await card.getByLabel("Mezcla de origen").selectOption("1");
    await card
      .getByLabel("Puerto UDP del receptor")
      .fill(String(socket.address().port));
    await card.getByLabel("Destinos IP", { exact: false }).fill("127.0.0.1");
    await card
      .getByRole("button", { name: "Aplicar flujo", exact: true })
      .click();
    await page.waitForFunction(
      () =>
        cfg.audio_network[0].enabled && lastStatus.audio_network?.[0]?.sent > 2,
    );
    assert(
      packets > 2 && signal,
      "real auxiliary PCM must arrive after applying the form",
    );
    assert(
      (await card.getByLabel("Mezcla de origen").innerText()).includes(
        "IEM voces",
      ),
    );
    await page.screenshot({ path: "docs/aumux-0.13-network-audio.png" });
    await page.locator("#audio-network-add").click();
    const other = page.locator(".audio-network-route").nth(1);
    await other.getByLabel("Transmitir", { exact: true }).check();
    await other.getByLabel("Identificador", { exact: false }).fill("IEM-VOCES");
    await other
      .getByRole("button", { name: "Aplicar flujo", exact: true })
      .click();
    await page.waitForFunction(() =>
      document
        .querySelector("#audio-network-message")
        .textContent.includes("único"),
    );
    const backend = await page.evaluate(
      async () => await (await fetch("/api/config")).json(),
    );
    assert.equal(
      backend.audio_network[1].enabled,
      false,
      "invalid duplicate must not change backend",
    );
    await other
      .getByRole("button", { name: "Quitar flujo", exact: true })
      .click();
    await page.waitForFunction(() => cfg.audio_network.length === 1);
    await page.locator('[data-close="network-dialog"]').click();
    const saved = page.waitForEvent("download");
    await page.locator("#save").click();
    const fs = require("node:fs/promises");
    const scene = JSON.parse(
      await fs.readFile(await (await saved).path(), "utf8"),
    );
    assert.equal(scene.audio_network[0].stream, "IEM-VOCES");
    assert.equal(scene.audio_network[0].bus, 1);
    assert.deepEqual(errors, []);
    console.log(
      "PASS: actual Preferences route creation/apply, named auxiliary PCM, live names, duplicate rejection without backend mutation, route removal and scene persistence",
    );
  } finally {
    await browser?.close();
    socket.close();
    app.kill();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
