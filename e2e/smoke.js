// End-to-end smoke test. Injected into the real PuriCat window by a debug build
// (see `PURICAT_E2E_SCRIPT` in src-tauri/src/lib.rs) and driven through the DOM, so every
// step goes through the real UI, the real IPC layer and real database servers.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const PAUSE = 1400; // long enough for the runner to take a screenshot of each stage
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const log = [];
  const report = (stage, extra) => invoke("e2e_report", { text: JSON.stringify({ stage, ...extra, log }, null, 1) });
  const stage = async (name, data = {}) => {
    log.push({ stage: name, ...data });
    await report(name);
    await sleep(PAUSE);
  };

  try {
    // First load: pin language and theme, then reload so the run is deterministic.
    if (!sessionStorage.getItem("e2e")) {
      sessionStorage.setItem("e2e", "1");
      await invoke("set_setting", { key: "ui", value: JSON.stringify({ language: "en", theme: "light", aiOpen: false }) });
      location.reload();
      return;
    }
    if (window.__e2eStarted) return;
    window.__e2eStarted = true;

    const waitFor = async (fn, what, timeout = 10000) => {
      const end = Date.now() + timeout;
      for (;;) {
        const v = fn();
        if (v) return v;
        if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
        await sleep(40);
      }
    };
    const all = (sel) => [...document.querySelectorAll(sel)];
    const withText = (sel, text) => all(sel).find((e) => e.textContent.replace(/\s+/g, " ").trim().includes(text));
    const exact = (sel, text) => all(sel).find((e) => e.textContent.replace(/\s+/g, " ").trim() === text);
    const click = async (el, what) => {
      if (!el) throw new Error(`nothing to click: ${what}`);
      el.scrollIntoView({ block: "nearest" });
      el.click();
      await sleep(60);
    };
    const type = async (el, value) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, value);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(30);
    };
    const field = (label) => {
      const l = all("label").find((x) => x.querySelector("span")?.textContent.trim().startsWith(label));
      if (!l) throw new Error(`no field labelled ${label}`);
      return l.querySelector("input, select");
    };
    const dialog = () => document.querySelector('[role="dialog"], [role="alertdialog"]');
    // Rows are matched on their label, not on badges or engine tags around it.
    const rowLabel = (r) => r.querySelector(".truncate")?.textContent.replace(/\s+/g, " ").trim() ?? "";
    const treeRows = (text) => all("[data-tree-row]").filter((r) => rowLabel(r) === text);
    const treeRow = (text) => treeRows(text)[0];
    const rightClick = async (el) => {
      const r = el.getBoundingClientRect();
      el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 10 }));
      await sleep(150);
    };
    const key = async (k, opts = {}) => {
      (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, ...opts }));
      await sleep(120);
    };
    // Inactive tabs stay mounted (hidden), so always look inside the visible panel.
    const panelSel = '[role="tabpanel"]:not([hidden])';
    const gridCells = () => all(`${panelSel} [role="grid"] [role="row"]`).slice(1, 2).flatMap((r) => [...r.querySelectorAll('[role="gridcell"]')].map((c) => c.textContent.trim()));
    const footer = () => document.querySelector('[role="tabpanel"]:not([hidden]) [data-table-status]')?.textContent.replace(/\s+/g, " ").trim();
    const activePanel = () => document.querySelector('[role="tabpanel"]:not([hidden])');

    await waitFor(() => exact("h1", "Welcome to PuriCat"), "welcome tab");
    await stage("welcome-empty", { emptyState: !!withText("div", "No connections yet") });

    // ---- create connections through the dialog --------------------------------------
    const create = async ({ name, engine, host, port, user, environment }) => {
      await click(document.querySelector('button[aria-label="New connection"]'), "new connection");
      await waitFor(dialog, "connection dialog");
      if (engine) await click(withText('[role="dialog"] [role="radio"]', engine), engine);
      await type(field("Name"), name);
      await type(field("Host"), host);
      if (port) await type(field("Port"), port);
      await type(field("Username"), user);
      if (environment) await click(exact('[role="dialog"] [role="radio"]', environment), environment);
      await click(exact('[role="dialog"] button', "Test connection"), "test");
      const result = await waitFor(
        () => document.querySelector('[role="dialog"] [data-test-result]'),
        `test result for ${name}`,
        20000,
      );
      return result.textContent.replace(/\s+/g, " ").trim();
    };
    const save = async () => {
      await click(exact('[role="dialog"] button', "Create connection"), "create");
      await waitFor(() => !dialog(), "dialog to close");
    };

    const mysqlTest = await create({ name: "Demo MySQL", host: "127.0.0.1", user: "__MYSQL_USER__" });
    if (!mysqlTest.startsWith("Connected to MySQL")) throw new Error(`MySQL test failed: ${mysqlTest}`);
    await stage("dialog-mysql-tested", { message: mysqlTest });
    await save();

    const pgTest = await create({ name: "Demo PostgreSQL", engine: "PostgreSQL", host: "127.0.0.1", user: "__PG_USER__", environment: "Staging" });
    if (!pgTest.startsWith("Connected to PostgreSQL")) throw new Error(`PostgreSQL test failed: ${pgTest}`);
    log.push({ stage: "dialog-pg-tested", message: pgTest, portFollowedEngine: field("Port").value });
    await save();

    // A server that is not there must produce a clear error, not a hang or a fake success.
    const broken = await create({ name: "Unreachable", host: "127.0.0.1", port: "1", user: "nobody", environment: "Production" });
    if (broken.startsWith("Connected")) throw new Error("unreachable server reported success");
    await stage("dialog-error", { message: broken, prodWarning: !!withText('[role="dialog"] div', "consider enabling the read-only session") });
    await click(exact('[role="dialog"] button', "Cancel"), "cancel");
    await waitFor(() => !dialog(), "dialog to close");

    // ---- browse MySQL ---------------------------------------------------------------
    await click(treeRow("Demo MySQL"), "mysql node");
    await click(await waitFor(() => treeRow("puricat_e2e"), "mysql database list"), "mysql db");
    await click(await waitFor(() => treeRow("orders"), "mysql tables"), "orders");
    await waitFor(() => gridCells().includes("SO-10001"), "mysql rows");
    const mysqlFirst = gridCells();
    await stage("mysql-orders", {
      firstRow: mysqlFirst,
      footer: footer(),
      renderedRows: all(`${panelSel} [role="grid"] [role="row"]`).length - 1,
      treeGroups: all("[data-tree-row]").map(rowLabel).filter((x) => /^(Tables|Views) \(/.test(x)),
    });

    // sort descending by total_amount (two header clicks), then page forward
    const header = () => all(`${panelSel} [role="columnheader"] button`).find((b) => b.textContent.trim() === "total_amount");
    await click(header(), "sort asc");
    await waitFor(() => header().parentElement.getAttribute("aria-sort") === "ascending" && gridCells()[0] !== mysqlFirst[0], "ascending sort");
    const ascFirst = gridCells();
    await click(header(), "sort desc");
    await waitFor(() => header().parentElement.getAttribute("aria-sort") === "descending" && gridCells()[5] !== ascFirst[5], "descending sort");
    const descFirst = gridCells();
    await click(document.querySelector('[role="tabpanel"]:not([hidden]) button[aria-label="Next page"]'), "next page");
    await waitFor(() => footer()?.includes("Rows 201–400"), "second page");
    await stage("mysql-sorted-page2", { ascTotal: ascFirst[5], descTotal: descFirst[5], footer: footer() });

    await click(exact('[role="tabpanel"]:not([hidden]) [role="radio"]', "Structure"), "structure");
    await waitFor(() => activePanel().querySelector("table"), "structure table");
    await stage("mysql-structure", {
      columns: [...activePanel().querySelectorAll("tbody tr")].map((tr) => [...tr.children].slice(1).map((td) => td.textContent.trim()).join(" | ")),
    });
    await click(exact('[role="tabpanel"]:not([hidden]) [role="radio"]', "Data"), "data");

    // ---- browse PostgreSQL at the same time -----------------------------------------
    await click(treeRow("Demo PostgreSQL"), "pg node");
    const pgDb = await waitFor(() => treeRows("puricat_e2e")[1], "pg database list");
    await click(pgDb, "pg db");
    await click(await waitFor(() => treeRow("shop"), "pg schemas"), "shop schema");
    const pgOrders = await waitFor(() => treeRows("orders")[1], "pg tables");
    await click(pgOrders, "pg orders");
    await waitFor(() => activePanel()?.textContent.includes("shop") && gridCells().includes("SO-10001") && gridCells().some((c) => c.includes("channel")), "pg rows");
    await stage("pg-orders", {
      firstRow: gridCells(),
      footer: footer(),
      tabs: all('[role="tab"]').map((t) => t.textContent.replace(/\s+/g, " ").trim()),
      status: document.querySelector("footer")?.textContent.trim(),
    });

    // ---- AI panel: real CLI detection ------------------------------------------------
    await click(document.querySelector('button[aria-label="Toggle AI panel"]'), "ai toggle");
    const aiCard = () => all("aside").at(-1);
    await waitFor(() => /Installed|Not installed/.test(aiCard()?.textContent ?? ""), "claude detection", 30000);
    const claude = aiCard().querySelector("section").textContent.replace(/\s+/g, " ").trim();
    await click(exact('aside [role="radio"]', "Codex"), "codex");
    await sleep(200);
    const codex = aiCard().querySelector("section").textContent.replace(/\s+/g, " ").trim();
    await stage("ai-panel", { claude, codex });

    // ---- duplicate + delete through context menus -----------------------------------
    await rightClick(treeRow("Demo MySQL"));
    await click(await waitFor(() => exact('[role="menuitem"]', "Duplicate"), "context menu"), "duplicate");
    const copy = await waitFor(() => treeRow("Demo MySQL copy"), "duplicated connection");
    await rightClick(copy);
    await click(await waitFor(() => exact('[role="menuitem"]', "Delete…"), "context menu"), "delete");
    await waitFor(() => document.querySelector('[role="alertdialog"]'), "confirm dialog");
    await stage("delete-confirm", { text: document.querySelector('[role="alertdialog"]').textContent.replace(/\s+/g, " ").trim() });
    await click(exact('[role="alertdialog"] button', "Delete connection"), "confirm delete");
    await waitFor(() => !treeRow("Demo MySQL copy"), "connection removed");

    // ---- command palette: dark theme, then Thai -------------------------------------
    const palette = async (query) => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
      const input = await waitFor(() => document.querySelector("[cmdk-input]"), "palette");
      await type(input, query);
      await sleep(150);
      const item = document.querySelector('[cmdk-item][data-selected="true"]');
      const label = item?.textContent.trim();
      await click(item, `palette item for ${query}`);
      await waitFor(() => !document.querySelector("[cmdk-input]"), "palette to close");
      return label;
    };
    const darkItem = await palette("dark theme");
    await waitFor(() => document.documentElement.dataset.theme === "dark", "dark theme");
    await stage("dark", { ran: darkItem, bg: getComputedStyle(document.body).backgroundColor });
    const thaiItem = await palette("language");
    await waitFor(() => document.documentElement.lang === "th", "thai");
    await stage("thai-dark", { ran: thaiItem, footer: footer() });
    await palette("ธีมสว่าง");
    await waitFor(() => document.documentElement.dataset.theme === "light", "light theme");
    await click(all('[role="tab"]')[0], "welcome tab");
    await stage("thai-light-welcome", { heading: document.querySelector('[role="tabpanel"]:not([hidden]) h1')?.textContent });

    // ---- what was persisted ----------------------------------------------------------
    const saved = await invoke("list_connections");
    const settings = await invoke("get_settings");
    log.push({
      stage: "persisted",
      connections: saved.map((c) => ({ name: c.name, engine: c.engine, environment: c.environment, hasPassword: c.hasPassword, used: c.lastUsedAt !== null })),
      profileKeys: Object.keys(saved[0] ?? {}).sort(),
      ui: JSON.parse(settings.ui),
      restoredTabs: JSON.parse(settings.workspace).tabs.map((t) => t.kind === "table" ? t.table.name : t.kind),
    });
    await report("done");
  } catch (e) {
    await report("FAILED", { error: `${e?.message ?? e}\n${e?.stack ?? ""}`, where: document.querySelector('[role="dialog"]')?.textContent.slice(0, 300) });
  }
})();
