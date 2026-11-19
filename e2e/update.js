// Injected by e2e/update.sh into a bundled debug build: the updater against a real package.
// The same script runs in the installed version and, after the restart, in the new one.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const log = [];
  const report = (stage, extra) => invoke("e2e_report", { text: JSON.stringify({ stage, ...extra, log }, null, 1) });
  try {
    // First load: pin the language, then reload, so the texts compared below are English.
    if (!sessionStorage.getItem("e2e")) {
      sessionStorage.setItem("e2e", "1");
      await invoke("set_setting", { key: "ui", value: JSON.stringify({ language: "en", theme: "light", aiOpen: false }) });
      location.reload();
      return;
    }
    if (window.__e2eStarted) return;
    window.__e2eStarted = true;

    const waitFor = async (fn, what, timeout = 15000) => {
      const end = Date.now() + timeout;
      for (;;) {
        const v = fn();
        if (v) return v;
        if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
        await sleep(50);
      }
    };
    const pill = () => document.querySelector("[data-update-status]");
    const dialog = () => document.querySelector("[data-update-dialog]");
    const text = (el) => el.textContent.replace(/\s+/g, " ").trim();
    const version = await invoke("plugin:app|version");
    log.push({ running: version });

    if (version === "__NEW__") {
      // After the restart: this is the new version, and a check finds nothing newer.
      await waitFor(() => pill()?.dataset.updateStatus === "current", "the check at start-up to find nothing newer");
      const shown = text(pill());
      pill().click();
      await waitFor(() => dialog()?.dataset.updateDialog === "current", "the up-to-date dialog");
      const upToDate = text(dialog());
      if (!upToDate.includes("PuriCat is up to date") || !upToDate.includes("Version __NEW__ is the latest.")) throw new Error(`after the update the dialog says: ${upToDate}`);
      await report("done", { version, shown, upToDate });
      return;
    }

    if (version !== "__OLD__") throw new Error(`this is version ${version}, neither __OLD__ nor __NEW__`);
    await waitFor(() => pill()?.dataset.updateStatus === "available", "the update found at start-up", 40000);
    const offered = text(pill());
    if (!offered.includes(__ANNOUNCED__)) throw new Error(`the status bar offers “${offered}”`);
    pill().click();
    await waitFor(() => dialog()?.dataset.updateDialog === "available", "the update dialog");
    dialog().querySelector("[data-update-install]").click();
    let sawProgress = false;
    const outcome = await waitFor(
      () => {
        const status = dialog()?.dataset.updateDialog;
        if (status === "installing" && dialog().querySelector('[role="progressbar"]')?.getAttribute("aria-valuenow")) sawProgress = true;
        return (status === "ready" || status === "failed") && status;
      },
      "the install to end",
      180000,
    );
    const said = text(dialog());
    log.push({ offered, outcome, sawProgress, said });

    if ("__EXPECT__" === "refuse") {
      if (outcome !== "failed") throw new Error(`a package that must be refused was installed: ${said}`);
      const error = text(dialog().querySelector("[data-update-error]"));
      await report("done", { version, outcome, error });
      return;
    }
    if (outcome !== "ready") throw new Error(`the install failed: ${said}`);
    if (!said.includes("PuriCat __NEW__ is installed")) throw new Error(`after the install the dialog says: ${said}`);
    if (text(pill()) !== "Restart to finish updating") throw new Error(`after the install the status bar says “${text(pill())}”`);
    await report("installed", { version, sawProgress });
    // Give the runner a moment to look at the app on disk while the old one still runs.
    await sleep(2500);
    dialog().querySelector("[data-update-restart]").click();
  } catch (e) {
    const state = { status: document.querySelector("[data-update-status]")?.dataset.updateStatus ?? null, dialog: document.querySelector("[data-update-dialog]")?.textContent ?? null };
    // The check at start-up hides its failures: ask again and say what the backend answers.
    let check;
    try {
      check = { found: await invoke("check_update") };
    } catch (failure) {
      check = { failure };
    }
    await report("FAILED", { error: String(e?.message ?? e), state, check });
  }
})();
