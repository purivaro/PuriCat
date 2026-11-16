// End-to-end smoke test. Injected into the real PuriCat window by a debug build
// (see `PURICAT_E2E_SCRIPT` in src-tauri/src/lib.rs) and driven through the DOM, so every
// step goes through the real UI, the real IPC layer and real database servers.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const PAUSE = 1400; // long enough for the runner to take a screenshot of each stage
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const log = [];
  // Content-Security-Policy violations and uncaught errors seen while the test ran. They are
  // part of the report: under the production policy a blocked resource fails silently.
  const problems = [];
  addEventListener("securitypolicyviolation", (e) => problems.push(`csp: ${e.violatedDirective} blocked ${e.blockedURI || "inline code"} at ${e.sourceFile ?? "?"}:${e.lineNumber ?? "?"}`));
  addEventListener("error", (e) => problems.push(`error: ${e.message}`));
  addEventListener("unhandledrejection", (e) => {
    // Monaco rejects its own pending timers with "Canceled" whenever focus moves; that is not a fault.
    if (e.reason?.name !== "Canceled") problems.push(`rejection: ${e.reason?.stack ?? e.reason}`);
  });
  const report = (stage, extra) => invoke("e2e_report", { text: JSON.stringify({ stage, ...extra, problems, log }, null, 1) });
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


    // ---- editing: type, paste from a spreadsheet, undo, review, save ------------------
    const gridEl = () => document.querySelector(`${panelSel} [role="grid"]`);
    const dataRows = () => all(`${panelSel} [role="grid"] [role="row"]`).slice(1);
    const cellAt = (r, c) => dataRows()[r]?.querySelectorAll('[role="gridcell"]')[c];
    const cellText = (r, c) => cellAt(r, c)?.textContent;
    const mouse = (el, type) => el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0 }));
    const selectCell = async (r, c) => {
      mouse(cellAt(r, c), "mousedown");
      window.dispatchEvent(new MouseEvent("mouseup"));
      gridEl().focus();
      await sleep(60);
    };
    const gridKey = async (k, opts = {}) => {
      gridEl().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...opts }));
      await sleep(120);
    };
    const editor = () => document.querySelector(`${panelSel} [aria-label="Edit cell"]`);
    const typeInEditor = async (text) => {
      const el = await waitFor(editor, "cell editor");
      const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, text);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(40);
      editor().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
      await sleep(120);
    };
    const dirtySummary = () => document.querySelector(`${panelSel} [data-dirty-summary]`)?.textContent.trim();
    const panelButton = (label) => all(`${panelSel} button`).find((b) => b.textContent.trim().startsWith(label) || b.getAttribute("aria-label") === label);
    const editState = () => document.querySelector(`${panelSel} [data-edit-state]`)?.dataset.editState;

    await click(treeRow("customers"), "mysql customers");
    await waitFor(() => cellText(0, 1) === "สมชาย ใจดี", "customers rows");
    if (editState() !== "editable") throw new Error("customers should be editable");

    // 1. double-click a cell and type
    mouse(cellAt(0, 2), "dblclick");
    await typeInEditor("เชียงราย");
    await waitFor(() => cellAt(0, 2)?.dataset.edited === "true" && cellText(0, 2) === "เชียงราย", "staged edit");
    const afterTyping = dirtySummary();

    // 2. paste a 3x2 block copied from a spreadsheet: quotes, a tab inside a cell, and one row past the end
    let previousClipboard = null;
    try {
      previousClipboard = await invoke("plugin:clipboard-manager|read_text");
    } catch {
      /* the clipboard held no text */
    }
    await invoke("plugin:clipboard-manager|write_text", {
      text: 'Kenji T.\tKyoto\r\n"มาลี ""คนใหม่"""\t"ภูเก็ต\tใต้"\r\nNew Co\tTokyo\r\n',
    });
    await selectCell(3, 1);
    await gridKey("v", { metaKey: true });
    await waitFor(() => document.querySelector(`${panelSel} [data-row-state="inserted"]`), "pasted new row");
    const afterPaste = dirtySummary();
    const pasted = [cellText(3, 1), cellText(3, 2), cellText(4, 1), cellText(4, 2), cellText(5, 1), cellText(5, 2)];

    // 3. undo and redo the paste
    await gridKey("z", { metaKey: true });
    await waitFor(() => !document.querySelector(`${panelSel} [data-row-state="inserted"]`), "undo");
    const afterUndo = dirtySummary();
    await gridKey("z", { metaKey: true, shiftKey: true });
    await waitFor(() => document.querySelector(`${panelSel} [data-row-state="inserted"]`), "redo");

    // 4. add a row with the button and type straight into it
    await click(panelButton("Add row"), "add row");
    await waitFor(() => all(`${panelSel} [data-row-state="inserted"]`).length === 2, "second new row");
    await gridKey("Q");
    await typeInEditor("Quick Shop");
    await waitFor(() => cellText(6, 1) === "Quick Shop", "typed into new row");

    // 5. mark a customer that still has orders for deletion: the review step is mandatory,
    //    the server refuses (foreign key), and nothing at all is saved
    mouse(dataRows()[2].firstElementChild, "mousedown");
    await sleep(80);
    await click(panelButton("Delete selected rows"), "delete rows");
    await waitFor(() => dataRows()[2].dataset.rowState === "deleted", "row marked deleted");
    await stage("mysql-editing", { afterTyping, afterPaste, afterUndo, pasted, beforeSave: dirtySummary() });

    await click(panelButton("Save"), "save");
    const review = await waitFor(() => document.querySelector("[data-review-dialog]"), "review dialog");
    await waitFor(() => review.querySelector("ol li"), "planned statements");
    const planned = [...review.querySelectorAll("ol li")].map((li) => li.textContent.replace(/\s+/g, " ").trim().slice(0, 150));
    await stage("review-dialog", { planned });
    await click(exact("[data-review-dialog] button", "Save changes"), "commit");
    const refused = await waitFor(() => document.querySelector("[data-review-error]"), "foreign key error");
    log.push({ stage: "save-refused", message: refused.textContent.trim() });
    await click(exact("[data-review-dialog] button", "Cancel"), "cancel review");
    await waitFor(() => !document.querySelector("[data-review-dialog]"), "review closed");
    if (cellText(0, 2) !== "เชียงราย" || !dirtySummary()) throw new Error("staged edits were lost after a failed save");

    // 6. restore the row; the remaining small save goes straight through
    mouse(dataRows()[2].firstElementChild, "mousedown");
    await sleep(80);
    await click(panelButton("Restore selected rows"), "restore rows");
    await waitFor(() => !dataRows()[2].dataset.rowState, "row restored");
    await click(panelButton("Save"), "save");
    await waitFor(() => !document.querySelector(`${panelSel} [data-dirty-bar]`) && dataRows().length === 7, "saved and reloaded", 15000);
    const savedRows = dataRows().map((r) => [...r.querySelectorAll('[role="gridcell"]')].slice(0, 3).map((c) => c.textContent));
    if (savedRows[0][2] !== "เชียงราย" || savedRows[4][1] !== 'มาลี "คนใหม่"' || savedRows[4][2] !== "ภูเก็ต\tใต้") {
      throw new Error(`saved data does not match what was typed: ${JSON.stringify(savedRows)}`);
    }
    await stage("mysql-saved", { savedRows, stillEdited: all(`${panelSel} [data-edited]`).length });

    // 7. closing a tab with unsaved edits asks first
    mouse(cellAt(1, 2), "dblclick");
    await typeInEditor("temp");
    const tabDirty = !!document.querySelector('[role="tab"][aria-selected="true"] [role="img"]');
    await click(document.querySelector('[role="tab"][aria-selected="true"] button'), "close dirty tab");
    const discard = await waitFor(() => document.querySelector('[role="alertdialog"]'), "discard dialog");
    const discardText = discard.textContent.replace(/\s+/g, " ").trim();
    await click(exact('[role="alertdialog"] button', "Keep editing"), "keep editing");
    await waitFor(() => !document.querySelector('[role="alertdialog"]'), "dialog closed");
    if (cellText(1, 2) !== "temp") throw new Error("edit lost after keeping the tab open");
    await click(panelButton("Discard"), "discard");
    await waitFor(() => !document.querySelector(`${panelSel} [data-dirty-bar]`) && cellText(1, 2) === "Chiang Mai", "discarded");
    log.push({ stage: "dirty-tab-guard", tabDirty, discardText });

    // 8. views and keyless tables are read-only, and say why
    await click(treeRow("audit_log"), "keyless table");
    await waitFor(() => document.querySelector(`${panelSel} h1`)?.textContent === "audit_log" && editState(), "audit_log");
    const keyless = editState();
    await click(all("[data-tree-row]").find((r) => rowLabel(r).startsWith("Views (")), "views group");
    await click(await waitFor(() => treeRow("order_totals"), "view row"), "view");
    await waitFor(() => document.querySelector(`${panelSel} h1`)?.textContent === "order_totals" && editState(), "view");
    log.push({ stage: "read-only-objects", keyless, view: editState() });

    // 8b. Smart Paste (MySQL): upsert on the unique order number, with a header row
    await click(treeRow("orders"), "mysql orders for smart paste");
    await waitFor(() => document.querySelector(`${panelSel} h1`)?.textContent === "orders" && editState() === "editable", "orders tab");
    await invoke("plugin:clipboard-manager|write_text", {
      text: [
        "order_no\tStatus\ttotal_amount\tcustomer_id\torder_date\tremark",
        "SO-10001\tCompleted\t999.99\t3\t2026-01-10 12:41:31\tignored",
        "SO-10002\tPending\t278.38\t5\t2026-01-10 14:58:31\tignored",
        "SO-99001\tPending\t10\t1\t2026-11-16 09:00:00\tignored",
        "SO-99002\tPending\tlots\t1\t2026-11-16 09:05:00\tignored",
        "\t\t\t\t\t",
      ].join("\r\n") + "\r\n",
    });
    const smart = () => document.querySelector("[data-smart-paste]");
    const smartButton = (label) => all("[data-smart-paste] button").find((b) => b.textContent.trim().startsWith(label));
    const smartCounts = () => Object.fromEntries(all("[data-smart-count]").map((e) => [e.dataset.smartCount, Number(e.querySelector("b").textContent)]));
    const smartRows = () => all("[data-smart-preview] tbody tr").map((tr) => [...tr.children].map((td) => td.textContent.trim()).join(" | "));

    await click(panelButton("Smart Paste…"), "open smart paste");
    await waitFor(() => document.querySelector("[data-smart-source]"), "clipboard parsed");
    const smartSetup = {
      source: document.querySelector("[data-smart-source]").textContent.trim(),
      headersDetected: all('[data-smart-paste] input[type="checkbox"]')[0].checked,
      mapping: all('[data-smart-paste] select[aria-label^="Table column"]').map((el) => el.value),
      key: document.querySelector("[data-smart-key]").selectedOptions[0]?.textContent,
    };
    await click(smartButton("Preview"), "preview");
    await waitFor(() => document.querySelector("[data-smart-preview]"), "preview result", 15000);
    const badPreview = { counts: smartCounts(), rows: smartRows() };
    if (!smartButton("Run").disabled) throw new Error("Run must be disabled while a row has a problem");
    await stage("smart-paste-problem", { smartSetup, badPreview });

    // fix the source and read the clipboard again
    await invoke("plugin:clipboard-manager|write_text", {
      text:
        "order_no\tStatus\ttotal_amount\tcustomer_id\torder_date\r\n" +
        "SO-10001\tCompleted\t999.99\t3\t2026-01-10 12:41:31\r\n" +
        "SO-10002\tPending\t278.38\t5\t2026-01-10 14:58:31\r\n" +
        "SO-99001\tPending\t10\t1\t2026-11-16 09:00:00\r\n",
    });
    await click(smartButton("Read clipboard"), "re-read clipboard");
    await sleep(300);
    await click(smartButton("Preview"), "preview again");
    await waitFor(() => smartCounts().problems === 0 && smartCounts().insert === 1, "clean preview", 15000);
    const goodPreview = { counts: smartCounts(), rows: smartRows() };
    await stage("smart-paste-preview", { goodPreview });
    await click(smartButton("Run"), "run smart paste");
    await waitFor(() => !smart(), "smart paste finished", 20000);
    // this tab was left sorted and on page 2: sort by id to find the rows that were touched
    const idHeader = () => all(`${panelSel} [role="columnheader"] button`).find((b) => b.textContent.trim() === "id");
    await click(idHeader(), "sort id ascending");
    await waitFor(() => cellText(0, 1) === "SO-10001" && cellText(0, 4) === "Completed" && cellText(0, 5) === "999.99", "updated row visible");
    await click(idHeader(), "sort id descending");
    await waitFor(() => cellText(0, 1) === "SO-99001", "inserted row visible");
    log.push({ stage: "smart-paste-done", updated: ["Completed", "999.99"], inserted: [cellText(0, 1), cellText(0, 4), cellText(0, 5)] });

    // 8c. SQL console (MySQL): run a script, confirmation for a destructive statement,
    //     the impact guard, an error, the history, and exports
    await rightClick(treeRow("Demo MySQL"));
    await click(await waitFor(() => withText('[role="menuitem"]', "New query"), "new query menu"), "new query");
    const sqlTab = () => document.querySelector(`${panelSel} [data-sql-tab]`);
    await waitFor(() => sqlTab()?.__editor, "sql editor", 30000);
    const sqlButton = (label) => all(`${panelSel} button`).find((b) => b.textContent.trim().startsWith(label));
    const runSql = async (text) => {
      const before = document.querySelector(`${panelSel} [data-sql-summary]`)?.textContent ?? "";
      sqlTab().__editor.setValue(text);
      await sleep(80);
      await click(sqlButton("Run"), "run sql");
      return before;
    };
    const messages = () => all(`${panelSel} [data-sql-messages] li`).map((li) => ({ status: li.dataset.status, text: li.textContent.replace(/\s+/g, " ").trim().slice(0, 220) }));
    const sqlStatus = () => all(`${panelSel} [data-sql-messages] li`).map((li) => li.dataset.status).join(",");
    const showMessages = () => click(all(`${panelSel} [role="tab"]`).find((b) => b.textContent.trim() === "Messages"), "messages tab");

    await runSql(
      "-- two statements\nSELECT order_no, status, total_amount FROM puricat_e2e.orders WHERE order_no IN ('SO-10001', 'SO-99001') ORDER BY order_no;\nSELECT COUNT(*) AS customers FROM puricat_e2e.customers;",
    );
    await waitFor(() => gridCells().includes("SO-10001"), "sql result rows", 20000);
    const sqlFirst = {
      row: gridCells(),
      tabs: all(`${panelSel} [role="tab"]`).map((b) => b.textContent.replace(/\s+/g, " ").trim()),
      summary: document.querySelector(`${panelSel} [data-sql-summary]`)?.textContent.trim(),
    };
    await stage("sql-console", { sqlFirst });

    // a DELETE without WHERE asks first; cancelling runs nothing
    await runSql("DELETE FROM puricat_e2e.audit_log");
    const sqlConfirm = await waitFor(() => document.querySelector("[data-sql-confirm]"), "destructive confirmation");
    const confirmText = sqlConfirm.textContent.replace(/\s+/g, " ").trim().slice(0, 260);
    await click(exact("[data-sql-confirm] button", "Cancel"), "cancel destructive");
    await waitFor(() => !document.querySelector("[data-sql-confirm]"), "confirmation closed");

    // an UPDATE over the connection's row limit is rolled back, then allowed explicitly
    await runSql("UPDATE puricat_e2e.orders SET note = 'bulk' WHERE id <= 50");
    await waitFor(() => sqlStatus() === "blocked", "impact guard", 20000);
    const blocked = messages();
    await stage("sql-blocked", { confirmText, blocked });
    await click(sqlButton("Run it without the limit"), "run anyway");
    await waitFor(() => sqlStatus() === "ok" && messages()[0].text.includes("50 rows affected"), "large change applied", 20000);
    const allowed = messages();

    // an error is reported with the server's message, and later statements are skipped
    await runSql("SELECT 1;\nSELECT * FROM puricat_e2e.no_such_table;\nSELECT 2");
    await sleep(600);
    await showMessages();
    await waitFor(() => sqlStatus() === "ok,error", "sql error", 20000);
    const failed = { messages: messages(), skipped: withText(`${panelSel} [data-sql-messages] p`, "not run")?.textContent };

    // a transaction the user opens (even behind a comment) is theirs: the impact guard stays
    // out of it, the tab is flagged, the database cannot be switched, and closing asks first
    await runSql("-- held open\nBEGIN;\nUPDATE puricat_e2e.orders SET note = 'tx' WHERE id <= 40");
    const txnBadge = await waitFor(() => document.querySelector(`${panelSel} [data-sql-transaction]`), "transaction badge", 20000);
    const txn = {
      badge: txnBadge.textContent.trim(),
      statuses: sqlStatus(),
      tabFlagged: !!document.querySelector('[role="tab"][aria-selected="true"] [role="img"]'),
      databaseLocked: document.querySelector(`${panelSel} select[aria-label="Database"]`).disabled,
    };
    await click(document.querySelector('[role="tab"][aria-selected="true"] button'), "close tab in transaction");
    const txnDialog = await waitFor(() => document.querySelector('[role="alertdialog"]'), "transaction close dialog");
    txn.dialog = txnDialog.textContent.replace(/\s+/g, " ").trim();
    await stage("sql-transaction", { txn });
    await click(exact('[role="alertdialog"] button', "Keep the tab"), "keep the tab");
    await waitFor(() => !document.querySelector('[role="alertdialog"]'), "dialog closed");
    await runSql("ROLLBACK;\nSELECT COUNT(*) AS still_tx FROM puricat_e2e.orders WHERE note = 'tx'");
    await waitFor(() => !document.querySelector(`${panelSel} [data-sql-transaction]`) && gridCells().join() === "0", "rolled back", 20000);
    txn.afterRollback = {
      tabFlagged: !!document.querySelector('[role="tab"][aria-selected="true"] [role="img"]'),
      databaseLocked: document.querySelector(`${panelSel} select[aria-label="Database"]`).disabled,
    };
    if (txn.statuses !== "ok,ok" || !txn.tabFlagged || !txn.databaseLocked || txn.afterRollback.tabFlagged || txn.afterRollback.databaseLocked) {
      throw new Error(`transaction handling is wrong: ${JSON.stringify(txn)}`);
    }

    // a run that cannot start says so in Messages, not behind the previous result
    await runSql("-- nothing to run");
    const noStatement = await waitFor(() => document.querySelector(`${panelSel} [role="alert"]`), "failure shown");
    txn.failure = noStatement.textContent.trim();

    // history lists what ran on this connection
    await click(document.querySelector(`${panelSel} button[aria-label="History"]`), "history");
    document.querySelector(`${panelSel} button[aria-label="History"]`).dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
    const historyMenu = await waitFor(() => document.querySelector("[data-sql-history] [role=menuitem]") && document.querySelector("[data-sql-history]"), "history list");
    const historyItems = [...historyMenu.querySelectorAll("[role=menuitem]")].map((m) => m.textContent.replace(/\s+/g, " ").trim().slice(0, 90));
    // Escape is sent to the menu itself until it is gone: where focus sits at this instant varies.
    await waitFor(() => {
      const menu = document.querySelector("[data-sql-history]");
      menu?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      return !menu;
    }, "history closed");

    // exports are written by the backend from the full result
    await runSql("SELECT id, name, city FROM puricat_e2e.customers ORDER BY id");
    await waitFor(() => gridCells().includes("สมชาย ใจดี"), "customers result", 20000);
    const sqlTabId = JSON.parse((await invoke("get_settings")).workspace).tabs.find((x) => x.kind === "sql").id;
    const exported = {};
    for (const format of ["csv", "json", "xlsx"]) {
      exported[format] = await invoke("export_result", { tab: sqlTabId, result: 0, format, path: `__WORK__/export.${format}` });
    }
    log.push({ stage: "sql-more", allowed, failed, txn, historyItems, exported });

    // 9. PostgreSQL: the server refuses a bad number (the edit stays staged), then a valid save
    await click(all('[role="tab"]').find((x) => x.textContent.includes("Demo Postgr")), "postgres tab");
    await waitFor(() => cellText(0, 1) === "SO-10001" && editState() === "editable", "postgres orders tab");
    mouse(cellAt(0, 5), "dblclick");
    await typeInEditor("abc");
    await click(panelButton("Save"), "save invalid");
    const pgError = await waitFor(() => document.querySelector(`${panelSel} [data-save-error]`), "postgres rejects bad number");
    const pgRefused = pgError.textContent.trim();
    mouse(cellAt(0, 5), "dblclick");
    await typeInEditor("200.50");
    mouse(cellAt(0, 4), "dblclick");
    await typeInEditor("Completed");
    await click(panelButton("Save"), "save valid");
    await waitFor(() => !document.querySelector(`${panelSel} [data-dirty-bar]`) && cellText(0, 5) === "200.50", "postgres saved", 15000);
    log.push({ stage: "pg-edited", refused: pgRefused, saved: [cellText(0, 4), cellText(0, 5)] });

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
    if (previousClipboard !== null) await invoke("plugin:clipboard-manager|write_text", { text: previousClipboard });
    if (problems.length > 0) throw new Error(`the page reported ${problems.length} problem(s); see "problems" in the report`);
    await report("done");
  } catch (e) {
    await report("FAILED", { error: `${e?.message ?? e}\n${e?.stack ?? ""}`, where: document.querySelector('[role="dialog"]')?.textContent.slice(0, 300) });
  }
})();
