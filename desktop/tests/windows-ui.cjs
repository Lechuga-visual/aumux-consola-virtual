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
      const t = setTimeout(() => reject(Error("startup")), 10000);
      app.stdout.on("data", (b) => {
        out += b;
        const m = out.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) {
          clearTimeout(t);
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
    const context = await browser.newContext({
      viewport: { width: 1600, height: 1000 },
    });
    context.setDefaultTimeout(12000);
    const main = await context.newPage(),
      errors = [];
    context.on("page", (p) => p.on("pageerror", (e) => errors.push(e.message)));
    main.on("pageerror", (e) => errors.push(e.message));
    await main.goto(url);
    await main.waitForSelector("#mixer .strip");
    assert.equal(await main.evaluate(() => panelMode), "windows");
    await main.evaluate(async () => {
      cfg.channels[0].generator = {
        signal: "sine",
        level_db: -24,
        frequency: 1000,
      };
      await publish();
      await api("start", { mode: "demo" });
    });
    async function popup(fn) {
      const event = main.waitForEvent("popup");
      await main.evaluate(fn);
      const p = await event;
      await p.waitForSelector("body.detached-panel dialog[open]");
      return p;
    }
    await main.evaluate(async () => {
      await api("stop");
      const list = await (await fetch("/api/native")).json();
      const effect = list.find(
        (e) => e.id !== "native/freevox8" && e.params.length >= 2,
      );
      cfg.channels[0].inserts = [
        {
          id: effect.id,
          name: effect.name,
          path: "",
          bypass: false,
          params: Object.fromEntries(
            effect.params.map((p) => [p.id, p.default]),
          ),
        },
      ];
      await publish();
      await api("start", { mode: "demo" });
    });
    console.log("Checking native FX popup and cross-window parameters");
    const fx = await popup(() => openFx("channel", 0, 0));
    await fx.waitForSelector("#params .rotary-control");
    const values = await fx.evaluate(() => {
      const d = descriptors[Number($("classes").value)];
      return d.params.slice(0, 2).map((p) => ({
        id: p.id,
        name: p.name,
        value: p.min + (p.max - p.min) * 0.37,
      }));
    });
    await fx
      .getByLabel(values[0].name, { exact: true })
      .fill(String(values[0].value));
    await fx
      .getByLabel(values[0].name, { exact: true })
      .dispatchEvent("change");
    await main.waitForFunction(
      (p) =>
        Math.abs(cfg.channels[0].inserts[0].params[p.id] - p.value) < 0.005,
      values[0],
    );
    await main.evaluate(async (p) => {
      cfg.channels[0].inserts[0].params[p.id] = p.value;
      await publish();
    }, values[1]);
    await fx.waitForFunction(
      (p) => Math.abs(pendingFx.params[p.id] - p.value) < 0.005,
      values[1],
    );
    console.log("Checking compressor popup");
    const comp = await popup(() => openCompressor("channel", 0));
    await comp.getByLabel("Compresor threshold", { exact: true }).fill("-30");
    await main.evaluate(async () => {
      cfg.channels[3].gain_db = -3;
      await publish();
    });
    await comp.waitForTimeout(450);
    assert.equal(
      await comp
        .getByLabel("Compresor threshold", { exact: true })
        .inputValue(),
      "-30",
      "remote update must not replace a numeric draft before change",
    );

    await comp
      .getByLabel("Compresor threshold", { exact: true })
      .dispatchEvent("change");
    try {
      await main.waitForFunction(
        () => cfg.channels[0].compressor.threshold === -30,
      );
    } catch (e) {
      console.error(
        "COMP diagnostic",
        await main.evaluate(async () => ({
          local: cfg.channels[0].compressor,
          base: serverConfig.channels[0].compressor,
          backend: (await (await fetch("/api/config")).json()).channels[0]
            .compressor,
          rev: syncedRemoteRevision,
          remote: lastStatus.remote,
          status: $("status").textContent,
          error: $("error").textContent,
          down: remotePointerDown,
        })),
      );
      throw e;
    }
    await main.evaluate(async () => {
      cfg.channels[0].compressor.ratio = 7;
      await publish();
    });
    await comp.waitForFunction(
      () =>
        Number(
          document.querySelector('[aria-label="Compresor ratio"]').value,
        ) === 7,
    );
    console.log("Checking independent EQs and console faders");
    const eq = await popup(() => openEq("channel", 0));
    assert(
      !(await eq.locator("#eq-dialog").evaluate((d) => d.matches(":modal"))),
    );
    await eq.selectOption("#eq-kind", "3");
    await main.waitForFunction(() => cfg.channels[0].eq.bands[0].kind === 3);
    await main.locator("#fader-value-c0").fill("-6");
    await main.locator("#fader-value-c0").dispatchEvent("change");
    await eq.waitForFunction(() => cfg.channels[0].gain_db === -6);
    const eq2 = await popup(() => openEq("channel", 1));
    await eq2.selectOption("#eq-kind", "4");
    await main.waitForFunction(() => cfg.channels[1].eq.bands[0].kind === 4);
    assert.equal(await eq.evaluate(() => cfg.channels[0].eq.bands[0].kind), 3);
    await eq.evaluate(() => selectEqBand(2));
    const count = context.pages().length;
    await main.evaluate(() => openEq("channel", 0));
    await main.waitForTimeout(200);
    assert.equal(context.pages().length, count, "reuse open panel");
    assert.equal(
      await eq.evaluate(() => eqBandIndex),
      2,
      "reopening must preserve the selected EQ band",
    );
    const preferences = await popup(() =>
      document.getElementById("network-settings").click(),
    );
    await preferences.locator("#pref-tab-mix").click();
    await main.evaluate(async () => {
      cfg.channels[0].gain_db = -12;
      await publish();
    });
    await eq.waitForFunction(() => cfg.channels[0].gain_db === -12);
    const calc = await popup(() =>
      document.getElementById("open-calculator").click(),
    );
    await calc.locator("#calc-frames").fill("128");
    await calc.locator("#calc-frames").dispatchEvent("input");
    assert((await calc.locator("#calc-result").innerText()).includes("2.67"));
    const scope = await popup(() => openDiagnostic("scope"));
    await scope.locator("#diag-monitor-start").click();
    await scope.waitForFunction(() =>
      document
        .querySelector("#diagnostic-message")
        .textContent.includes("Captura activa"),
    );
    await main.evaluate(async () => {
      cfg.channels[0].pan = 0.5;
      await publish();
    });
    await scope.waitForFunction(() => cfg.channels[0].pan === 0.5);
    const eqClosed = eq.waitForEvent("close");
    await eq.locator('[data-close="eq-dialog"]').click();
    await eqClosed;
    assert.equal(await main.evaluate(() => lastStatus.mode), "DEMO");
    assert(!eq2.isClosed());
    await main.screenshot({ path: "docs/aumux-0.13.1-consola-libre.png" });
    await eq2.screenshot({ path: "docs/aumux-0.13.1-eq-independiente.png" });
    await comp.close();
    const phase = await popup(() => openDiagnostic("phase"));
    await phase.locator("#diag-monitor-start").click();
    const phaseOwner = await phase.evaluate(() => diagnosticOwner);
    await phase.evaluate(async (owner) => {
      const deadline = performance.now() + 5000;
      while (performance.now() < deadline) {
        const d = await (await fetch("/api/diagnostics/live")).json();
        if (d.active && d.context.owner === owner && d.analysis?.phase) return;
        await new Promise((r) => setTimeout(r, 50));
      }
      throw Error("phase capture ownership timeout");
    }, phaseOwner);
    const scopeClosed = scope.waitForEvent("close");
    await scope.close({ runBeforeUnload: true });
    await scopeClosed;
    await phase.waitForTimeout(200);
    const live = await phase.evaluate(
      async () => await (await fetch("/api/diagnostics/live")).json(),
    );
    assert(live.active);
    assert.equal(
      live.context.owner,
      phaseOwner,
      "closing older scope must not cancel newer phase capture",
    );
    await phase.close();
    await main.evaluate(async () => {
      await api("stop");
      cfg.channels[0].inserts = [];
      await publish();
    });
    try {
      await fx.waitForFunction(() => pendingFx === null);
    } catch (e) {
      console.error(
        "Stale FX diagnostic",
        await fx.evaluate(() => ({
          actual: cfg.channels[0].inserts,
          authoritative: serverConfig.channels[0].inserts,
          pending: pendingFx,
          base: fxPanelBase,
          revision: syncedRemoteRevision,
          statusRevision: lastStatus?.remote?.revision,
          status: $("status").textContent,
          error: $("error").textContent,
          fxerror: $("fxerror").textContent,
        })),
      );
      throw e;
    }
    const fxClosed = fx.waitForEvent("close");
    await fx.locator("#closefx").click();
    await fxClosed;
    assert.equal(
      await main.evaluate(
        async () =>
          (await (await fetch("/api/config")).json()).channels[0].inserts
            .length,
      ),
      0,
      "closing stale FX must not restore an externally removed insert",
    );
    await preferences.close();
    await calc.close();
    await eq2.close();
    // Fallback is also modeless, draggable, and works without popup permission.
    await main.evaluate(() => {
      panelMode = "floating";
      openEq("channel", 0);
    });
    await main.locator("#eq-dialog").waitFor({ state: "visible" });
    assert(
      !(await main.locator("#eq-dialog").evaluate((d) => d.matches(":modal"))),
    );
    const before = await main.locator("#eq-dialog").boundingBox();
    const heading = main.locator("#eq-dialog .dialog-heading h2");
    const h = await heading.boundingBox();
    await main.mouse.move(h.x + 30, h.y + 10);
    await main.mouse.down();
    await main.mouse.move(h.x + 100, h.y + 60);
    await main.mouse.up();
    const after = await main.locator("#eq-dialog").boundingBox();
    assert(after.x > before.x + 20);
    assert(after.y > before.y + 20);
    await main.evaluate(async () => {
      cfg.channels[0].gain_db = -18;
      await publish();
    });
    await main.locator('[data-close="eq-dialog"]').click();
    await main.evaluate(() => {
      const old = window.open;
      window.open = () => null;
      panelMode = "windows";
      openEq("channel", 0);
      window.open = old;
    });
    assert(await main.locator("#eq-dialog").isVisible());
    assert(
      !(await main.locator("#eq-dialog").evaluate((d) => d.matches(":modal"))),
    );
    assert((await main.locator("#error").innerText()).includes("bloqueó"));
    await main.locator('[data-close="eq-dialog"]').click();
    const remoteUrl = await main.evaluate(async () => {
      await api("network-host", { host: "127.0.0.1" });
      const r = await api("remote", { action: "start", port: 0 });
      await api("remote", { action: "access", scope: "console", mode: "open" });
      return r.console_url;
    });
    const remotePage = await context.newPage();
    await remotePage.goto(remoteUrl);
    await remotePage.waitForSelector("#mixer .strip");
    const remotePopup = remotePage.waitForEvent("popup");
    await remotePage.evaluate(() => openEq("bus", 0));
    const remoteEq = await remotePopup;
    await remoteEq.waitForSelector("#eq-dialog[open]");
    await remoteEq.selectOption("#eq-kind", "5");
    await main.waitForFunction(() => cfg.buses[0].eq.bands[0].kind === 5);
    await remoteEq.close();
    await remotePage.close();
    await main.evaluate(async () => await api("remote", { action: "stop" }));
    assert.deepEqual(errors, []);
    console.log(
      "PASS: independent EQ/FX/compressor/preferences/tools, live faders, draft preservation, shared engine and local/remote sync, stale-chain protection, capture ownership, window reuse and popup fallback",
    );
  } finally {
    await browser?.close();
    app.kill("SIGTERM");
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
