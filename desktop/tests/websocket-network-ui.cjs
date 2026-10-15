// Real Chromium + Rust backend. Only the WebSocket transport is delayed/dropped;
// no application responses or mixer state are mocked. Delays apply per direction
// and measure control RTT, never physical audio or a real Wi-Fi/Tailscale path.
const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const assert = require("node:assert/strict");
const { setTimeout: sleep } = require("node:timers/promises");

(async () => {
  const app = spawn(
    process.env.AUMUX_TEST_BINARY || "desktop/target/debug/aumux",
    ["--no-browser"],
    {
      env: {
        ...process.env,
        MONITOR_REMOTE_BIND: "127.0.0.1",
        MONITOR_LISTEN_HOST: "127.0.0.1",
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  let browser;
  let stderr = "";
  const timers = new Set();
  const records = [];
  const pageErrors = [];
  const httpMixRequests = [];
  const routeErrors = [];
  let blockConnections = false;
  app.stderr.on("data", (data) => (stderr += data));

  function defer(record, task, delay) {
    const timer = setTimeout(() => {
      timers.delete(timer);
      record.timers.delete(timer);
      if (record.closed) return;
      try {
        task();
      } catch (error) {
        if (!record.closed) routeErrors.push(error.message);
      }
    }, delay);
    timers.add(timer);
    record.timers.add(timer);
  }

  async function closeRecord(record) {
    if (record.closed) return;
    record.closed = true;
    for (const timer of record.timers) {
      clearTimeout(timer);
      timers.delete(timer);
    }
    record.timers.clear();
    await Promise.allSettled([
      record.client.close({ code: 1011, reason: "test network interruption" }),
      record.server.close({ code: 1011, reason: "test network interruption" }),
    ]);
  }

  try {
    const origin = await new Promise((resolve, reject) => {
      let stdout = "";
      const timeout = setTimeout(
        () => reject(Error(`Application startup timed out: ${stderr}`)),
        8000,
      );
      app.on("error", (error) => {
        clearTimeout(timeout);
        reject(error);
      });
      app.on("exit", (code) => {
        clearTimeout(timeout);
        reject(Error(`Application exited during startup (${code}): ${stderr}`));
      });
      app.stdout.on("data", (data) => {
        stdout += data;
        const match = stdout.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (match) {
          clearTimeout(timeout);
          resolve(match[0]);
        }
      });
    });
    const html = await (await fetch(origin)).text();
    const token = html.match(
      /name="(?:monitor|aumux)-token" content="([^"]+)"/,
    )?.[1];
    assert(token, "loopback operator token is present");
    async function api(route, data) {
      const response = await fetch(`${origin}/api/${route}`, {
        ...(data === undefined
          ? {}
          : {
              method: "POST",
              headers: {
                "Content-Type": "application/json",
                "X-Monitor-Token": token,
                Origin: origin,
              },
              body: JSON.stringify(data),
            }),
        signal: AbortSignal.timeout(6000),
      });
      const value = await response.json();
      assert(response.ok, `${route}: ${JSON.stringify(value)}`);
      return value;
    }
    async function setAuthoritativeLevel(level) {
      const config = await api("config");
      config.channels[0].sends_db[0] = level;
      await api("config", config);
    }
    const config = await api("config");
    assert(config.channels.length >= 2 && config.buses.length >= 2);
    config.channels[0].name = "Voz red";
    config.channels[0].sends_db[0] = -33;
    config.channels[0].generator = {
      signal: "sine",
      level_db: -30,
      frequency: 1000,
    };
    config.channels[1].send_mute[0] = false;
    config.buses[1].name = "IEM red";
    await api("config", config);
    await api("start", { mode: "demo" });
    await api("remote", { action: "start", port: 0 });
    await api("network-host", { host: "127.0.0.1" });
    const remote = await api("remote", {
      action: "access",
      scope: "mix",
      mode: "open",
      allow_pre: true,
    });
    const shared = remote.generic_url;
    assert(shared.startsWith("http://127.0.0.1:"));

    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });

    async function mobileWithDelay(delay) {
      const context = await browser.newContext({
        viewport: { width: 390, height: 844 },
        hasTouch: true,
        isMobile: true,
      });
      const page = await context.newPage();
      page.setDefaultTimeout(12000);
      page.on("pageerror", (error) => pageErrors.push(error.message));
      page.on("request", (request) => {
        const url = new URL(request.url());
        if (/^\/mix(?:\?|$)/.test(url.pathname))
          httpMixRequests.push(request.url());
      });
      const connections = [];
      await page.routeWebSocket("**/mix/ws", (client) => {
        const server = client.connectToServer();
        const record = {
          client,
          server,
          closed: false,
          delay,
          timers: new Set(),
          commands: [],
          forwarded: [],
          acknowledgements: [],
          outstanding: new Set(),
          maxOutstanding: 0,
          holdChanges: false,
          stateFrames: 0,
        };
        records.push(record);
        connections.push(record);
        client.onMessage((raw) => {
          const message = JSON.parse(String(raw));
          if (message.type === "change") {
            record.commands.push({ message, at: Date.now() });
            record.outstanding.add(message.seq);
            record.maxOutstanding = Math.max(
              record.maxOutstanding,
              record.outstanding.size,
            );
            if (record.holdChanges) return;
          }
          defer(
            record,
            () => {
              if (message.type === "change") record.forwarded.push(message);
              server.send(raw);
            },
            delay,
          );
        });
        server.onMessage((raw) => {
          const message = JSON.parse(String(raw));
          defer(
            record,
            () => {
              if (message.type === "ack") {
                record.outstanding.delete(message.seq);
                const command = record.commands.find(
                  (item) => item.message.seq === message.seq,
                );
                record.acknowledgements.push({
                  message,
                  rtt: Date.now() - command.at,
                });
              }
              if (message.type === "state") record.stateFrames++;
              client.send(raw);
            },
            delay,
          );
        });
        client.onClose(() => closeRecord(record));
        server.onClose(() => closeRecord(record));
        if (blockConnections) void closeRecord(record);
      });
      await page.goto(shared, { waitUntil: "domcontentloaded" });
      await page.waitForFunction(() => connected && state?.shared);
      await page.evaluate(() => {
        window.__networkSamples = [];
        window.__networkSampler = setInterval(
          () => window.__networkSamples.push(connected),
          100,
        );
      });
      assert.equal(await page.locator(".name").first().innerText(), "Voz red");
      return { page, context, connections };
    }

    async function gesture(page, values) {
      await page.evaluate(async (values) => {
        const fader = document.querySelector('[data-channel="0"] .fader');
        for (const value of values) {
          fader.value = toPos(value);
          fader.dispatchEvent(new Event("input", { bubbles: true }));
          await new Promise((resolve) => setTimeout(resolve, 5));
        }
      }, values);
    }

    const reports = [];
    for (const delay of [80, 250, 800]) {
      await setAuthoritativeLevel(-33);
      const { page, context, connections } = await mobileWithDelay(delay);
      const record = connections[0];
      const started = Date.now();
      await gesture(page, [-22]);
      await page.locator('[data-channel="1"] .controls button').first().click();
      await page.waitForFunction(() => busy);
      await gesture(
        page,
        [-21, -20, -19, -18, -17, -16, -15, -14, -13, -11, -9],
      );
      await page.waitForFunction(
        () => !busy && !pending.size && state.channels[0].level_db === -9,
      );
      assert.equal((await api("config")).channels[0].sends_db[0], -9);
      assert.equal(record.commands.length, 2, "rapid moves become two batches");
      assert(
        record.commands[0].at - started >= 60,
        "first batch observes the 80 ms aggregation window",
      );
      assert.equal(record.maxOutstanding, 1, "one unconfirmed batch at a time");
      assert.equal(
        record.commands[1].message.changes.find(
          (change) => change.channel === 0,
        ).level_db,
        -9,
        "last finger position wins over intermediate positions",
      );
      const rtt = record.acknowledgements.at(-1).rtt;
      assert(
        rtt >= delay * 2 - 30,
        "both real transport directions are delayed",
      );
      // A quiet connection must survive well beyond the reported 1–2 s failure.
      // The longest profile also exceeds the old cutoff while moving controls.
      await sleep(delay === 80 ? 16000 : delay === 800 ? 3500 : 2300);
      assert.equal(
        connections.length,
        1,
        "slow RTT does not trigger reconnection",
      );
      assert.equal(await page.evaluate(() => connected), true);
      const samples = await page.evaluate(() => window.__networkSamples);
      assert(samples.length >= 20 && samples.every(Boolean));
      assert(record.stateFrames > 15, "real engine state continues arriving");
      reports.push({
        oneWayDelayMs: delay,
        measuredControlRttMs: rtt,
        connections: connections.length,
        batches: record.commands.length,
        sampledConnectedMs: samples.length * 100,
      });
      await context.close();
    }

    // Drop before a command reaches the real engine. Both its in-flight value
    // and a newer queued finger position must be discarded, never replayed.
    await setAuthoritativeLevel(-33);
    const { page, context, connections } = await mobileWithDelay(250);
    let active = connections.at(-1);
    active.holdChanges = true;
    await gesture(page, [-27]);
    await page.waitForFunction(() => busy);
    await gesture(page, [-31]);
    assert((await page.evaluate(() => pending.size)) > 0);
    blockConnections = true;
    await closeRecord(active);
    await page.waitForFunction(() => !connected && !busy && !pending.size);
    assert.equal(
      await page.locator('[data-channel="0"] .fader').isDisabled(),
      true,
    );
    assert.equal(await page.locator(".strip").count(), config.channels.length);
    assert.equal(
      active.forwarded.length,
      0,
      "dropped edits never reach engine",
    );
    await setAuthoritativeLevel(-19);
    blockConnections = false;
    await page.locator("#reconnect").click();
    await page.waitForFunction(
      () => connected && state.channels[0].level_db === -19,
    );
    await sleep(1000);
    assert.equal((await api("config")).channels[0].sends_db[0], -19);
    assert.equal(
      connections.slice(1).flatMap((record) => record.commands).length,
      0,
      "recovery reads engine state without replaying earlier gestures",
    );

    // Chromium's offline/online lifecycle should preserve the same behavior.
    await context.setOffline(true);
    await page.waitForFunction(() => !navigator.onLine && !connected);
    await setAuthoritativeLevel(-16);
    await context.setOffline(false);
    await page.waitForFunction(
      () => navigator.onLine && connected && state.channels[0].level_db === -16,
    );
    assert.equal(
      await page.locator('[data-channel="0"] .value').inputValue(),
      "-16",
    );
    await gesture(page, [-11]);
    await page.waitForFunction(
      () => !busy && state.channels[0].level_db === -11,
    );
    assert.equal((await api("config")).channels[0].sends_db[0], -11);
    assert.equal(
      httpMixRequests.length,
      0,
      "aux control performs no HTTP polling",
    );
    assert.deepEqual(pageErrors, []);
    assert.deepEqual(routeErrors, []);
    console.log(
      JSON.stringify({ realBackend: true, profiles: reports }, null, 2),
    );
    console.log(
      "PASS: real backend/mobile WebSocket, delayed bidirectional control, 16 s quiet connection, 80 ms batches, one batch in flight, final-value coalescing, interruption/offline recovery, authoritative resync and no stale replay or HTTP polling. Audio latency and physical Wi-Fi/Tailscale were not measured.",
    );
  } finally {
    blockConnections = true;
    for (const timer of timers) clearTimeout(timer);
    await Promise.allSettled(records.map(closeRecord));
    if (browser) await browser.close();
    app.kill("SIGTERM");
    if (app.exitCode === null)
      await Promise.race([
        new Promise((resolve) => app.once("exit", resolve)),
        sleep(3000).then(() => app.kill("SIGKILL")),
      ]);
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
