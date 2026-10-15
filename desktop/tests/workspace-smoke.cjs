// Real-browser checks for adaptive strips, compressor, context menu and LAN playback.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
(async () => {
  const app = spawn(
    process.env.MONITOR_BINARY || "desktop/target/debug/monitor-mix",
    ["--no-browser"],
    {
      env: {
        ...process.env,
        MONITOR_LISTEN_HOST: "127.0.0.1",
        MONITOR_MIX_DATA_DIR: await fs.mkdtemp(
          "/workspace/scratch/mix07-presets-",
        ),
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
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
      args: [
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--autoplay-policy=no-user-gesture-required",
      ],
    });
    const page = await browser.newPage({
        viewport: { width: 1640, height: 980 },
      }),
      errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(url);
    await page.waitForSelector("#mixer .strip");
    const setSize = async (ch, aux) => {
      await page.evaluate(
        async ({ ch, aux }) => {
          await resizeStructure(ch, aux, "mono", "mono");
        },
        { ch, aux },
      );
    };
    await setSize(2, 1);
    assert(
      await page.evaluate(() => {
        const s = document
            .querySelector("#mixer-stage")
            .getBoundingClientRect(),
          a = document
            .querySelector(".strip[data-strip=c0]")
            .getBoundingClientRect(),
          b = document
            .querySelector(".strip[data-strip=b0]")
            .getBoundingClientRect();
        return Math.abs((a.left + b.right) / 2 - (s.left + s.right) / 2) < 2;
      }),
      "few strips centered",
    );
    assert.equal(
      await page
        .locator("body")
        .evaluate((e) => getComputedStyle(e).userSelect),
      "none",
    );
    assert.equal(
      await page
        .locator(".strip .name")
        .first()
        .evaluate((e) => getComputedStyle(e).userSelect),
      "text",
    );
    await page
      .locator(".strip[data-strip=c0] .comp-module")
      .click({ button: "right" });
    await page
      .getByRole("menuitem", { name: "Compresor / sidechain", exact: true })
      .click();
    await page.waitForSelector("#compressor-dialog[open]");
    assert.equal(await page.locator("#comp-knobs [role=slider]").count(), 8);
    await page.locator("#comp-source").selectOption("channel:1");
    await page.locator("#comp-bypass").uncheck();
    await page.locator('[aria-label="Compresor threshold"]').fill("-40");
    await page
      .locator('[aria-label="Compresor threshold"]')
      .dispatchEvent("change");
    await page.waitForTimeout(250);
    await page.evaluate(async () => {
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -20,
        frequency: 440,
      };
      cfg.channels[1].generator = {
        signal: "sine",
        level_db: -6,
        frequency: 1000,
      };
      cfg.channels[1].mute = true;
      await publish();
      await api("start", { mode: "demo" });
    });
    await page.waitForFunction(
      () =>
        parseFloat(document.querySelector("#comp-gr-value").textContent) > 20,
    );
    await page.screenshot({ path: "/workspace/scratch/monitor-mix-workspace-compressor.png" });
    await page.locator("[data-close=compressor-dialog]").click();
    await page.evaluate(async () => {
      await api("stop", {});
    });
    await page.waitForFunction(() => !running);
    await setSize(24, 4);
    await page.evaluate(async () => {
      await pinStrip("channel", 0, "left");
      await pinStrip("bus", 1, "right");
    });
    const positions = () =>
      page.evaluate(() => {
        const r = (s) => {
          const e = document.querySelector(s).getBoundingClientRect();
          return { x: e.x, right: e.right };
        };
        return {
          left: r(".strip[data-strip=c0]"),
          aux: r(".strip[data-strip=b1]"),
          master: r(".strip[data-strip=b0]"),
          stage: r("#mixer-stage"),
          scroll: document.querySelector("#mixer-stage").scrollLeft,
        };
      });
    const before = await positions();
    await page.evaluate(() => {
      document.querySelector("#mixer-stage").scrollLeft = 1000;
    });
    await page.waitForTimeout(100);
    const after = await positions();
    assert(after.scroll > 500);
    assert.equal(after.master.right, before.master.right);
    assert.equal(after.aux.right, before.aux.right);
    assert.equal(after.left.x, before.left.x);
    assert.equal(after.master.right, after.stage.right);
    assert.equal(after.left.x, after.stage.x);
    assert(
      await page.evaluate(
        () =>
          Math.abs(
            document.querySelector("#mixer-stage").scrollLeft -
              document.querySelector("#meter-stage").scrollLeft,
          ) < 2,
      ),
    );
    await page.screenshot({ path: "/workspace/scratch/monitor-mix-workspace.png" });
    await page
      .locator(".strip[data-strip=c0] .comp-module")
      .click({ button: "right" });
    await page.getByRole("menuitem", { name: "Desfijar", exact: true }).click();
    await page.waitForFunction(() => cfg.channels[0].pin === "none");
    await page.locator("#network-settings").click();
    await page.evaluate(async () => {
      const n = await api("listen", { enabled: true, port: 0 });
      lastStatus.listen = n;
      updateWorkspaceStatus(lastStatus);
    });
    await page.waitForFunction(() =>
      document.querySelector("#network-url").value.includes("key="),
    );
    const link = await page.locator("#network-url").inputValue();
    await page.locator("[data-close=network-dialog]").click();
    await page.evaluate(async () => {
      await api("start", { mode: "demo" });
    });
    const listener = await browser.newPage({
      viewport: { width: 700, height: 640 },
    });
    listener.on("pageerror", (e) => errors.push(e.message));
    await listener.goto(link);
    await listener.waitForFunction(
      () => document.querySelector("#bus").options.length === 5,
    );
    await listener.locator("#play").click();
    await listener.waitForFunction(() =>
      document.querySelector("#state").textContent.startsWith("Escuchando"),
    );
    await listener.waitForFunction(
      () => sources.size > 0 && context.sampleRate === 48000,
    );
    await listener.locator("#bus").selectOption("1");
    await listener.waitForFunction(
      () =>
        document.querySelector("#state").textContent.includes("Monitor 1") &&
        sources.size > 0,
    );
    await listener.screenshot({ path: "/workspace/scratch/monitor-mix-workspace-listen.png" });
    // Switch while fetch/resume is pending, then stop: old sessions cannot revive playback.
    await listener.evaluate(() => {
      play();
      bus.value = "0";
      play();
      stop();
    });
    await listener.waitForTimeout(200);
    assert.equal(await listener.locator("#state").innerText(), "Detenido");
    await page.evaluate(async () => {
      await api("listen", { enabled: false });
      await api("stop", {});
    });
    assert.deepEqual(errors, []);
    console.log(
      "PASS: centered/overflow mixer, master + independent left/right pins, synchronized meters, custom context menus, selection rules, live compressor with external key, LAN browser playback and rapid source switching",
    );
  } finally {
    await browser?.close();
    app.kill();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
