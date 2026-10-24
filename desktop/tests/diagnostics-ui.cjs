const { chromium } = require("playwright-core");
const { spawn } = require("node:child_process");
const assert = require("node:assert/strict");
const path = require("node:path");
(async () => {
  const app = spawn(
    process.env.MONITOR_BINARY || path.resolve("desktop/target/debug/aumux"),
    ["--no-browser"],
    { env: process.env, stdio: ["ignore", "pipe", "pipe"] },
  );
  let browser;
  try {
    const url = await new Promise((resolve, reject) => {
      let out = "";
      let timer = setTimeout(() => reject(Error("startup timeout")), 10000);
      app.stdout.on("data", (b) => {
        out += b;
        const m = out.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) {
          clearTimeout(timer);
          resolve(m[0]);
        }
      });
      app.on("exit", (n) => reject(Error("exit " + n)));
    });
    browser = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      headless: true,
      args: ["--no-sandbox", "--no-proxy-server", "--disable-dev-shm-usage"],
    });
    require("./floating-panels.cjs")(browser);
    const page = await browser.newPage({
      viewport: { width: 1440, height: 980 },
      acceptDownloads: true,
    });
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(url);
    await page.waitForSelector("#mixer .strip");
    await page.locator("#tools-menu > summary").click();
    await page.locator("#diagnostics-menu > summary").click();
    await page.locator("#open-diagnostics").click();
    await page.waitForSelector("#latency-dialog[open]");
    await page.locator('[data-close="latency-dialog"]').click();
    await page.evaluate(() => openDiagnostic("loopback"));
    await page.locator("#diag-measure").click();
    await page.waitForFunction(() =>
      document
        .querySelector("#diagnostic-notice")
        .textContent.includes("ASIO activo"),
    );
    await page.evaluate(async () => {
      await api("start", { mode: "demo" });
    });
    await page.evaluate(() => openLatency());
    await page.waitForFunction(() =>
      document
        .querySelector("#dsp-performance")
        .textContent.includes("Bloques"),
    );
    await page.waitForFunction(
      () => lastStatus?.mode === "DEMO" && lastStatus.performance.blocks > 0,
    );
    const p = await page.evaluate(() => lastStatus.performance);
    assert(p.blocks > 0);
    assert(p.max_ms >= p.last_ms);
    await page.locator('[data-close="latency-dialog"]').click();
    await page.locator('[data-close="diagnostic-dialog"]').click();
    await page.evaluate(async () => {
      await api("stop");
      openDiagnostic("stability");
    });
    await page.locator("#diag-bench-mode").selectOption("demo");
    await page.locator("#diag-bench-buffers").fill("64,128");
    await page.locator("#diag-bench-seconds").fill("1");
    await page.locator("#diag-bench-start").click();
    await page.evaluate(async () => {
      const deadline = performance.now() + 10000;
      while (performance.now() < deadline) {
        const d = await (await fetch("/api/diagnostics")).json();
        if (d.benchmark === null && d.bench_results.length === 2) return;
        await new Promise((r) => setTimeout(r, 100));
      }
      throw Error("benchmark completion timeout");
    });
    const bench = await page.evaluate(
      async () => await (await fetch("/api/diagnostics")).json(),
    );
    assert.deepEqual(
      bench.bench_results.map((r) => r.actual_buffer),
      [64, 128],
    );
    assert(bench.bench_results.every((r) => r.performance.blocks > 0));
    const restored = await page.evaluate(async () => ({
      c: await (await fetch("/api/config")).json(),
      s: await (await fetch("/api/status")).json(),
    }));
    assert.equal(restored.c.buffer, 256);
    assert.equal(restored.s.mode, "DETENIDO");
    await page.locator("#diag-bench-seconds").fill("30");
    await page.locator("#diag-bench-start").click();
    await page.locator("#diag-bench-cancel").click();
    await page.evaluate(async () => {
      const deadline = performance.now() + 5000;
      while (performance.now() < deadline) {
        const d = await (await fetch("/api/diagnostics")).json();
        if (d.benchmark === null && d.bench_results.some((r) => r.cancelled))
          return;
        await new Promise((r) => setTimeout(r, 100));
      }
      throw Error("benchmark cancellation timeout");
    });
    await page.evaluate(() => openDiagnostic("comparison"));
    assert.equal(await page.locator("#diag-route-channel option").count(), 8);
    assert.equal(await page.locator("#diag-route-bus option").count(), 5);
    await page.evaluate(() => openDiagnostic("report"));
    await page
      .locator("#diag-report-notes")
      .fill('<script>alert("test")</script>');
    const download = page.waitForEvent("download");
    await page.locator("#diag-export-html").click();
    const file = await download;
    const html = await require("node:fs/promises").readFile(
      await file.path(),
      "utf8",
    );
    assert(html.includes("Informe de pruebas"));
    assert(html.includes("&lt;script&gt;"));
    assert(!html.includes("<script>"));
    assert(html.includes("Pruebas de buffers"));
    assert(html.includes("DEMO · solo DSP"));
    assert(!html.includes("#key="));
    const data = await page.evaluate(
      async () => await (await fetch("/api/diagnostics/report")).json(),
    );
    assert.equal(data.application.name, "AuMux");
    assert(data.status.system.peak_ram_bytes > 0);
    assert.equal(data.status.remote.sessions, undefined);
    assert.equal(data.status.listen.url, undefined);
    await page.evaluate(async () => {
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -18,
        frequency: 1000,
      };
      await publish();
      await api("start", { mode: "demo" });
      openDiagnostic("feedback");
    });
    await page.locator("#diag-monitor-start").click();
    await page.waitForSelector(".diagnostic-candidate button");
    const before = await page.evaluate(
      async () => await (await fetch("/api/diagnostics")).json(),
    );
    assert(
      Math.abs(before.analysis.feedback_candidates[0].frequency_hz - 1000) < 5,
    );
    await page.screenshot({ path: "docs/aumux-0.13-acoples.png" });
    await page.locator(".diagnostic-candidate button").first().click();
    await page.waitForFunction(() =>
      cfg.buses[0].eq.bands.some(
        (b) => b.gain_db === -6 && Math.abs(b.frequency - 1000) < 5,
      ),
    );
    await page.locator("#diag-monitor-stop").click();
    await page.evaluate(() => openDiagnostic("phase"));
    await page.locator("#diag-monitor-start").click();
    await page.waitForSelector(".phase-readout");
    assert(
      (await page.locator(".phase-readout").innerText()).includes("1.0000"),
    );
    await page.locator("#diag-monitor-stop").click();
    await page.evaluate(() => openDiagnostic("scope"));
    await page.locator("#diag-monitor-start").click();
    await page.waitForFunction(() =>
      document
        .querySelector("#diagnostic-message")
        .textContent.includes("Captura activa"),
    );
    await page.locator("#diag-scope-freeze").click();
    const image = await page
      .locator("#diag-scope-canvas")
      .evaluate((c) => c.toDataURL());
    await page.waitForTimeout(400);
    assert.equal(
      await page.locator("#diag-scope-canvas").evaluate((c) => c.toDataURL()),
      image,
    );
    await page.locator("#diag-scope-freeze").click();
    await page.locator("#diag-scope-time").selectOption("5");
    await page.locator("#diag-scope-scale").selectOption("0.0625");
    await page.waitForFunction(
      () => scopeLast?.analysis?.scope?.visible_frames === 240,
    );
    await page.screenshot({ path: "docs/aumux-0.13-osciloscopio.png" });
    await page.locator("#diag-monitor-stop").click();
    const summary = await page.evaluate(async () =>
      reportHTML(await (await fetch("/api/diagnostics/report")).json()),
    );
    assert(!summary.includes("<dt>points</dt>"));
    assert(summary.length < 80000);
    assert.deepEqual(errors, []);
    console.log(
      "PASS: diagnostic submenu/DSP, ASIO guard, DEMO buffer sweep/restore/cancel, printable escaped report, feedback detection/manual EQ cut, phase, live scope zoom/freeze",
    );
  } finally {
    await browser?.close();
    app.kill("SIGTERM");
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
