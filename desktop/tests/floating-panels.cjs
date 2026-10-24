// Keep legacy in-page workflows explicit; windows-ui.cjs tests independent windows.
module.exports = function floatingPanels(browser) {
  for (const method of ["newPage", "newContext"]) {
    const original = browser[method].bind(browser);
    browser[method] = async (...args) => {
      const target = await original(...args);
      await target.addInitScript(() => {
        try {
          localStorage.setItem("aumux-panel-mode", "floating");
        } catch {}
      });
      return target;
    };
  }
};
