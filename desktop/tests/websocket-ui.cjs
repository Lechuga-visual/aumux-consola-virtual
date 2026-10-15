// Browser regression tests for batching, stale replies and WebSocket recovery.
// Protocol is mocked here; websocket-smoke.py exercises the real Rust server.
const { chromium } = require("playwright-core");
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const http = require("node:http");

(async () => {
  const html = await fs.readFile("desktop/web/remote.html", "utf8");
  const httpMixRequests = [];
  const server = http.createServer((request, response) => {
    if (request.url.startsWith("/mix")) httpMixRequests.push(request.url);
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    response.end(html);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${server.address().port}`;
  let browser;
  const intervals = new Set();
  try {
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });
    const page = await browser.newPage({
      viewport: { width: 390, height: 844 },
    });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const commands = [],
      hellos = [];
    let holdAcks = true,
      active = null,
      revision = 1,
      value = -12,
      muted = false;
    let pendingAcks = [],
      pauseCurrent = false;
    function view(aux = 1, shared = false) {
      return {
        epoch: 1,
        revision,
        shared,
        max_db: 0,
        allow_pre: true,
        running: true,
        emergency: false,
        failed: false,
        bus: {
          index: aux,
          name: aux === 1 ? "Monitores" : "Invitado",
          label: "Mi mezcla",
          format: "mono",
          mute: false,
          peak: [0.1, 0.1],
        },
        choices: [
          { index: 1, name: "Monitores" },
          { index: 2, name: "Invitado" },
        ],
        channels: [0, 1].map((index) => ({
          index,
          name: index ? "Guitarra" : "Voz",
          format: "mono",
          level_db: index ? -18 : value,
          mute: index ? muted : false,
          pre: false,
          channel_mute: false,
          peak: [0.05, 0.05],
          rms: [0.01, 0.01],
        })),
        latency: {
          block_ms: 5.333,
          driver_ms: null,
          buses: [{ plugin_ms: 0 }, { plugin_ms: 0 }, { plugin_ms: 0 }],
        },
      };
    }
    function send(route, message) {
      if (!route.closed) route.send(JSON.stringify(message));
    }
    function confirm(route, command, aux, shared) {
      if (route.closed) return;
      for (const change of command.changes) {
        if (change.channel === 0 && "level_db" in change)
          value = change.level_db;
        if (change.channel === 1 && "mute" in change) muted = change.mute;
      }
      revision++;
      send(route, { type: "ack", seq: command.seq, state: view(aux, shared) });
    }
    await page.routeWebSocket("**/mix/ws", (route) => {
      const record = {
        route,
        aux: 1,
        shared: false,
        hello: false,
        closed: false,
      };
      active = record;
      pauseCurrent = false;
      route.closed = false;
      route.onMessage((raw) => {
        const message = JSON.parse(String(raw));
        if (message.type === "hello") {
          hellos.push(message);
          record.aux = message.aux ?? 1;
          record.shared = !message.key;
          record.hello = true;
          send(route, {
            type: "state",
            state: view(record.aux, record.shared),
          });
        } else if (message.type === "change") {
          commands.push(message);
          if (holdAcks)
            pendingAcks.push(() =>
              confirm(route, message, record.aux, record.shared),
            );
          else confirm(route, message, record.aux, record.shared);
        } else if (message.type === "ping" && !pauseCurrent) {
          send(route, { type: "pong", sent: message.sent });
        }
      });
      const interval = setInterval(() => {
        if (record.hello && !pauseCurrent)
          send(route, {
            type: "state",
            state: view(record.aux, record.shared),
          });
      }, 100);
      intervals.add(interval);
      route.onClose(() => {
        route.closed = true;
        record.closed = true;
        clearInterval(interval);
        intervals.delete(interval);
      });
    });
    await page.goto(url + "/#key=private-fixture", {
      waitUntil: "domcontentloaded",
    });
    await page.waitForFunction(() => connected && widgets.size === 2);
    assert.equal(hellos[0].key, "private-fixture");
    assert.equal(
      active.route.url().includes("private-fixture"),
      false,
      "secret must not enter the WebSocket URL",
    );
    const started = Date.now();
    await page.evaluate(() => {
      edit(0, "level_db", -15);
      edit(1, "mute", true);
    });
    await page.waitForFunction(() => busy);
    while (!commands.length)
      await new Promise((resolve) => setTimeout(resolve, 10));
    assert(
      Date.now() - started >= 60,
      "edits should wait for the 80 ms aggregation window",
    );
    assert.equal(commands.length, 1);
    assert.equal(
      commands[0].changes.length,
      2,
      "one batch includes independent channels",
    );
    await page.evaluate(() => {
      edit(0, "level_db", -10);
      edit(0, "level_db", -8);
      edit(0, "level_db", -6);
    });
    // Simulate a delayed confirmation longer than the old two-second socket
    // cutoff while normal state frames continue to arrive.
    await page.waitForTimeout(2300);
    assert.equal(await page.evaluate(() => connected), true);
    assert.equal(
      commands.length,
      1,
      "only one unconfirmed batch may be in flight",
    );
    assert.equal(
      await page.locator('[data-channel="0"] .value').inputValue(),
      "-6",
      "old snapshots must not overwrite the latest finger position",
    );
    pendingAcks.shift()();
    await page.waitForFunction(
      () =>
        busy && pending.size === 0 && flight?.changes.get(0)?.level_db === -6,
    );
    assert.equal(commands.length, 2);
    assert.deepEqual(
      commands[1].changes,
      [{ channel: 0, level_db: -6 }],
      "coalesce intermediate positions",
    );
    pendingAcks.shift()();
    await page.waitForFunction(
      () => !busy && state.channels[0].level_db === -6,
    );
    send(active.route, {
      type: "state",
      state: {
        ...view(),
        revision: 1,
        channels: view().channels.map((channel) => ({
          ...channel,
          level_db: -60,
        })),
      },
    });
    await page.waitForTimeout(30);
    assert.equal(
      await page.locator('[data-channel="0"] .value').inputValue(),
      "-6",
      "ignore older revisions",
    );

    // A disconnect must discard both the unconfirmed batch and later edits.
    await page.evaluate(() => edit(0, "level_db", -22));
    await page.waitForFunction(() => busy);
    await page.evaluate(() => edit(0, "level_db", -30));
    const beforeDrop = commands.length;
    pendingAcks = [];
    active.route.closed = true;
    active.route.close({
      code: 1011,
      reason: "simulated network interruption",
    });
    await page.waitForFunction(() => !connected && !pending.size && !busy);
    assert.equal(
      await page.locator(".strip").count(),
      2,
      "keep the surface visible while reconnecting",
    );
    assert.equal(
      await page.locator('[data-channel="0"] .fader').isDisabled(),
      true,
    );
    await page.waitForFunction(
      () => connected && state.channels[0].level_db === -6,
      { timeout: 6000 },
    );
    await page.waitForTimeout(180);
    assert.equal(
      commands.length,
      beforeDrop,
      "do not replay old gestures after reconnecting",
    );
    assert.equal(
      await page.locator('[data-channel="0"] .value').inputValue(),
      "-6",
    );

    // A quiet/half-open socket also recovers, rather than remaining writable forever.
    const beforeWatchdog = hellos.length;
    pauseCurrent = true;
    await page.waitForFunction(() => !connected, { timeout: 9000 });
    await page.waitForFunction(() => connected, { timeout: 6000 });
    assert(hellos.length > beforeWatchdog);
    holdAcks = false;
    await page.evaluate(() => edit(0, "level_db", -9));
    await page.waitForFunction(
      () => !busy && !pending.size && state.channels[0].level_db === -9,
    );
    send(active.route, {
      type: "error",
      code: "auth",
      message: "Enlace revocado",
      access: { mode: "disabled" },
    });
    await page.waitForFunction(() => revoked && !connected);
    const afterRevocation = hellos.length;
    await page.waitForTimeout(600);
    assert.equal(
      hellos.length,
      afterRevocation,
      "authorization errors must not reconnect repeatedly",
    );
    assert.equal(
      await page.locator("#status").textContent(),
      "Enlace revocado",
    );

    // Shared users select a bus by making a fresh authenticated hello.
    await page.goto(url, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => connected && state.shared);
    await page.locator("#mix-select").selectOption("2");
    await page.waitForFunction(() => connected && state.bus.index === 2);
    assert.equal(hellos.at(-1).aux, 2);
    assert.equal(
      await page.locator("#mix-select option:checked").textContent(),
      "Invitado",
    );
    assert.deepEqual(
      httpMixRequests,
      [],
      "auxiliary control must not make recurring HTTP /mix requests",
    );
    assert.deepEqual(errors, []);
    console.log(
      "WebSocket browser: 80 ms batching, coalescing, one batch in flight, ordered snapshots, recovery, no stale replay, watchdog, revocation and shared aux selection OK",
    );
  } finally {
    for (const interval of intervals) clearInterval(interval);
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
