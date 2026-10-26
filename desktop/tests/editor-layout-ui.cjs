const { chromium } = require("playwright-core"),
  { spawn } = require("child_process"),
  assert = require("assert/strict");
(async () => {
  const app = spawn("desktop/target/debug/aumux", ["--no-browser"], {
    env: {
      ...process.env,
      MONITOR_MIX_DATA_DIR: "/workspace/scratch/aumux-0142-check-" + Date.now(),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let b;
  try {
    const url = await new Promise((r) =>
      app.stdout.on("data", (d) => {
        const m = String(d).match(/http:\/\/127\.0\.0\.1:\d+/);
        if (m) r(m[0]);
      }),
    );
    b = await chromium.launch({
      executablePath: "/usr/bin/chromium",
      args: ["--no-sandbox"],
    });
    const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
    const errors = [];
    p.on("pageerror", (e) => errors.push(e.message));
    await p.goto(url);
    await p.waitForSelector("#mixer .strip");
    assert.equal(await p.evaluate(() => pluginHelpEnabled), false);
    await p.evaluate(() => {
      panelMode = "floating";
      $("network-settings").click();
    });
    assert.equal(
      await p.evaluate(() => $("network-dialog").dataset.category),
      "general",
    );
    assert.equal(
      await p
        .locator(".preferences-nav [role=tab]")
        .first()
        .getAttribute("data-preference"),
      "general",
    );
    assert.equal(
      await p
        .locator(".preferences-layout")
        .evaluate(
          (e) => getComputedStyle(e).gridTemplateColumns.split(" ").length,
        ),
      2,
    );
    require("fs").mkdirSync("docs/screenshots/0.14.2", { recursive: true });
    await p.screenshot({
      path: "docs/screenshots/0.14.2/00-general-lateral.png",
    });
    const before = await p.locator(".meter-bridge").boundingBox();
    await p.getByLabel("Altura vertical de los medidores").fill("240");
    await p
      .getByLabel("Altura vertical de los medidores")
      .dispatchEvent("input");
    const after = await p.locator(".meter-bridge").boundingBox();
    assert(after.height > before.height + 70);
    await p.locator("[data-close=network-dialog]").click();
    await p.evaluate(() => openEq("channel", 0));
    assert(
      await p
        .locator("#eq-title")
        .innerText()
        .then((t) => t.includes("Ecualizador Grafico de 8 bandas")),
    );
    assert(await p.locator("#eq-dialog .plugin-help").isHidden());
    const q = await p.getByLabel("EQ banda 1 q", { exact: true }).boundingBox(),
      eq = await p.locator("#eq-dialog").boundingBox();
    assert(q.y + q.height < eq.y + eq.height);
    await p
      .locator("#eq-dialog")
      .screenshot({ path: "docs/screenshots/0.14.2/01-eq.png" });
    await p.locator("[data-close=eq-dialog]").click();
    await p.evaluate(() => openCompressor("channel", 0));
    const knobs = await p.locator("#comp-knobs").boundingBox(),
      comp = await p.locator("#compressor-dialog").boundingBox();
    assert(knobs.y + knobs.height < comp.y + comp.height);
    await p
      .locator("#compressor-dialog")
      .screenshot({ path: "docs/screenshots/0.14.2/02-compresor.png" });
    await p.locator("[data-close=compressor-dialog]").click();
    await p.locator("#tools-menu>summary").click();
    await p.locator("#diagnostics-menu>summary").click();
    await p.locator("#diagnostics-menu>div").waitFor({ state: "visible" });
    await p.screenshot({ path: "docs/screenshots/0.14.2/03-herramientas.png" });
    await p.locator("#open-calculator").click();
    assert(await p.locator("#calculator-dialog").isVisible());
    await p.locator("[data-close=calculator-dialog]").click();
    await p.evaluate(async () => {
      await ensurePluginCatalogue();
    });
    assert(
      await p.evaluate(() =>
        nativePluginEntries.every((x) => pluginDescription(x).length > 20),
      ),
    );
    assert.deepEqual(errors, []);
    console.log(
      "PASS: default help off, General first/default, sidebar, adjustable meters, complete EQ/comp controls, cascading menu, plugin descriptions",
    );
  } finally {
    await b?.close();
    app.kill();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
