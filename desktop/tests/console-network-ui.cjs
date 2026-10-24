// Real mobile operator console over fragmented HTTP and throttled asset delivery.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const net = require("node:net");
const assert = require("node:assert/strict");
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
(async () => {
  const app = spawn("desktop/target/debug/aumux", ["--no-browser"], {
    env: {
      ...process.env,
      MONITOR_REMOTE_BIND: "127.0.0.1",
      MONITOR_LISTEN_HOST: "127.0.0.1",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  app.stderr.on("data", () => {});
  let browser, proxy;
  const sockets = new Set();
  try {
    const local = await new Promise((resolve, reject) => {
      const timeout = setTimeout(
        () => reject(Error("App startup timeout")),
        5000,
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
    });
    const html = await (await fetch(local)).text();
    const token = html.match(/name="monitor-token" content="([^"]+)"/)[1];
    async function api(route, value) {
      const response = await fetch(
        local + "/api/" + route,
        value === undefined
          ? {}
          : {
              method: "POST",
              headers: {
                "Content-Type": "application/json",
                "X-Monitor-Token": token,
                Origin: local,
              },
              body: JSON.stringify(value),
            },
      );
      const data = await response.json();
      assert(response.ok, JSON.stringify(data));
      return data;
    }
    const remote = await api("remote", { action: "start", port: 0 });
    await api("remote", { action: "access", scope: "console", mode: "open" });
    await api("start", { mode: "demo" });
    proxy = net.createServer((front) => {
      const backend = net.connect(remote.port, "127.0.0.1");
      sockets.add(front);
      sockets.add(backend);
      let header = Buffer.alloc(0),
        foundHeader = false;
      let upstream = Promise.resolve(),
        downstream = Promise.resolve();
      front.on("data", (bytes) => {
        if (!foundHeader) {
          header = Buffer.concat([header, bytes]);
          const end = header.indexOf("\r\n\r\n");
          if (end < 0) return;
          foundHeader = true;
          const first = header.subarray(0, 48),
            rest = header.subarray(48);
          upstream = upstream.then(async () => {
            if (backend.destroyed) return;
            backend.write(first);
            await delay(40);
            if (!backend.destroyed) backend.write(rest);
          });
        } else {
          upstream = upstream.then(() => {
            if (!backend.destroyed) backend.write(bytes);
          });
        }
      });
      front.on("end", () => {
        upstream.then(() => {
          if (!backend.destroyed) backend.end();
        });
      });
      backend.on("data", (bytes) => {
        downstream = downstream.then(async () => {
          for (let offset = 0; offset < bytes.length; offset += 8192) {
            await delay(4);
            if (front.destroyed) return;
            front.write(bytes.subarray(offset, offset + 8192));
          }
        });
      });
      backend.on("end", () => {
        downstream.then(() => {
          if (!front.destroyed) front.end();
        });
      });
      front.on("error", () => backend.destroy());
      backend.on("error", () => front.destroy());
      front.on("close", () => {
        sockets.delete(front);
        backend.destroy();
      });
      backend.on("close", () => sockets.delete(backend));
    });
    await new Promise((resolve) => proxy.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${proxy.address().port}/console/`;
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      isMobile: true,
      hasTouch: true,
    });
    const page = await context.newPage();
    const errors = [],
      failures = [],
      assets = new Set();
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("requestfailed", (request) => {
      if (request.failure()?.errorText !== "net::ERR_ABORTED")
        failures.push(request.url());
    });
    page.on("response", (response) => {
      if (
        /\/(console\.css|console\.js|effects-ui\.js|workspace-ui\.js|tools-ui\.js)$/.test(
          response.url(),
        ) &&
        response.status() === 200
      )
        assets.add(new URL(response.url()).pathname);
    });
    async function loaded(target) {
      await target.waitForFunction(
        () =>
          typeof cfg !== "undefined" &&
          cfg?.channels.length === 8 &&
          lastStatus?.system?.scope === "process",
      );
      await target.waitForSelector("#mixer .strip");
      assert.equal(await target.locator("#mixer .strip").count(), 8);
      assert(
        await target.evaluate(
          () =>
            getComputedStyle(document.documentElement)
              .getPropertyValue("--base")
              .trim() === "#1d2027",
        ),
        "console CSS must actually be applied",
      );
      assert((await target.locator("#system-ram").innerText()).includes("MiB"));
    }
    await page.goto(url, { waitUntil: "domcontentloaded" });
    await loaded(page);
    assert.equal(
      assets.size,
      5,
      "all CSS/JS assets completed over fragmented HTTP",
    );
    await delay(10500);
    assert.equal(await page.locator("#status").innerText(), "DEMO");
    assert.deepEqual(
      failures,
      [],
      "idle full console must not lose fetch requests",
    );
    assert.deepEqual(errors, []);
    await page.evaluate(async () => {
      cfg.channels[0].gain_db = -9;
      await publish();
    });
    assert.equal((await api("config")).channels[0].gain_db, -9);
    await context.setOffline(true);
    await page.waitForFunction(
      () => document.querySelector("#status").textContent === "RECONECTANDO…",
    );
    const current = await api("config");
    current.channels[0].name = "Voz recuperada";
    await api("config", current);
    await context.setOffline(false);
    await page.waitForFunction(
      () =>
        lastStatus?.mode === "DEMO" &&
        cfg.channels[0].name === "Voz recuperada",
    );
    await page.locator("#tools-menu > summary").click();
    await page.locator("#touch-mode").selectOption("on");
    await page.locator("#tools-menu > summary").click();
    await page.screenshot({ path: "docs/aumux-0.13-console-mobile.png" });
    await context.close();
    await api("remote", {
      action: "access",
      scope: "console",
      mode: "password",
      password: "console-network-test",
    });
    const protectedContext = await browser.newContext({
      viewport: { width: 390, height: 844 },
      isMobile: true,
      hasTouch: true,
    });
    const protectedPage = await protectedContext.newPage();
    protectedPage.on("pageerror", (error) => errors.push(error.message));
    await protectedPage.goto(url, { waitUntil: "domcontentloaded" });
    await protectedPage.locator("#password").fill("console-network-test");
    await protectedPage
      .getByRole("button", { name: "Entrar", exact: true })
      .click();
    await loaded(protectedPage);
    assert(
      (await protectedContext.cookies()).some(
        (c) => c.name === "mm_console" && c.httpOnly,
      ),
    );
    for (let repeat = 0; repeat < 2; repeat++) {
      await protectedPage.reload({ waitUntil: "domcontentloaded" });
      await loaded(protectedPage);
    }
    await protectedPage.evaluate(async () => {
      cfg.channels[1].pan = 0.25;
      await publish();
    });
    assert.equal((await api("config")).channels[1].pan, 0.25);
    assert.deepEqual(errors, []);
    await protectedContext.close();
    console.log(
      "PASS: full mobile console loads all CSS/JS over fragmented HTTP and throttled responses, idle polling >10 s, real operator writes, offline recovery, password/HttpOnly auth and reloads; no physical Windows/Wi-Fi claim.",
    );
  } finally {
    if (browser) await browser.close();
    for (const socket of sockets) socket.destroy();
    if (proxy) await new Promise((resolve) => proxy.close(resolve));
    app.kill();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
