// Real Chrome touch gestures, unified shared access, main-console control and latency tools.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const assert = require("node:assert/strict");
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
    const origin = await new Promise((resolve, reject) => {
      let text = "";
      const t = setTimeout(() => reject(Error("startup timeout")), 5000);
      app.stdout.on("data", (b) => {
        text += b;
        const m = text.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) {
          clearTimeout(t);
          resolve(m[0]);
        }
      });
    });
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: [
        "--no-sandbox",
        "--no-proxy-server",
        "--disable-dev-shm-usage",
        "--autoplay-policy=no-user-gesture-required",
      ],
    });
    const errors = [],
      operator = await browser.newPage({
        viewport: { width: 1600, height: 980 },
      });
    operator.on("pageerror", (e) => errors.push(e.message));
    await fetch(origin, { signal: AbortSignal.timeout(5000) });
    await operator.goto(origin, { waitUntil: "domcontentloaded" });
    await operator.waitForSelector("#mixer .strip");
    await operator.evaluate(async () => {
      cfg.channels[0].name = "Voz principal";
      cfg.channels[0].gain_db = -24;
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -20,
        frequency: 1000,
      };
      cfg.buses[1].name = "IEM cantante";
      cfg.buses[2].name = "IEM baterista";
      await publish();
      await api("start", { mode: "demo" });
      lastStatus.remote = await api("remote", { action: "start", port: 0 });
      await api("network-host", { host: "127.0.0.1" });
    });
    await operator.locator("#network-settings").click();
    await operator.waitForFunction(
      () => document.querySelector("#network-interface").options.length > 1,
    );
    await operator.locator("#network-interface").selectOption("127.0.0.1");
    assert.equal(
      await operator.locator("#network-host").inputValue(),
      "127.0.0.1",
    );
    await operator.locator("#mix-access-mode").selectOption("open");
    await operator.locator("#mix-access-save").click();
    await operator.waitForFunction(() =>
      document
        .querySelector("#access-status")
        .textContent.includes("actualizado"),
    );
    await operator.waitForFunction(() =>
      document.querySelector("#mix-generic-url").value.startsWith("http"),
    );
    const shared = await operator.locator("#mix-generic-url").inputValue();
    assert(shared.endsWith("/") && !shared.includes("key="));
    await operator.locator("[data-close=network-dialog]").click();
    const mobile = await browser.newContext({
        viewport: { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
      }),
      tablet = await browser.newContext({
        viewport: { width: 1100, height: 820 },
        hasTouch: true,
      });
    const mix = await mobile.newPage();
    mix.on("pageerror", (e) => errors.push(e.message));
    await mix.goto(shared);
    await mix.waitForFunction(() => state?.shared === true);
    assert.equal(
      await mix.locator("body").getAttribute("data-device"),
      "mobile",
    );
    await mix.locator("#mix-select").selectOption("2");
    await mix.waitForFunction(() => state?.bus.index === 2);
    assert.equal(
      await mix.locator(".name").first().innerText(),
      "Voz principal",
    );
    assert(
      (await mix.locator("#mix-select").innerText()).includes("IEM cantante"),
    );
    async function dragTouch(page, selector, delta) {
      const locator = page.locator(selector).first();
      await locator.waitFor({ state: "visible" });
      const rect = await locator.boundingBox();
      assert(rect && rect.height > 70, "usable fader height");
      const data = await locator.evaluate((e) => ({
        value: Number(e.value),
        max: Number(e.max),
        min: Number(e.min),
      }));
      const ratio = (data.value - data.min) / (data.max - data.min);
      const x = rect.x + rect.width / 2,
        y = rect.y + 15 + (1 - ratio) * (rect.height - 30);
      const target = Math.min(
        rect.y + rect.height - 16,
        Math.max(rect.y + 16, y + delta),
      );
      const cdp = await page.context().newCDPSession(page);
      await cdp.send("Input.dispatchTouchEvent", {
        type: "touchStart",
        touchPoints: [{ x, y }],
      });
      for (let i = 1; i <= 8; i++) {
        await cdp.send("Input.dispatchTouchEvent", {
          type: "touchMove",
          touchPoints: [{ x, y: y + ((target - y) * i) / 8 }],
        });
        await page.waitForTimeout(15);
      }
      await cdp.send("Input.dispatchTouchEvent", {
        type: "touchEnd",
        touchPoints: [],
      });
      await cdp.detach();
      assert.equal(
        await page.evaluate(() => scrollY),
        0,
        "touch did not scroll document vertically",
      );
      await page.waitForFunction((s) => {
        const e = document.querySelector(s);
        return e && getComputedStyle(e).touchAction === "none";
      }, selector);
    }
    await dragTouch(mix, ".fader", -90);
    await mix.waitForFunction(() => !busy && state.channels[0].level_db > -89);
    const afterMix = await operator.evaluate(
      async () => await (await fetch("/api/config")).json(),
    );
    assert(afterMix.channels[0].sends_db[1] > -89);
    assert.equal(afterMix.channels[0].sends_db[0], -90);
    await mix.evaluate(() => {
      window.dragTimer = setInterval(() => edit(1, "level_db", -22), 5);
    });
    await operator.waitForFunction(() => cfg.channels[1].sends_db[1] === -22);
    await mix.evaluate(() => clearInterval(window.dragTimer));
    await mix.waitForFunction(() => !busy && !pending.size);
    await mix.locator("footer summary").click();
    await mix.waitForFunction(() =>
      document.querySelector("#remote-latency").textContent.includes("RTT"),
    );
    await mix.screenshot({ path: "docs/monitor-mix-0.9-mobile-mix.png" });
    await mix.locator("footer summary").click();
    // Password replaces open access; the page must request authentication, not silently keep controls.
    await operator.evaluate(async () => {
      lastStatus.remote = await api("remote", {
        action: "access",
        scope: "mix",
        mode: "password",
        password: "personas",
        max_db: 0,
      });
    });
    await mix.waitForSelector("#shared-login:not([hidden])");
    await mix.locator("#mix-login-password").fill("personas");
    await mix.locator("#shared-login button").click();
    await mix.waitForFunction(() => connected && !authRequired);
    assert(
      (await mobile.cookies()).some((c) => c.name === "mm_mix" && c.httpOnly),
    );
    assert(!(await mix.evaluate(() => document.cookie)).includes("mm_mix"));
    await operator.evaluate(async () => {
      lastStatus.remote = await api("remote", {
        action: "access",
        scope: "console",
        mode: "password",
        password: "operador",
      });
    });
    const fullLink = await operator.evaluate(
      () => lastStatus.remote.console_url,
    );
    const full = await tablet.newPage();
    full.on("pageerror", (e) => errors.push(e.message));
    await full.goto(fullLink);
    await full.locator("#password").fill("operador");
    await full.getByRole("button", { name: "Entrar", exact: true }).click();
    await full.waitForSelector("#mixer .strip");
    assert(
      await full
        .locator("body")
        .evaluate((e) => e.classList.contains("touch-mode")),
    );
    let configPosts = 0;
    await full.route("**/api/config", async (route) => {
      if (route.request().method() === "POST") {
        configPosts++;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      await route.continue();
    });
    await full.evaluate(async () => {
      for (let i = 0; i < 20; i++) {
        cfg.channels[2].gain_db = -30 + i;
        publish();
        await new Promise((resolve) => setTimeout(resolve, 5));
      }
      await queue;
    });
    await operator.waitForFunction(() => cfg.channels[2].gain_db === -11);
    assert(
      configPosts < 10,
      "drag changes coalesced instead of accumulating stale requests",
    );
    await full.unroute("**/api/config");
    const oldGain = await full.evaluate(() => cfg.channels[0].gain_db);
    await dragTouch(full, "#mixer .fader input", -65);
    await operator.waitForFunction((v) => cfg.channels[0].gain_db > v, oldGain);
    assert.equal(
      await full.evaluate(
        () => document.documentElement.scrollHeight > innerHeight,
      ),
      false,
    );
    await full.locator("#tools-menu summary").click();
    await full.locator("#open-calculator").click();
    await full.locator("#calc-plugins").fill("480");
    await full.locator("#calc-plugins").dispatchEvent("input");
    assert(
      (await full.locator("#calc-result").innerText()).includes("20.67 ms"),
    );
    assert.equal(await operator.evaluate(() => cfg.buffer), 256);
    await full.screenshot({ path: "docs/monitor-mix-0.9-calculator.png" });
    await full.locator("[data-close=calculator-dialog]").click();
    await full.locator("#tools-menu summary").click();
    await full.locator("#open-diagnostics").click();
    await full.locator("#latency-route").selectOption("2");
    await full.waitForFunction(() =>
      document
        .querySelector("#latency-details")
        .textContent.includes("Sin medición"),
    );
    assert(
      (await full.locator("#latency-details").innerText()).includes(
        "IEM baterista",
      ),
    );
    await full.screenshot({ path: "docs/monitor-mix-0.9-latency.png" });
    await full.locator("[data-close=latency-dialog]").click();
    await full.locator("#toggle-inspector").click();
    assert(await full.locator(".sidebar").isVisible());
    await full.locator("#toggle-inspector").click();
    await full.screenshot({ path: "docs/monitor-mix-0.9-main-touch.png" });
    // The local console's explicit Touch switch also works with a mouse-only desktop.
    await operator.locator("#tools-menu summary").click();
    await operator.locator("#touch-mode").selectOption("on");
    assert(
      await operator
        .locator("body")
        .evaluate((e) => e.classList.contains("touch-mode")),
    );
    await operator.locator("#touch-mode").selectOption("off");
    assert(
      !(await operator
        .locator("body")
        .evaluate((e) => e.classList.contains("touch-mode"))),
    );
    await operator.locator("#tools-menu summary").click();
    const listenLink = await operator.evaluate(async () => {
      await api("listen-access", { mode: "open" });
      return (await api("listen", { enabled: true, port: 0 })).url;
    });
    const listen = await mobile.newPage();
    listen.on("pageerror", (e) => errors.push(e.message));
    await listen.goto(listenLink);
    await listen.waitForFunction(
      () => document.querySelector("#bus").options.length === 5,
    );
    await listen.locator("#bus").selectOption("2");
    await listen.locator("#play").click();
    await listen.waitForFunction(() => sources.size > 0);
    await listen.locator("details summary").click();
    await listen.waitForFunction(() =>
      document
        .querySelector("#listen-latency")
        .textContent.includes("Cola programada del navegador"),
    );
    assert(
      (await listen.locator("#listen-latency").innerText()).includes(
        "sin medición",
      ),
    );
    await listen.waitForTimeout(200);
    await listen.screenshot({ path: "docs/monitor-mix-0.9-mobile-listen.png" });
    await listen.locator("#stop").click();
    await mix.setViewportSize({ width: 844, height: 390 });
    await mix.waitForTimeout(150);
    await dragTouch(mix, ".fader", -30);
    await full.setViewportSize({ width: 820, height: 1180 });
    await full.waitForTimeout(150);
    assert(
      await full.evaluate(() => {
        const s = document.querySelector("#mixer .strip"),
          rack = s.querySelector(".insert-rack").getBoundingClientRect(),
          eq = s.querySelector(".eq-module").getBoundingClientRect(),
          button = s
            .querySelector(".insert-row button")
            .getBoundingClientRect();
        return button.height >= 30 && rack.bottom <= eq.top + 2;
      }),
      "tall touch FX buttons fit their module",
    );
    assert.equal(await full.evaluate(() => scrollY), 0);
    await full.setViewportSize({ width: 844, height: 390 });
    await full.waitForTimeout(150);
    await dragTouch(full, "#mixer .fader input", -20);
    await full.evaluate(async () => {
      await queue;
    });
    await operator.evaluate(async () => {
      await resizeStructure(9, 4);
    });
    await full.waitForFunction(() => cfg.channels.length === 9);
    assert.equal(await full.locator("#mixer .strip").count(), 9);
    await operator.evaluate(async () => {
      await api("remote", {
        action: "access",
        scope: "console",
        mode: "password",
        password: "nueva",
      });
    });
    await full.waitForSelector("#password");
    await mix.waitForFunction(() => connected);
    assert.deepEqual(errors, []);
    console.log(
      "PASS: mobile/tablet/desktop layouts, real touch fader drags without vertical page scrolling in both orientations, network adapter selection, generic aux selection and names, password login and HttpOnly roles, full-console sync/control, touch switch/inspector, latency components/calculator and mobile listening",
    );
  } finally {
    await browser?.close();
    app.kill();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
