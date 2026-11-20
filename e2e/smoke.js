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
    // A menu or popover that was closed can stay in the page, closed, when the window is
    // covered and its exit animation never runs. Only what is open counts: an item of the
    // same name in a menu that was closed earlier belongs to something else.
    const OPEN = '[data-state="open"]';
    const menuItem = (text) => all(`[role="menu"]${OPEN} [role="menuitem"]`).find((e) => e.textContent.replace(/\s+/g, " ").trim() === text);
    const menuItemWith = (text) => all(`[role="menu"]${OPEN} [role="menuitem"]`).find((e) => e.textContent.replace(/\s+/g, " ").trim().includes(text));
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

    // ---- updates: found by the check at start-up, shown, and harmless when it cannot install
    // The runner's stand-in server announces 99.0.0, whose package does not exist.
    const updatePill = () => document.querySelector("[data-update-status]");
    const updateDialog = () => document.querySelector("[data-update-dialog]");
    await waitFor(() => updatePill()?.dataset.updateStatus === "available", "the update found at start-up");
    const updateText = updatePill().textContent.trim();
    if (updateText !== "Update available · v99.0.0") throw new Error(`the status bar says “${updateText}”`);
    if (updateDialog()) throw new Error("the check at start-up opened a dialog by itself");
    await click(updatePill(), "the update in the status bar");
    await waitFor(() => updateDialog()?.dataset.updateDialog === "available", "the update dialog");
    const updateOffered = updateDialog().textContent.replace(/\s+/g, " ").trim();
    for (const text of ["PuriCat 99.0.0 is available", "You have version ", "Released December 1, 2026.", "A release that exists only in the test.", "Check when PuriCat starts"]) {
      if (!updateOffered.includes(text)) throw new Error(`the update dialog lacks “${text}”: ${updateOffered}`);
    }
    // The switch for the check at start-up is on until the user turns it off, and is remembered.
    const autoSwitch = () => updateDialog().querySelector('[role="switch"]');
    const savedAuto = async () => JSON.parse((await invoke("get_settings")).ui).autoUpdate;
    if (autoSwitch().getAttribute("aria-checked") !== "true") throw new Error("the check at start-up is not on by default");
    await click(autoSwitch(), "turn the check at start-up off");
    await sleep(150);
    const autoOff = await savedAuto();
    await click(autoSwitch(), "and on again");
    await sleep(150);
    const autoOn = await savedAuto();
    if (autoOff !== false || autoOn !== true) throw new Error(`the switch saved ${autoOff}, then ${autoOn}`);
    await stage("update-available", { updateText, updateOffered, autoOff, autoOn });
    // Installing cannot work here (a development build is no app bundle; a bundle finds no
    // package on the stand-in server). It must say so and leave the update on offer.
    await click(updateDialog().querySelector("[data-update-install]"), "download and install");
    await waitFor(() => updateDialog()?.dataset.updateDialog === "failed", "the refusal to install", 30000);
    const updateRefused = updateDialog().querySelector("[data-update-error]").textContent.trim();
    if (!/does not run from an app bundle|The download failed/.test(updateRefused)) throw new Error(`installing failed with “${updateRefused}”`);
    if (!updateDialog().textContent.includes("The update was not installed")) throw new Error("the failure has the wrong title");
    await stage("update-refused", { updateRefused });
    await click(exact("[data-update-dialog] button", "Close"), "close the update dialog");
    await waitFor(() => !updateDialog(), "the update dialog to close");
    if (updatePill().textContent.trim() !== updateText) throw new Error(`after the failure the status bar says “${updatePill().textContent.trim()}”`);

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

    // ---- a new database from the connection's menu ------------------------------------
    // The form only chooses; the statement shown is the backend's, and it is what runs.
    const ddl = () => document.querySelector("[data-new-database]");
    const ddlButton = (label) => all("[data-new-database] button").find((b) => b.textContent.trim() === label);
    const ddlPlan = () => ddl()?.querySelector("[data-ddl-preview]")?.textContent.trim();
    const setChoice = async (select, value) => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(select, value);
      select.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(60);
    };
    const newDatabase = async (connectionName) => {
      await rightClick(treeRow(connectionName));
      await click(await waitFor(() => menuItem("New database…"), `new database item of ${connectionName}`), "new database");
      await waitFor(ddl, "new database dialog");
    };

    await newDatabase("Demo MySQL");
    const ddlEmpty = {
      hint: ddl().querySelector("[data-ddl-preview-empty]")?.textContent.trim(),
      createDisabled: ddlButton("Create database").disabled,
      defaults: await waitFor(() => /\(utf8/.test(field("Character set").selectedOptions[0].textContent) && [field("Character set"), field("Collation")].map((el) => el.selectedOptions[0].textContent), "server defaults"),
    };
    await type(field("Name"), "puricat_e2e_made");
    await waitFor(() => ddlPlan() === "CREATE DATABASE `puricat_e2e_made`", "plain statement");
    await setChoice(field("Character set"), "utf8mb4");
    const collationsOffered = [...field("Collation").options].map((o) => o.value);
    if (!collationsOffered.includes("utf8mb4_bin") || collationsOffered.some((c) => c && !c.startsWith("utf8mb4"))) {
      throw new Error(`collations offered for utf8mb4: ${collationsOffered.slice(0, 8)}`);
    }
    await setChoice(field("Collation"), "utf8mb4_bin");
    const mysqlPlan = await waitFor(() => ddlPlan()?.endsWith("COLLATE utf8mb4_bin") && ddlPlan(), "statement with a collation");
    await stage("new-database", { ddlEmpty, mysqlPlan });
    await click(ddlButton("Create database"), "create database");
    await waitFor(() => !ddl(), "dialog to close after creating", 15000);
    await waitFor(() => treeRows("puricat_e2e_made").length === 1, "the new database in the explorer");

    // the same name again: the server's own refusal, shown in the dialog, which stays open
    await newDatabase("Demo MySQL");
    await type(field("Name"), "puricat_e2e_made");
    await waitFor(() => ddlPlan() === "CREATE DATABASE `puricat_e2e_made`", "statement again");
    await click(ddlButton("Create database"), "create again");
    const ddlRefused = (await waitFor(() => ddl()?.querySelector("[data-ddl-error]"), "the server's refusal")).textContent.trim();
    await click(ddlButton("Cancel"), "cancel new database");
    await waitFor(() => !ddl(), "dialog closed");

    // PostgreSQL has other choices: owner, template, encoding
    await newDatabase("Demo PostgreSQL");
    const pgChoices = {
      owner: field("Owner").selectedOptions[0].textContent,
      templates: [...field("Template").options].map((o) => o.textContent),
      encodings: [...field("Encoding").options].map((o) => o.value).filter((v) => ["UTF8", "LATIN1", "SJIS"].includes(v)),
    };
    await type(field("Name"), "puricat_e2e_made");
    await setChoice(field("Template"), "template0");
    await setChoice(field("Encoding"), "UTF8");
    const pgPlan = await waitFor(() => ddlPlan()?.endsWith("ENCODING 'UTF8'") && ddlPlan(), "postgres statement");
    await click(ddlButton("Create database"), "create postgres database");
    await waitFor(() => !ddl(), "dialog to close after creating", 15000);
    await waitFor(() => treeRows("puricat_e2e_made").length === 2, "the new database under both servers");
    log.push({ stage: "new-database-made", ddlRefused, pgChoices, pgPlan });

    // ---- the database menu: edit, open a SQL file, flush, rename, drop ------------------
    const dbDialog = (kind) => document.querySelector(kind ? `[data-database-dialog="${kind}"]` : "[data-database-dialog]");
    const dbButton = (label) => all("[data-database-dialog] button").find((b) => b.textContent.trim() === label);
    const dbPlan = () => dbDialog()?.querySelector("[data-ddl-preview]")?.textContent.trim();
    // The same database name exists under both servers: take the row below its connection.
    const dbRowOf = (connectionName, database) => {
      const rows = all("[data-tree-row]");
      return rows.slice(rows.indexOf(treeRow(connectionName)) + 1).find((r) => rowLabel(r) === database);
    };
    const dbMenu = async (connectionName, database, item) => {
      await rightClick(dbRowOf(connectionName, database));
      await click(await waitFor(() => menuItem(item), `${item} of ${database}`), item);
    };

    // MySQL: the form opens on what the server has, and sends only what was changed
    await rightClick(dbRowOf("Demo MySQL", "puricat_e2e_made"));
    const dbMenuItems = (await waitFor(() => menuItem("Edit database…") && all(`[role="menu"]${OPEN} [role="menuitem"]`), "database menu")).map((e) => e.textContent.replace(/\s+/g, " ").trim());
    await click(menuItem("Edit database…"), "edit database");
    await waitFor(() => dbDialog("edit") && field("Collation").value === "utf8mb4_bin", "the database as it is now");
    const editBefore = {
      charset: field("Character set").value,
      saveDisabled: dbButton("Save changes").disabled,
      empty: dbDialog().querySelector("[data-ddl-preview-empty]")?.textContent.trim(),
    };
    await setChoice(field("Collation"), "utf8mb4_unicode_ci");
    const editPlan = await waitFor(() => dbPlan()?.includes("COLLATE") && dbPlan(), "alter statement");
    if (editPlan !== "ALTER DATABASE `puricat_e2e_made` COLLATE utf8mb4_unicode_ci;") throw new Error(`alter statement: ${editPlan}`);
    await stage("database-edit", { dbMenuItems, editBefore, editPlan });
    await click(dbButton("Save changes"), "save database");
    await waitFor(() => !dbDialog(), "edit dialog to close", 15000);

    // a SQL file opens in a query tab on that database; nothing runs
    await dbMenu("Demo MySQL", "puricat_e2e_made", "Open SQL file…");
    const openedFile = await waitFor(() => document.querySelector(`${panelSel} [data-sql-tab]`)?.__editor?.getValue(), "the file in a query tab", 30000);
    if (openedFile !== "SELECT 42 AS answer; -- เปิดจากไฟล์\n") throw new Error(`opened file: ${JSON.stringify(openedFile)}`);
    await click(document.querySelector('[role="tab"][aria-selected="true"] button'), "close the file's tab");

    // MySQL maintenance from the connection's menu: the statement is shown, then run
    await rightClick(treeRow("Demo MySQL"));
    await click(await waitFor(() => menuItem("Flush"), "flush submenu"), "flush");
    await click(await waitFor(() => menuItem("Status"), "flush status"), "flush status");
    const flushPlan = await waitFor(() => dbDialog("flush") && dbPlan(), "flush statement");
    if (flushPlan !== "FLUSH STATUS") throw new Error(`flush statement: ${flushPlan}`);
    await click(dbButton("Run"), "run flush");
    await waitFor(() => !dbDialog(), "flush dialog to close", 15000);
    const flushToast = (await waitFor(() => withText("[data-sonner-toast]", "FLUSH STATUS"), "flush toast")).textContent.replace(/\s+/g, " ").trim();

    // PostgreSQL: the encoding read back from the server, then a new name
    await dbMenu("Demo PostgreSQL", "puricat_e2e_made", "Edit database…");
    await waitFor(() => dbDialog("edit") && field("Encoding").value === "UTF8", "the postgres database as it is now");
    await type(field("Name"), "puricat_e2e_renamed");
    const renamePlan = await waitFor(() => dbPlan()?.includes("RENAME TO") && dbPlan(), "rename statement");
    if (renamePlan !== 'ALTER DATABASE "puricat_e2e_made" RENAME TO "puricat_e2e_renamed";') throw new Error(`rename statement: ${renamePlan}`);
    await click(dbButton("Save changes"), "rename database");
    await waitFor(() => !dbDialog(), "rename dialog to close", 15000);
    await waitFor(() => dbRowOf("Demo PostgreSQL", "puricat_e2e_renamed") && treeRows("puricat_e2e_made").length === 1, "the new name in the explorer");

    // drop: only after the name was typed exactly
    await dbMenu("Demo PostgreSQL", "puricat_e2e_renamed", "Drop database…");
    const dropPlan = await waitFor(() => dbDialog("drop") && dbPlan(), "drop statement");
    if (dropPlan !== 'DROP DATABASE "puricat_e2e_renamed"') throw new Error(`drop statement: ${dropPlan}`);
    const dropHolds = await waitFor(() => dbDialog().querySelector("[data-drop-contents]")?.textContent.trim(), "what the database holds");
    const dropName = dbDialog().querySelector("input");
    const dropStates = [dbButton("Drop database").disabled];
    await type(dropName, "puricat_e2e_rename");
    dropStates.push(dbButton("Drop database").disabled);
    await type(dropName, "puricat_e2e_renamed");
    await waitFor(() => !dbButton("Drop database").disabled, "drop button once the name is typed");
    if (dropStates.some((disabled) => !disabled)) throw new Error("the drop button was enabled before the name was typed");
    await stage("database-drop", { dropPlan, dropHolds });
    await click(dbButton("Drop database"), "drop database");
    await waitFor(() => !dbDialog(), "drop dialog to close", 15000);
    await waitFor(() => !dbRowOf("Demo PostgreSQL", "puricat_e2e_renamed"), "the database gone from the explorer");
    log.push({ stage: "database-menu", flushPlan, flushToast, renamePlan });

    // ---- the table menu: definition, rename, empty, drop --------------------------------
    const tableDialog = (kind) => document.querySelector(kind ? `[data-table-dialog="${kind}"]` : "[data-table-dialog]");
    const tableButton = (label) => all("[data-table-dialog] button").find((b) => b.textContent.trim() === label);
    const tablePlan = () => tableDialog()?.querySelector("[data-ddl-preview]")?.textContent.trim();
    const tableMenu = async (connectionName, name, item) => {
      await rightClick(await waitFor(() => dbRowOf(connectionName, name), `${name} in the explorer`));
      await click(await waitFor(() => menuItem(item), `${item} of ${name}`), item);
    };
    // A table of the test's own, made behind the UI's back and found by refreshing.
    const scratchId = (await invoke("list_connections")).find((c) => c.engine === "mysql").id;
    const scratchSql = async (sql) => {
      const outcome = await invoke("run_sql", { id: scratchId, tab: "e2e-scratch", database: "puricat_e2e", sql, options: { maxRows: 10, changeLimit: null } });
      await invoke("close_console", { tab: "e2e-scratch" });
      const failed = outcome.statements.find((s) => s.status !== "ok");
      if (failed) throw new Error(`scratch statement ${failed.status}: ${JSON.stringify(failed).slice(0, 300)}`);
      return outcome;
    };
    await scratchSql("CREATE TABLE e2e_scratch (id INT PRIMARY KEY AUTO_INCREMENT, note VARCHAR(40) NOT NULL); INSERT INTO e2e_scratch (note) VALUES ('หนึ่ง'), ('two')");
    await dbMenu("Demo MySQL", "puricat_e2e", "Refresh");

    // Definition: MySQL's own statement, which opens in a query tab without running
    await rightClick(await waitFor(() => dbRowOf("Demo MySQL", "e2e_scratch"), "the scratch table in the explorer"));
    const tableMenuItems = (await waitFor(() => menuItem("Definition…") && all(`[role="menu"]${OPEN} [role="menuitem"]`), "table menu")).map((e) => e.textContent.replace(/\s+/g, " ").trim());
    await click(menuItem("Definition…"), "definition");
    const definition = await waitFor(() => tableDialog("definition")?.querySelector("[data-definition]")?.textContent, "the table's definition", 15000);
    if (!definition.startsWith("CREATE TABLE `e2e_scratch` (")) throw new Error(`definition: ${definition}`);
    const definitionSource = tableDialog().querySelector("[data-definition-source]").textContent.trim();
    await stage("table-definition", { tableMenuItems, definition, definitionSource });
    await click(tableButton("Open in query tab"), "open the definition in a query tab");
    await waitFor(() => !tableDialog(), "definition dialog to close");
    const definitionTab = await waitFor(() => document.querySelector(`${panelSel} [data-sql-tab]`)?.__editor?.getValue(), "the definition in a query tab", 30000);
    if (definitionTab !== definition) throw new Error(`the query tab holds ${JSON.stringify(definitionTab)}`);
    await click(document.querySelector('[role="tab"][aria-selected="true"] button'), "close the definition's tab");

    // PostgreSQL has no such statement: the text is put together from the catalogue, and says so
    await tableMenu("Demo PostgreSQL", "orders", "Definition…");
    const pgDefinition = await waitFor(() => tableDialog("definition")?.querySelector("[data-definition]")?.textContent, "the postgres definition", 15000);
    if (!pgDefinition.startsWith('CREATE TABLE "shop"."orders" (') || !/CONSTRAINT \S+ PRIMARY KEY \(id\)/.test(pgDefinition)) throw new Error(`postgres definition: ${pgDefinition}`);
    const pgDefinitionSource = tableDialog().querySelector("[data-definition-source]").textContent.trim();
    if (!pgDefinitionSource.includes("pg_dump")) throw new Error(`postgres definition note: ${pgDefinitionSource}`);
    await click(tableButton("Close"), "close the definition");
    await waitFor(() => !tableDialog(), "definition dialog to close");

    // Rename: nothing to run until the name differs, then the exact statement
    await tableMenu("Demo MySQL", "e2e_scratch", "Rename…");
    await waitFor(() => tableDialog("rename") && field("New name").value === "e2e_scratch", "the rename form");
    const renameBefore = { disabled: tableButton("Rename").disabled, empty: tableDialog().querySelector("[data-ddl-preview-empty]")?.textContent.trim() };
    if (!renameBefore.disabled) throw new Error("rename was enabled with the old name");
    await type(field("New name"), "e2e_scratch2");
    const tableRenamePlan = await waitFor(tablePlan, "rename statement");
    if (tableRenamePlan !== "RENAME TABLE `puricat_e2e`.`e2e_scratch` TO `puricat_e2e`.`e2e_scratch2`") throw new Error(`rename statement: ${tableRenamePlan}`);
    await waitFor(() => !tableButton("Rename").disabled, "rename button");
    await click(tableButton("Rename"), "rename table");
    await waitFor(() => !tableDialog(), "rename dialog to close", 15000);
    await waitFor(() => dbRowOf("Demo MySQL", "e2e_scratch2") && !dbRowOf("Demo MySQL", "e2e_scratch"), "the new name in the explorer");

    // Empty: only after the name was typed; the table's open tab then shows no rows
    await click(dbRowOf("Demo MySQL", "e2e_scratch2"), "open the scratch table");
    await waitFor(() => gridCells().includes("หนึ่ง"), "the scratch table's rows");
    await tableMenu("Demo MySQL", "e2e_scratch2", "Empty table…");
    const emptyPlan = await waitFor(() => tableDialog("empty") && tablePlan(), "empty statement");
    if (emptyPlan !== "TRUNCATE TABLE `puricat_e2e`.`e2e_scratch2`") throw new Error(`empty statement: ${emptyPlan}`);
    const confirmName = () => tableDialog().querySelector("input");
    const emptyStates = [tableButton("Empty table").disabled];
    await type(confirmName(), "e2e_scratch");
    emptyStates.push(tableButton("Empty table").disabled);
    await type(confirmName(), "e2e_scratch2");
    await waitFor(() => !tableButton("Empty table").disabled, "empty button once the name is typed");
    if (emptyStates.some((disabled) => !disabled)) throw new Error("the empty button was enabled before the name was typed");
    await click(tableButton("Empty table"), "empty table");
    await waitFor(() => !tableDialog(), "empty dialog to close", 15000);
    await waitFor(() => !activePanel().textContent.includes("หนึ่ง"), "the open tab to read the emptied table again");
    const leftRows = Number((await scratchSql("SELECT COUNT(*) FROM e2e_scratch2")).statements[0].results[0].rows[0][0]);
    if (leftRows !== 0) throw new Error(`${leftRows} rows left after emptying`);

    // Drop: the table goes from the explorer and its tab closes
    await tableMenu("Demo MySQL", "e2e_scratch2", "Drop table…");
    const tableDropPlan = await waitFor(() => tableDialog("drop") && tablePlan(), "drop table statement");
    if (tableDropPlan !== "DROP TABLE `puricat_e2e`.`e2e_scratch2`") throw new Error(`drop table statement: ${tableDropPlan}`);
    const tableDropContents = await waitFor(() => /closed/.test(tableDialog().querySelector("[data-table-contents]")?.textContent) && tableDialog().querySelector("[data-table-contents]").textContent.trim(), "what dropping takes with it");
    const tableDropDisabled = tableButton("Drop table").disabled;
    await type(confirmName(), "e2e_scratch2");
    await waitFor(() => !tableButton("Drop table").disabled, "drop table button once the name is typed");
    if (!tableDropDisabled) throw new Error("the drop table button was enabled before the name was typed");
    await stage("table-drop", { tableDropPlan, tableDropContents });
    await click(tableButton("Drop table"), "drop table");
    await waitFor(() => !tableDialog(), "drop table dialog to close", 15000);
    await waitFor(() => !dbRowOf("Demo MySQL", "e2e_scratch2") && !all('[role="tab"]').some((x) => x.title.startsWith("e2e_scratch2")), "the table gone from the explorer and its tab closed");
    log.push({ stage: "table-menu", pgDefinition, pgDefinitionSource, renameBefore, tableRenamePlan, emptyPlan });

    // ---- views through a form: a new view, tried and created; its query changed; dropped --
    const viewDialog = (kind) => document.querySelector(kind ? `[data-view-dialog="${kind}"]` : "[data-view-dialog]");
    const viewButton = (label) => all("[data-view-dialog] button").find((b) => b.textContent.replace(/\s+/g, " ").trim().startsWith(label));
    const viewPlan = () => viewDialog()?.querySelector("[data-ddl-preview]")?.textContent.trim();
    const viewEditor = () => viewDialog()?.querySelector("form")?.__editor;
    const scratchCount = async (sql) => Number((await scratchSql(sql)).statements[0].results[0].rows[0][0]);

    // New view…: the form holds the SELECT only; the statement around it is the backend's
    await dbMenu("Demo MySQL", "puricat_e2e", "New view…");
    await waitFor(() => viewDialog("new") && viewEditor(), "the view form with its editor", 30000);
    const viewBefore = {
      title: viewDialog().querySelector("h2")?.textContent.trim(),
      createDisabled: viewButton("Create view").disabled,
      empty: viewDialog().querySelector("[data-ddl-preview-empty]")?.textContent.trim(),
    };
    if (!viewBefore.createDisabled) throw new Error("create view was enabled with nothing typed");
    await type(field("Name"), "e2e_view");
    const viewQuery = "SELECT id, order_no, total_amount FROM orders WHERE status = 'Cancelled' AND id <= 40";
    viewEditor().setValue(viewQuery);
    const viewCreatePlan = await waitFor(() => viewPlan()?.includes("Cancelled") && viewPlan(), "create view statement");
    if (viewCreatePlan !== `CREATE VIEW \`puricat_e2e\`.\`e2e_view\` AS\n${viewQuery};`) throw new Error(`create view statement: ${viewCreatePlan}`);

    // a statement that is not one reading query is refused before anything exists
    viewEditor().setValue(`${viewQuery}; DROP TABLE orders`);
    const viewRefusal = await waitFor(() => !viewPlan() && viewDialog().querySelector("[data-ddl-preview-empty] .text-danger")?.textContent.trim(), "the refusal of two statements");
    if (!viewButton("Create view").disabled) throw new Error("create view was enabled for two statements");
    viewEditor().setValue(viewQuery);
    await waitFor(() => viewPlan() === viewCreatePlan && !viewButton("Create view").disabled, "the statement again");

    // Preview rows: read-only, and what it shows is what the server counts
    const cancelled = await scratchCount("SELECT COUNT(*) FROM orders WHERE status = 'Cancelled' AND id <= 40");
    if (!(cancelled > 1 && cancelled < 100)) throw new Error(`the fixture gives ${cancelled} rows for the view's query`);
    await click(viewButton("Preview rows"), "preview rows");
    const viewRows = () => all("[data-view-dialog] [data-view-rows] tbody tr");
    await waitFor(() => viewRows().length > 0, "the previewed rows", 20000);
    const viewTried = {
      rows: viewRows().length,
      columns: all("[data-view-dialog] [data-view-rows] th").map((th) => th.textContent.trim()),
      first: [...viewRows()[0].querySelectorAll("td")].map((td) => td.textContent.trim()),
      summary: viewDialog().querySelector("[data-view-tried]").textContent.trim(),
    };
    if (viewTried.rows !== cancelled || viewTried.columns.join() !== "id,order_no,total_amount" || !viewTried.summary.startsWith(`${cancelled} rows · `)) {
      throw new Error(`preview: ${JSON.stringify(viewTried)}, the server counts ${cancelled}`);
    }
    await stage("view-new", { viewBefore, viewCreatePlan, viewRefusal, viewTried });
    await click(viewButton("Create view"), "create view");
    await waitFor(() => !viewDialog(), "view form to close", 15000);
    await waitFor(() => dbRowOf("Demo MySQL", "e2e_view"), "the new view in the explorer, under Views");
    if ((await scratchCount("SELECT COUNT(*) FROM e2e_view")) !== cancelled) throw new Error("the created view does not give the previewed rows");

    // Edit view…: MySQL gives back its own rewriting of the query; another query replaces it
    await rightClick(dbRowOf("Demo MySQL", "e2e_view"));
    const viewMenuItems = (await waitFor(() => menuItem("Edit view…") && all(`[role="menu"]${OPEN} [role="menuitem"]`), "view menu")).map((e) => e.textContent.replace(/\s+/g, " ").trim());
    await click(menuItem("Edit view…"), "edit view");
    const viewKept = await waitFor(() => viewDialog("edit") && viewEditor()?.getValue(), "the view's query as the server keeps it", 30000);
    if (!/^select /i.test(viewKept) || !viewKept.includes("`puricat_e2e`.`orders`") || !field("Name").disabled) throw new Error(`edit view opened with ${viewKept}`);
    const viewQuery2 = "SELECT id, order_no, total_amount, status FROM orders WHERE status = 'Completed' AND id <= 40";
    viewEditor().setValue(viewQuery2);
    const viewReplacePlan = await waitFor(() => viewPlan()?.includes("Completed") && viewPlan(), "replace view statement");
    if (viewReplacePlan !== `CREATE OR REPLACE VIEW \`puricat_e2e\`.\`e2e_view\` AS\n${viewQuery2};`) throw new Error(`replace view statement: ${viewReplacePlan}`);
    await waitFor(() => !viewButton("Save view").disabled, "save view button");
    await click(viewButton("Save view"), "save view");
    await waitFor(() => !viewDialog(), "view form to close after saving", 15000);
    const completed = await scratchCount("SELECT COUNT(*) FROM orders WHERE status = 'Completed' AND id <= 40");
    const viewAfter = {
      rows: await scratchCount("SELECT COUNT(*) FROM e2e_view"),
      columns: await scratchCount("SELECT COUNT(*) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = 'puricat_e2e' AND TABLE_NAME = 'e2e_view'"),
    };
    if (viewAfter.rows !== completed || viewAfter.columns !== 4) throw new Error(`after the edit the view gives ${JSON.stringify(viewAfter)}, expected ${completed} rows and 4 columns`);

    // ...and it goes the way any view goes: the Drop view… form
    await tableMenu("Demo MySQL", "e2e_view", "Drop view…");
    const viewDropPlan = await waitFor(() => tableDialog("drop") && tablePlan(), "drop view statement");
    if (viewDropPlan !== "DROP VIEW `puricat_e2e`.`e2e_view`") throw new Error(`drop view statement: ${viewDropPlan}`);
    await type(confirmName(), "e2e_view");
    await waitFor(() => !tableButton("Drop view").disabled, "drop view button once the name is typed");
    await click(tableButton("Drop view"), "drop view");
    await waitFor(() => !tableDialog(), "drop view dialog to close", 15000);
    await waitFor(() => !dbRowOf("Demo MySQL", "e2e_view"), "the view gone from the explorer");
    // The form opened "Views" to show the new view; the stages that follow find it closed, as it was.
    const viewsGroup = () => all("[data-tree-row]").find((r) => rowLabel(r).startsWith("Views ("));
    const viewsWereOpened = viewsGroup().getAttribute("aria-expanded") === "true";
    if (!viewsWereOpened) throw new Error("creating a view did not open the Views group");
    await click(viewsGroup(), "close the views group again");
    await waitFor(() => viewsGroup().getAttribute("aria-expanded") === "false", "the views group closed");
    log.push({ stage: "view-form", viewMenuItems, viewKept, viewReplacePlan, viewAfter });

    // ---- users and privileges: the accounts, a new user from a template, its grants, drop
    const usersDialog = () => document.querySelector("[data-users-dialog]");
    const usersView = (kind) => usersDialog()?.querySelector(`[data-users-view="${kind}"]`);
    const usersButton = (label) => all("[data-users-dialog] button").find((b) => b.textContent.trim() === label);
    const accountRow = (label) => all("[data-users-dialog] [data-account-row]").find((r) => r.dataset.accountRow === label);
    // The statements of a form, once they answer the choices on screen.
    const usersPlan = () => all("[data-users-dialog] [data-ddl-preview]:not([aria-busy='true']) pre").map((p) => p.textContent.trim());
    const badges = (row) => [...row.querySelectorAll("span")].filter((x) => x.children.length === 0).map((x) => x.textContent.trim());
    await rightClick(treeRow("Demo MySQL"));
    await click(await waitFor(() => menuItem("Users and privileges…"), "users menu item"), "users and privileges");
    await waitFor(() => usersView("browse") && accountRow("mysql.sys@localhost"), "the accounts of the server");
    // this connection's own account is marked, and so are the server's own
    const ownRow = all("[data-users-dialog] [data-account-row]").find((r) => badges(r).includes("you"));
    if (!ownRow || !ownRow.dataset.accountRow.startsWith("__MYSQL_USER__@")) throw new Error(`the account marked "you": ${ownRow?.dataset.accountRow}`);
    if (!badges(accountRow("mysql.sys@localhost")).includes("server's own")) throw new Error("mysql.sys is not marked as the server's own");
    // its grants as the server prints them; the forms are not offered for it
    await click(ownRow, "own account");
    const ownGrants = await waitFor(() => usersDialog().querySelector("[data-grants]")?.textContent, "grants of the own account");
    if (!ownGrants.startsWith("GRANT ")) throw new Error(`grants of the own account: ${ownGrants.slice(0, 80)}`);
    const ownFixed = [usersButton("Change password…").disabled, usersButton("Drop user…").disabled];
    if (ownFixed.some((disabled) => !disabled)) throw new Error("the own account can be changed from the list");
    await stage("users", { ownAccount: ownRow.dataset.accountRow, accounts: all("[data-users-dialog] [data-account-row]").length, ownGrants: ownGrants.slice(0, 120) });

    // a read-only user for one database: the statements with the password masked
    await click(usersButton("New user…"), "new user");
    await waitFor(() => usersView("new"), "new user form");
    const userNoPlan = usersDialog().querySelector("[data-ddl-preview-empty]")?.textContent.trim();
    await click(usersDialog().querySelector('[data-template="readOnly"]'), "read-only template");
    await type(field("User name"), "puricat_e2e_user");
    await setChoice(field("May connect from"), "localhost");
    await waitFor(() => [...field("Database").options].some((o) => o.value === "puricat_e2e"), "databases to choose from");
    // Only the count goes into the report: the other databases on a developer's server are theirs.
    const userDatabases = [...field("Database").options].map((o) => o.value).filter(Boolean);
    const systemOffered = userDatabases.filter((d) => ["mysql", "information_schema", "performance_schema", "sys"].includes(d));
    if (systemOffered.length > 0) throw new Error(`system databases are offered: ${systemOffered}`);
    await setChoice(field("Database"), "puricat_e2e");
    const userMasked = "CREATE USER 'puricat_e2e_user'@'localhost' IDENTIFIED BY '••••••••';\nGRANT SELECT, SHOW VIEW ON `puricat\\_e2e`.* TO 'puricat_e2e_user'@'localhost';";
    const userPlan = await waitFor(() => usersPlan().length === 1 && usersPlan()[0], "statements of the new user");
    if (userPlan !== userMasked) throw new Error(`statements of the new user: ${userPlan}`);
    // nothing is created without a password
    if (!usersButton("Create user").disabled) throw new Error("a user can be created without a password");
    // a generated password is shown, and then the statement holds it, as it will be written
    await click(usersButton("Generate"), "generate a password");
    const generated = field("Password").value;
    if (!/^[A-Za-z0-9]{24}$/.test(generated) || field("Password").type !== "text") throw new Error("the generated password is not 24 letters and digits in view");
    await waitFor(() => usersPlan()[0] === userMasked.replace("'••••••••'", `'${generated}'`), "the statement with the password");
    // hidden again, the mask is back
    await click(usersDialog().querySelector('button[aria-label="Hide the password"]'), "hide the password");
    await waitFor(() => usersPlan()[0] === userMasked && field("Password").type === "password", "the mask again");
    await waitFor(() => !usersButton("Create user").disabled, "create button");
    await stage("user-new", { userNoPlan, databasesOffered: userDatabases.length, userPlan });
    await click(usersButton("Create user"), "create user");
    // the one chance to copy the password, then the new account in the list, selected
    const kept = (await waitFor(() => usersDialog().querySelector("[data-users-done] [data-password]"), "the password to copy", 15000)).value;
    if (kept !== generated) throw new Error("the password shown after creating is not the one that was set");
    await click(usersButton("Done"), "done");
    await waitFor(() => usersView("browse") && accountRow("puricat_e2e_user@localhost")?.getAttribute("aria-selected") === "true", "the new account, selected");
    const userGrants = await waitFor(() => {
      const text = usersDialog().querySelector("[data-grants]")?.textContent;
      return text?.includes("puricat_e2e_user") && text;
    }, "grants of the new user");
    if (!userGrants.includes("GRANT SELECT, SHOW VIEW ON `puricat\\_e2e`.*")) throw new Error(`grants of the new user: ${userGrants}`);
    // the server has it so: reading that database, and nothing more in it
    const granted = await scratchCount("SELECT COUNT(*) FROM mysql.db WHERE User = 'puricat_e2e_user' AND Host = 'localhost' AND Db = 'puricat\\\\_e2e' AND Select_priv = 'Y' AND Show_view_priv = 'Y' AND Insert_priv = 'N' AND Delete_priv = 'N'");
    if (granted !== 1) throw new Error("the server does not hold the grant the form showed");

    // drop: only after the account was typed as it is written, host included
    await click(usersButton("Drop user…"), "drop user");
    const userDropPlan = await waitFor(() => usersView("drop") && usersDialog().querySelector("[data-ddl-preview]")?.textContent.trim(), "drop user statement");
    if (userDropPlan !== "DROP USER 'puricat_e2e_user'@'localhost';") throw new Error(`drop user statement: ${userDropPlan}`);
    const userConfirm = usersDialog().querySelector("[data-confirm-name]");
    const userDropStates = [usersButton("Drop user").disabled];
    await type(userConfirm, "puricat_e2e_user");
    userDropStates.push(usersButton("Drop user").disabled);
    await type(userConfirm, "puricat_e2e_user@localhost");
    await waitFor(() => !usersButton("Drop user").disabled, "drop user button once the account is typed");
    if (userDropStates.some((disabled) => !disabled)) throw new Error("the drop user button was enabled before the account was typed");
    await click(usersButton("Drop user"), "drop the user");
    await waitFor(() => usersView("browse") && !accountRow("puricat_e2e_user@localhost"), "the account gone from the list", 15000);
    if ((await scratchCount("SELECT COUNT(*) FROM mysql.user WHERE User = 'puricat_e2e_user'")) !== 0) throw new Error("the dropped user is still on the server");
    await click(usersDialog().querySelector('button[aria-label="Close"]'), "close users and privileges");
    await waitFor(() => !usersDialog(), "users dialog closed");
    log.push({ stage: "users-form", userGrants, userDropPlan });

    // ---- backups: a database's menu, a folder of the user's choice, the history, a schedule
    const backupDialog = () => document.querySelector("[data-backup-dialog]");
    const inBackup = (sel) => backupDialog()?.querySelector(sel);
    const backUp = async (connectionName, extension) => {
      await dbMenu(connectionName, "puricat_e2e", "Back up…");
      await waitFor(() => inBackup("[data-backup-tool]"), `the dump tool for ${connectionName}`, 20000);
      const tool = inBackup("[data-backup-tool]").textContent.replace(/\s+/g, " ").trim();
      // Nothing was chosen yet: the folder offered is in the user's Documents. It is only
      // created when a backup is made, and the test chooses another one first.
      const offered = inBackup("[data-backup-folder]").textContent.trim();
      if (!offered.endsWith("/Documents/PuriCat Backups")) throw new Error(`the folder offered is ${offered}`);
      await click(inBackup("[data-backup-choose]"), "choose the folder");
      await waitFor(() => inBackup("[data-backup-folder]").textContent.trim() === "__WORK__/backups", "the chosen folder");
      const preview = inBackup("[data-backup-file-name]").textContent.trim();
      const pattern = new RegExp(`^${connectionName.replace(" ", "-")}_puricat_e2e_\\d{4}-\\d\\d-\\d\\d_\\d{6}\\.${extension.replace(".", "\\.")}$`);
      if (!pattern.test(preview)) throw new Error(`the file name previewed is ${preview}`);
      if (all("[data-backup-run]").length !== 0) throw new Error("a database that was never backed up has a history");
      await click(inBackup("[data-backup-now]"), "back up now");
      const result = await waitFor(() => inBackup("[data-backup-result]"), "the backup to end", 60000);
      const said = result.textContent.replace(/\s+/g, " ").trim();
      if (result.dataset.backupResult !== "ok" || !said.includes("Backup saved")) throw new Error(`the backup of ${connectionName} ended with: ${said}`);
      const file = said.match(/\/[^ ]+_puricat_e2e_[\d_-]+\.[a-z.]+/)?.[0];
      if (!file?.startsWith("__WORK__/backups/") || !file.endsWith(`.${extension}`)) throw new Error(`the backup was saved as ${file}`);
      await waitFor(() => all("[data-backup-run]").length === 1 && inBackup('[data-backup-run="ok"]'), "the run in the history");
      return { tool, preview, file, history: inBackup("[data-backup-run]").textContent.replace(/\s+/g, " ").trim() };
    };
    const closeBackup = async () => {
      await click(inBackup('button[aria-label="Close"]'), "close the backup dialog");
      await waitFor(() => !backupDialog(), "the backup dialog to close");
    };

    // ---- restore: a backup of the history, into a database that is made for it
    const restoreDialog = () => document.querySelector("[data-restore-dialog]");
    const inRestore = (sel) => restoreDialog()?.querySelector(sel);
    const ordersIn = async (connectionName, database, schema) => {
      const id = (await invoke("list_connections")).find((c) => c.name === connectionName).id;
      return (await invoke("count_rows", { id, table: { database, schema, name: "orders" }, filters: [], job: crypto.randomUUID() })).count;
    };
    const restoreInto = async (connectionName, schema, loader, shot) => {
      await click(inBackup("[data-backup-restore]"), "restore…");
      const nameField = await waitFor(() => inRestore("[data-restore-name]"), "the restore dialog");
      // The name offered says where the data came from and when; it is never the source's own.
      const offered = nameField.value;
      if (!/^puricat_e2e_restored_\d{8}_\d{4}$/.test(offered)) throw new Error(`the name offered for the restored database is ${offered}`);
      // A database that exists is refused before anything runs: nothing is ever replaced.
      await type(nameField, "puricat_e2e");
      const refusal = await waitFor(() => {
        const said = inRestore("[data-restore-no-plan]")?.textContent ?? "";
        return said.includes("already exists") && said;
      }, "the refusal of a name that is taken", 20000);
      if (!inRestore("[data-restore-run]").disabled) throw new Error("Restore was enabled for a database that exists");
      await type(nameField, "puricat_e2e_restored");
      const plan = await waitFor(() => {
        const shown = inRestore("[data-restore-plan]");
        return shown?.getAttribute("aria-busy") === "false" && shown.textContent.includes("puricat_e2e_restored") && shown.textContent.replace(/\s+/g, " ").trim();
      }, "the plan of the restore", 20000);
      if (!plan.includes("CREATE DATABASE") || !plan.includes(`Then ${loader} `)) throw new Error(`the restore plan of ${connectionName} is: ${plan}`);
      if (shot) await stage("restore-plan", { offered, refusal, plan });
      await click(inRestore("[data-restore-run]"), "restore");
      const result = await waitFor(() => inRestore("[data-restore-result]"), "the restore to end", 60000);
      const said = result.textContent.replace(/\s+/g, " ").trim();
      if (result.dataset.restoreResult !== "ok" || !said.includes("Restored into “puricat_e2e_restored”")) throw new Error(`the restore on ${connectionName} ended with: ${said}`);
      // The new database is in the explorer, and holds as many orders as the source has now.
      await waitFor(() => dbRowOf(connectionName, "puricat_e2e_restored"), "the restored database in the explorer", 15000);
      const orders = [await ordersIn(connectionName, "puricat_e2e", schema), await ordersIn(connectionName, "puricat_e2e_restored", schema)];
      if (orders[0] !== 1250 || orders[1] !== orders[0]) throw new Error(`the restored database of ${connectionName} holds ${orders[1]} orders, the source ${orders[0]}`);
      if (shot) await stage("restore-done", { said });
      await click(inRestore("[data-restore-close]"), "close the restore dialog");
      await waitFor(() => !restoreDialog(), "the restore dialog to close");
      return { offered, refusal, plan, said, orders: orders[1] };
    };

    const mysqlBackup = await backUp("Demo MySQL", "sql.gz");
    await stage("backup-done", { mysqlBackup });
    const mysqlRestore = await restoreInto("Demo MySQL", null, "mysql", true);
    // A schedule: every day at 03:15, keeping three. A development build installs no
    // background runner, and the dialog says that the schedule will not run instead of
    // showing a next time.
    await click(exact('[data-backup-dialog] [role="radio"]', "Every day"), "every day");
    await type(await waitFor(() => inBackup("[data-backup-time]"), "the time field"), "03:15");
    await type(inBackup("[data-backup-keep]"), "3");
    const scheduleStatus = () => inBackup("[data-backup-schedule-status]").textContent.replace(/\s+/g, " ").trim();
    await waitFor(() => scheduleStatus() === "Not saved yet.", "the schedule shown as not saved");
    await click(inBackup("[data-backup-save]"), "save the schedule");
    await waitFor(() => scheduleStatus().includes("development build does not install the background runner"), "the schedule's status", 15000);
    const backupConnection = (await invoke("list_connections")).find((c) => c.name === "Demo MySQL").id;
    const savedJob = (await invoke("backup_plan", { id: backupConnection, database: "puricat_e2e" })).job;
    if (JSON.stringify(savedJob.schedule) !== '{"kind":"daily","hour":3,"minute":15}' || savedJob.keep !== 3 || savedJob.destination !== "__WORK__/backups")
      throw new Error(`the job was saved as ${JSON.stringify(savedJob)}`);
    await stage("backup-schedule", { status: scheduleStatus(), savedJob });
    await click(exact('[data-backup-dialog] [role="radio"]', "Off"), "schedule off");
    await click(await waitFor(() => exact("[data-backup-dialog] button", "Remove schedule"), "the remove button"), "remove the schedule");
    await waitFor(() => scheduleStatus() === "Backups are made only when you ask.", "the schedule removed", 15000);
    await closeBackup();

    // PostgreSQL: its own archive by default, into the folder chosen for this database.
    const pgBackup = await backUp("Demo PostgreSQL", "dump");
    const pgRestore = await restoreInto("Demo PostgreSQL", "shop", "pg_restore", false);
    await closeBackup();
    // Opened again, the dialog remembers the folder and shows the run.
    await dbMenu("Demo PostgreSQL", "puricat_e2e", "Back up…");
    await waitFor(() => inBackup("[data-backup-folder]")?.textContent.trim() === "__WORK__/backups" && all("[data-backup-run]").length === 1, "the remembered folder and the history", 20000);
    await closeBackup();
    log.push({ stage: "backups", mysqlBackup, pgBackup, mysqlRestore, pgRestore });


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

    // ---- filter rows: the PostgreSQL orders table is still in front ----------------------
    const filterBar = () => document.querySelector(`${panelSel} [data-filter-bar]`);
    const filterRows = () => all(`${panelSel} [data-filter-row]`);
    const filterCount = () => document.querySelector(`${panelSel} [data-filter-count]`)?.textContent.trim() ?? "0";
    const columnCells = (c) => dataRows().map((r) => r.querySelectorAll('[role="gridcell"]')[c]?.textContent.trim());
    const choose = async (select, value) => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(select, value);
      select.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(60);
    };
    const condition = async (row, column, op, value) => {
      await choose(filterRows()[row].querySelectorAll("select")[0], column);
      await choose(filterRows()[row].querySelectorAll("select")[1], op);
      await type(filterRows()[row].querySelector("input"), value);
    };
    const STATUS = 4;
    const TOTAL = 5;

    // 0. The exact number of rows is counted only when asked for. Until then the footer has
    //    the server's estimate, or nothing.
    const countButton = () => document.querySelector(`${panelSel} [data-count-rows]`);
    const rowCount = () => document.querySelector(`${panelSel} [data-row-count]`)?.textContent.replace(/\s+/g, " ").trim();
    const truth = async (sql) => {
      const id = (await invoke("list_connections")).find((c) => c.engine === "postgres").id;
      const outcome = await invoke("run_sql", { id, tab: "e2e-check-count", database: "puricat_e2e", sql, options: { maxRows: 10, changeLimit: null } });
      await invoke("close_console", { tab: "e2e-check-count" });
      return Number(outcome.statements[0].results[0].rows[0][0]);
    };
    const counted = { before: footer(), asked: !!rowCount() };
    await click(await waitFor(countButton, "the count button"), "count rows");
    await waitFor(() => rowCount()?.includes("1,250 in total"), "the exact count");
    counted.all = footer();
    // The same rows on another page: the count stays.
    await click(document.querySelector(`${panelSel} button[aria-label="Next page"]`), "next page");
    await waitFor(() => footer()?.includes("Rows 201–400"), "second page");
    counted.onPageTwo = rowCount();
    await click(document.querySelector(`${panelSel} button[aria-label="Previous page"]`), "previous page");
    await waitFor(() => footer()?.includes("Rows 1–200"), "first page again");
    if (counted.asked || !counted.onPageTwo?.includes("1,250")) throw new Error(`the exact count is not shown when it should be: ${JSON.stringify(counted)}`);

    await click(document.querySelector(`${panelSel} [data-filter-toggle]`), "filter button");
    await waitFor(filterBar, "filter bar");
    // A new condition starts on the first column; a number is offered comparisons.
    const offered = [...filterRows()[0].querySelectorAll("select")[1].options].map((o) => o.value);

    // 1. An enum column, matched as the text the grid shows whatever the case; sent with Enter.
    await condition(0, "status", "contains", "PEND");
    filterRows()[0].querySelector("input").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await waitFor(() => footer()?.includes("1 condition applied") && columnCells(STATUS).every((v) => v === "Pending"), "one condition");
    // Other rows now: the count of the whole table is gone, and the matches can be counted.
    counted.afterFilter = rowCount() ?? null;
    const pending = await truth("SELECT count(*) FROM shop.orders WHERE status::text ILIKE '%pend%'");
    if (pending > 200) {
      await click(await waitFor(countButton, "the count button, for the matches"), "count matches");
      await waitFor(() => rowCount()?.includes("match"), "the number of matches");
      counted.matches = rowCount();
      if (!counted.matches.includes(pending.toLocaleString("en-US"))) throw new Error(`the footer says ${counted.matches}, the database ${pending}`);
    } else if (countButton()) {
      throw new Error("a count is offered although every matching row is on the page");
    }
    if (counted.afterFilter) throw new Error(`the table's count outlived the filter: ${counted.afterFilter}`);
    counted.pending = pending;
    await stage("table-count", { counted, footer: footer() });
    const oneCondition = {
      footer: footer(),
      statuses: [...new Set(columnCells(STATUS))],
      headerMarks: all(`${panelSel} [role="columnheader"]`).filter((h) => h.querySelector('[aria-label="Filtered"]')).map((h) => h.textContent.trim()),
    };

    // 2. A second condition, on a number, sent with the button. Both must hold.
    await click(panelButton("Add condition"), "add condition");
    await condition(1, "total_amount", "greaterOrEqual", "9000");
    const note = document.querySelector(`${panelSel} [data-filter-note]`).textContent.trim();
    await click(panelButton("Apply"), "apply");
    await waitFor(() => filterCount() === "2" && columnCells(TOTAL).every((v) => Number(v) >= 9000), "two conditions");
    const twoConditions = { note, footer: footer(), rows: dataRows().length, statuses: [...new Set(columnCells(STATUS))], smallest: Math.min(...columnCells(TOTAL).map(Number)) };
    if (twoConditions.rows !== 4) throw new Error(`the two conditions should leave 4 rows, not ${twoConditions.rows}`);
    await stage("table-filter", { offered, oneCondition, twoConditions });

    // 3. Nothing matches: the grid says so, and offers the way back.
    await condition(0, "order_no", "equals", "SO-NOPE");
    await click(panelButton("Apply"), "apply");
    const emptyState = await waitFor(() => document.querySelector(`${panelSel} [data-filter-empty]`), "no-match state");
    await stage("table-filter-empty", { text: emptyState.textContent.replace(/\s+/g, " ").trim(), footer: footer() });
    await click(emptyState.querySelector("button"), "clear from the empty state");
    await waitFor(() => filterCount() === "0" && dataRows().length > 20 && !footer().includes("condition"), "filter cleared");
    const barReset = filterRows().length === 1 && filterRows()[0].querySelector("input").value === "";

    // 4. A value the column's type cannot hold: the server's refusal is shown, not an empty grid.
    await condition(0, "total_amount", "equals", "abc");
    await click(panelButton("Apply"), "apply");
    await waitFor(() => activePanel().textContent.includes("Could not load the table"), "filter error");
    const filterRefusal = all(`${panelSel} *`).find((e) => e.children.length === 0 && e.textContent.includes("invalid input syntax"))?.textContent.trim();
    if (!filterRefusal) throw new Error("the server's refusal of the filter value is not shown");
    await click(all(`${panelSel} button`).filter((b) => b.textContent.trim() === "Clear filter").pop(), "clear after the error");
    await waitFor(() => filterCount() === "0" && dataRows().length > 20, "rows after the error");

    // 5. From a cell: "Filter by this value" adds the condition and shows it in the bar.
    await selectCell(0, STATUS);
    await rightClick(cellAt(0, STATUS));
    await click(await waitFor(() => menuItem("Filter by this value"), "cell menu"), "filter by this value");
    await waitFor(() => filterCount() === "1" && new Set(columnCells(STATUS)).size === 1, "filter from a cell");
    const fromCell = {
      column: filterRows()[0].querySelectorAll("select")[0].value,
      op: filterRows()[0].querySelectorAll("select")[1].value,
      value: filterRows()[0].querySelector("input").value,
      statuses: [...new Set(columnCells(STATUS))],
      footer: footer(),
    };
    await stage("table-filter-cell", { barReset, filterRefusal, fromCell });
    await click(panelButton("Clear filter"), "clear filter");
    await waitFor(() => filterCount() === "0" && new Set(columnCells(STATUS)).size > 1, "every row again");

    // 6. Any of the conditions instead of all, a list pasted as lines, a range, a condition
    //    switched off. The numbers follow from the fixture: 16 orders of 9,000 or more
    //    (SO-10113 is one), 18 between 100 and 250 (SO-10001 is one, SO-10002 is not).
    const matchChoice = (label) => all(`${panelSel} [data-filter-match] [role="radio"]`).find((b) => b.textContent.trim() === label);
    const rowsShown = (n) => new RegExp(`Rows 1–${n}(?!\\d)`).test(footer() ?? "");
    const orderNos = () => columnCells(1);
    await choose(filterRows()[0].querySelectorAll("select")[0], "order_no");
    await choose(filterRows()[0].querySelectorAll("select")[1], "in");
    const pastedLines = new DataTransfer();
    pastedLines.setData("text/plain", "SO-10113\r\nSO-10001\r\nSO-10002\r\n");
    filterRows()[0].querySelector("input").dispatchEvent(new ClipboardEvent("paste", { clipboardData: pastedLines, bubbles: true, cancelable: true }));
    const listBox = await waitFor(() => filterRows()[0].querySelector("[data-filter-values]") && filterRows()[0].querySelector("input").value, "the pasted list in the box");
    const listHint = filterRows()[0].querySelector("[data-filter-values]").textContent.trim();
    if (listBox !== "SO-10113, SO-10001, SO-10002" || listHint !== "3 values") throw new Error(`pasted lines became “${listBox}” (${listHint})`);
    if (matchChoice("any")) throw new Error("all / any is offered with one condition");
    await click(panelButton("Add condition"), "add condition");
    await condition(1, "total_amount", "greaterOrEqual", "9000");
    await click(panelButton("Apply"), "apply");
    await waitFor(() => filterCount() === "2" && dataRows().length === 1 && orderNos()[0] === "SO-10113", "both conditions: one order");
    const allOf = { footer: footer(), label: filterRows()[1].querySelector(":scope > span").textContent.trim() };
    await click(matchChoice("any"), "match any");
    const anyNote = document.querySelector(`${panelSel} [data-filter-note]`).textContent.trim();
    await click(panelButton("Apply"), "apply");
    await waitFor(() => rowsShown(18) && footer().includes("any of 2 conditions applied"), "either condition: 18 orders");
    const anyOf = { note: anyNote, footer: footer(), label: filterRows()[1].querySelector(":scope > span").textContent.trim() };
    const eitherTruth = await truth("SELECT count(*) FROM shop.orders WHERE order_no IN ('SO-10113', 'SO-10001', 'SO-10002') OR total_amount >= 9000");
    if (eitherTruth !== 18 || allOf.label !== "and" || anyOf.label !== "or") throw new Error(`any of the conditions: ${JSON.stringify({ eitherTruth, allOf, anyOf })}`);

    // a range, both ends included
    await choose(filterRows()[1].querySelectorAll("select")[1], "between");
    await type(filterRows()[1].querySelectorAll("input")[0], "100");
    await type(filterRows()[1].querySelectorAll("input")[1], "250");
    await click(panelButton("Apply"), "apply");
    await waitFor(() => rowsShown(20), "the list or the range: 20 orders");
    const rangeTruth = await truth("SELECT count(*) FROM shop.orders WHERE order_no IN ('SO-10113', 'SO-10001', 'SO-10002') OR total_amount BETWEEN 100 AND 250");
    const outside = columnCells(TOTAL).map(Number).filter((v) => v < 100 || v > 250).length; // SO-10113 and SO-10002, from the list
    if (rangeTruth !== 20 || outside !== 2) throw new Error(`the range: the database says ${rangeTruth}, and ${outside} rows are outside it`);

    // a third condition, then switched off: it stays in the bar and is not sent
    await click(panelButton("Add condition"), "add condition");
    await condition(2, "status", "equals", "Cancelled");
    await click(panelButton("Apply"), "apply");
    await waitFor(() => filterCount() === "3" && rowsShown(200), "three conditions, any of them");
    await click(filterRows()[2].querySelector('[role="checkbox"]'), "switch the third condition off");
    const offNote = document.querySelector(`${panelSel} [data-filter-note]`).textContent.trim();
    await click(panelButton("Apply"), "apply");
    await waitFor(() => filterCount() === "2" && rowsShown(20), "the condition switched off");
    const switchedOff = {
      note: offNote,
      rows: filterRows().length,
      off: filterRows().map((r) => r.querySelector('[role="checkbox"]').getAttribute("aria-checked")),
      footer: footer(),
    };
    if (switchedOff.rows !== 3 || switchedOff.off.join() !== "true,true,false") throw new Error(`switching a condition off: ${JSON.stringify(switchedOff)}`);
    await stage("table-filter-any", { listBox, listHint, allOf, anyOf, switchedOff });
    await click(panelButton("Clear filter"), "clear filter");
    await waitFor(() => filterCount() === "0" && filterRows().length === 1 && rowsShown(200), "every row again");

    // 7. From the grid: hide the rows that hold a cell's value; show those that hold any of several.
    const hidden = cellText(0, STATUS).trim();
    await selectCell(0, STATUS);
    await rightClick(cellAt(0, STATUS));
    const cellMenu = (await waitFor(() => menuItem("Hide rows with this value") && all(`[role="menu"]${OPEN} [role="menuitem"]`), "cell menu")).map((e) => e.textContent.replace(/\s+/g, " ").trim());
    await click(menuItem("Hide rows with this value"), "hide rows with this value");
    await waitFor(() => filterCount() === "1" && !columnCells(STATUS).includes(hidden), "rows with the value hidden");
    const hideValue = {
      hidden,
      op: filterRows()[0].querySelectorAll("select")[1].value,
      value: filterRows()[0].querySelector("input").value,
      statuses: [...new Set(columnCells(STATUS))].sort(),
    };
    if (hideValue.op !== "notEquals" || hideValue.value !== hidden || hideValue.statuses.length !== 3) throw new Error(`hide rows with this value: ${JSON.stringify(hideValue)}`);
    await click(panelButton("Clear filter"), "clear filter");
    await waitFor(() => filterCount() === "0" && rowsShown(200), "every row again");
    await selectCell(0, STATUS);
    await gridKey("ArrowDown", { shiftKey: true });
    await gridKey("ArrowDown", { shiftKey: true });
    const picked = [0, 1, 2].map((r) => cellText(r, STATUS).trim());
    await rightClick(cellAt(1, STATUS));
    await click(await waitFor(() => menuItem(`Show rows with these ${new Set(picked).size} values`), "show rows with these values"), "show rows with these values");
    await waitFor(() => filterCount() === "1" && new Set(columnCells(STATUS)).size === new Set(picked).size, "rows with any of the values");
    const theseValues = {
      picked,
      op: filterRows()[0].querySelectorAll("select")[1].value,
      list: filterRows()[0].querySelector("input").value,
      statuses: [...new Set(columnCells(STATUS))].sort(),
    };
    if (theseValues.op !== "in" || theseValues.list !== picked.join(", ") || theseValues.statuses.join() !== [...new Set(picked)].sort().join()) {
      throw new Error(`show rows with these values: ${JSON.stringify(theseValues)}`);
    }
    log.push({ stage: "table-filter-grid", cellMenu, hideValue, theseValues, rangeTruth });
    await click(panelButton("Clear filter"), "clear filter");
    await waitFor(() => filterCount() === "0" && new Set(columnCells(STATUS)).size > 3, "every row again");
    await click(document.querySelector(`${panelSel} [aria-label="Hide the filter bar"]`), "hide the filter bar");
    await waitFor(() => !filterBar(), "filter bar hidden");

    // ---- a JSON value opens laid out for reading (PostgreSQL orders, a jsonb column) -------
    const META = 6;
    const openCell = '[data-cell-dialog][data-state="open"]';
    const metaRow = dataRows().findIndex((_, r) => cellText(r, META)?.trim().startsWith("{"));
    const storedJson = cellText(metaRow, META).trim();
    await selectCell(metaRow, META);
    await rightClick(cellAt(metaRow, META));
    await click(await waitFor(() => menuItem("View whole value"), "cell menu"), "view the json value");
    const laidOut = (await waitFor(() => document.querySelector(`${openCell} [data-cell-formatted]`), "json laid out")).textContent;
    const jsonViews = all(`${openCell} [role="radio"]`).map((r) => `${r.getAttribute("aria-checked") === "true" ? "●" : "○"} ${r.textContent.trim()}`);
    await stage("cell-json", { storedJson, laidOut, jsonViews });
    await click(all(`${openCell} [role="radio"]`).find((r) => r.textContent.trim() === "As stored"), "as stored");
    const asStored = (await waitFor(() => document.querySelector(`${openCell} [data-cell-value]`), "the stored text")).value;
    // Only the spacing differs, and the text to edit is the stored one.
    if (!/^\{\n  "channel": "(web|line)"\n\}$/.test(laidOut) || laidOut.replace(/\s+/g, "") !== storedJson.replace(/\s+/g, "") || asStored !== storedJson || jsonViews.join() !== "● Formatted,○ As stored") {
      throw new Error(`the json view is wrong: ${JSON.stringify({ storedJson, laidOut, asStored, jsonViews })}`);
    }
    await click(all(`${openCell} button`).find((b) => b.textContent.trim() === "Cancel"), "close the json value");
    await waitFor(() => all("[data-cell-dialog]").every((d) => d.dataset.state === "closed"), "json value closed");

    // ---- one cell, whole: a note longer than a page shows (MySQL orders) ----------------
    // The row is found with the filter, which this also exercises on MySQL.
    await click(all('[role="tablist"] [role="tab"]').find((x) => x.textContent.includes("orders") && x.textContent.includes("Demo MySQL")), "mysql orders tab");
    await click(await waitFor(() => editState() === "editable" && document.querySelector(`${panelSel} [data-filter-toggle]`), "mysql orders tab"), "filter button");
    await waitFor(filterBar, "filter bar on mysql");
    await condition(0, "id", "equals", "3");
    await click(panelButton("Apply"), "apply");
    await waitFor(() => dataRows().length === 1 && cellText(0, 1) === "SO-10003", "order 3 alone");
    const NOTE = 6;
    const cellDialog = () => document.querySelector("[data-cell-dialog]");
    const wholeBox = () => waitFor(() => cellDialog()?.querySelector("[data-cell-value]"), "the whole value", 15000);
    const shownInGrid = cellText(0, NOTE).length;
    // A double-click on a shortened cell opens the dialog; the value is fetched by the row's key.
    mouse(cellAt(0, NOTE), "dblclick");
    let box = await wholeBox();
    const whole = box.value;
    if (whole.length !== 2502 || !whole.startsWith("บันทึกยาว ") || !whole.endsWith("จบ")) throw new Error(`the whole note should be 2502 characters, got ${whole.length}`);
    const wholeCell = {
      shownInGrid,
      where: cellDialog().querySelector("[data-cell-where]").textContent.trim(),
      size: cellDialog().querySelector("[data-cell-size]").textContent.trim(),
      applyDisabledUntilChanged: cellDialog().querySelector("[data-cell-apply]").disabled,
    };
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set.call(box, `${whole} (ตรวจแล้ว)`);
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(100);
    await stage("cell-whole", { wholeCell });
    // The edit is staged like any other, with the complete original for the concurrency check.
    await click(cellDialog().querySelector("[data-cell-apply]"), "apply the edit");
    await waitFor(() => !cellDialog() && cellAt(0, NOTE)?.dataset.edited === "true", "edit staged");
    const stagedSummary = dirtySummary();
    await click(panelButton("Save"), "save the long value");
    await waitFor(() => !document.querySelector(`${panelSel} [data-dirty-bar]`) && cellAt(0, NOTE) && cellAt(0, NOTE).dataset.edited !== "true", "long value saved", 15000);
    // Read it back through the cell's menu.
    await selectCell(0, NOTE);
    await rightClick(cellAt(0, NOTE));
    await click(await waitFor(() => menuItem("View whole value"), "cell menu"), "view whole value");
    box = await wholeBox();
    const savedBack = { length: box.value.length, tail: box.value.slice(-14) };
    if (box.value !== `${whole} (ตรวจแล้ว)`) throw new Error(`the saved note is not what was typed: ${savedBack.length} characters ending ${savedBack.tail}`);
    await click(all("[data-cell-dialog] button").find((b) => b.textContent.trim() === "Cancel"), "close the cell dialog");
    await waitFor(() => !cellDialog(), "cell dialog closed");
    log.push({ stage: "cell-whole-saved", stagedSummary, savedBack });
    await click(panelButton("Clear filter"), "clear the mysql filter");
    await waitFor(() => filterCount() === "0" && dataRows().length > 20, "mysql rows again");
    await click(document.querySelector(`${panelSel} [aria-label="Hide the filter bar"]`), "hide the filter bar");
    await waitFor(() => !filterBar(), "filter bar hidden");

    await click(treeRow("customers"), "mysql customers");
    await waitFor(() => cellText(0, 1) === "สมชาย ใจดี", "customers rows");
    if (editState() !== "editable") throw new Error("customers should be editable");

    // 0. Escape leaves the cell as it was: nothing is staged
    const cityBefore = cellText(0, 2);
    mouse(cellAt(0, 2), "dblclick");
    const discarded = await waitFor(editor, "cell editor");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(discarded, "typed, then given up");
    discarded.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(60);
    editor().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await waitFor(() => !editor(), "editor closed by Escape");
    await sleep(150);
    if (cellText(0, 2) !== cityBefore || cellAt(0, 2).dataset.edited === "true" || dirtySummary()) {
      throw new Error(`Escape kept the edit: ${cellText(0, 2)} / ${dirtySummary()}`);
    }

    // 1. double-click a cell and type; one undo takes it back, one redo restores it
    mouse(cellAt(0, 2), "dblclick");
    await typeInEditor("เชียงราย");
    await waitFor(() => cellAt(0, 2)?.dataset.edited === "true" && cellText(0, 2) === "เชียงราย", "staged edit");
    const afterTyping = dirtySummary();
    await gridKey("z", { metaKey: true });
    await waitFor(() => cellText(0, 2) === cityBefore && !dirtySummary(), "one undo for one typed edit");
    await gridKey("z", { metaKey: true, shiftKey: true });
    await waitFor(() => cellText(0, 2) === "เชียงราย" && dirtySummary() === afterTyping, "redo of the typed edit");

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

    // Rows with problems can be left out: the others run, the ones with problems change nothing.
    await invoke("plugin:clipboard-manager|write_text", {
      text:
        "order_no\tStatus\ttotal_amount\tcustomer_id\torder_date\r\n" +
        "SO-99003\tPending\t20\t1\t2026-11-17 09:00:00\r\n" + // new
        "SO-99004\tPending\tlots\t1\t2026-11-17 09:05:00\r\n" + // not a number
        "SO-99001\tCompleted\t10\t1\t2026-11-16 09:00:00\r\n" + // exists: its status changes
        "SO-99005\tPending\t5\t1\t2026-11-17 09:10:00\r\n" + // the same key twice
        "SO-99005\tPending\t6\t1\t2026-11-17 09:15:00\r\n",
    });
    await click(panelButton("Smart Paste…"), "open smart paste again");
    await waitFor(() => document.querySelector("[data-smart-source]"), "clipboard parsed");
    await click(smartButton("Preview"), "preview");
    const problemBox = await waitFor(() => document.querySelector("[data-smart-problems]"), "rows with problems", 15000);
    const smartCheck = (name) => document.querySelector(`[data-smart-check="${name}"]`);
    const listedStatuses = () => all("[data-smart-preview] tbody tr").map((tr) => tr.dataset.smartRow).join(",");
    const skipOffer = {
      counts: smartCounts(),
      box: problemBox.textContent.replace(/\s+/g, " ").trim(),
      runDisabled: smartButton("Run").disabled,
      listed: listedStatuses(),
      // The stored total is 10.00 and the pasted one is 10: the same number, so not a change.
      update: smartRows().find((r) => r.includes("SO-99001")),
    };
    await click(smartCheck("only-problems"), "list only the rows with problems");
    await waitFor(() => listedStatuses() === "invalid,duplicate,duplicate", "only the rows with problems");
    await click(document.querySelector("[data-smart-copy-problems]"), "copy the rows with problems");
    await sleep(300);
    skipOffer.copied = (await invoke("plugin:clipboard-manager|read_text")).split("\n");
    await click(smartCheck("only-problems"), "list every row again");
    await click(smartCheck("skip"), "leave the rows with problems out");
    await waitFor(() => !smartButton("Run").disabled, "run enabled");
    skipOffer.run = smartButton("Run").textContent.replace(/\s+/g, " ").trim();
    skipOffer.countLabel = document.querySelector('[data-smart-count="problems"]').textContent.replace(/\s+/g, " ").trim();
    if (
      JSON.stringify(skipOffer.counts) !== JSON.stringify({ insert: 1, update: 1, unchanged: 0, unmatched: 0, problems: 3 }) ||
      !skipOffer.runDisabled ||
      skipOffer.listed !== "insert,invalid,update,duplicate,duplicate" ||
      !skipOffer.update?.endsWith("| changes status") ||
      skipOffer.copied.length !== 4 ||
      skipOffer.copied[0] !== "order_no\tStatus\ttotal_amount\tcustomer_id\torder_date\tProblem" ||
      !skipOffer.copied[1].startsWith("SO-99004\tPending\tlots\t") ||
      !skipOffer.copied[1].includes("not a number") ||
      !skipOffer.copied[3].startsWith("SO-99005\tPending\t6\t") ||
      // each of the two rows names the other by its line in the pasted block (the header is line 1)
      !skipOffer.copied[2].endsWith("Same key as another pasted row (row 6)") ||
      !skipOffer.copied[3].endsWith("Same key as another pasted row (row 5)") ||
      !skipOffer.run.endsWith("2")
    ) {
      throw new Error(`the offer to leave rows out is wrong: ${JSON.stringify(skipOffer)}`);
    }
    await stage("smart-paste-skip", { skipOffer });
    await click(smartButton("Run"), "run without the rows with problems");
    await waitFor(() => !smart(), "smart paste finished", 20000);
    const skipToast = (await waitFor(() => withText("[data-sonner-toast]", "left out"), "what was left out")).textContent.replace(/\s+/g, " ").trim();
    const skipCheck = await invoke("run_sql", {
      id: (await invoke("list_connections")).find((c) => c.engine === "mysql").id,
      tab: "e2e-check-skip",
      database: "puricat_e2e",
      sql: "SELECT order_no, status, total_amount FROM orders WHERE order_no IN ('SO-99001', 'SO-99003', 'SO-99004', 'SO-99005') ORDER BY order_no",
      options: { maxRows: 10, changeLimit: null },
    });
    await invoke("close_console", { tab: "e2e-check-skip" });
    const afterSkip = skipCheck.statements[0].results[0].rows.map((r) => r.join(" "));
    if (afterSkip.join(" | ") !== "SO-99001 Completed 10.00 | SO-99003 Pending 20.00") throw new Error(`leaving rows out wrote the wrong rows: ${afterSkip.join(" | ")}`);
    log.push({ stage: "smart-paste-skipped", skipToast, afterSkip });

    // 8c. SQL console (MySQL): run a script, confirmation for a destructive statement,
    //     the impact guard, an error, the history, and exports
    // The "New query" button at the end of the tab strip opens on the connection and the
    // database of the tab in front.
    const frontTab = () => document.querySelector('[role="tablist"] [role="tab"][aria-selected="true"]');
    const frontBefore = frontTab().title;
    await click(document.querySelector("[data-new-query]"), "new query button");
    const viaButton = (await waitFor(() => frontTab()?.title.startsWith("Query 1 — ") && frontTab(), "query tab from the button")).title;
    if (viaButton !== "Query 1 — Demo MySQL › puricat_e2e") throw new Error(`the new query button opened “${viaButton}” from “${frontBefore}”`);
    await click(frontTab().querySelector("button"), "close the button's query tab");
    await waitFor(() => !all('[role="tab"]').some((t) => t.title.startsWith("Query 1")), "that tab to close");
    // With two connections open and the welcome tab in front, it asks which one.
    await click(all('[role="tab"]')[0], "welcome tab");
    document.querySelector("[data-new-query]").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
    await waitFor(() => menuItemWith("New query on "), "the connections under the new query button");
    const asked = all(`[role="menu"]${OPEN} [role="menuitem"]`).map((e) => e.textContent.replace(/\s+/g, " ").trim());
    if (asked.length !== 2 || !asked.some((x) => x.endsWith("New query on Demo MySQL")) || !asked.some((x) => x.endsWith("New query on Demo PostgreSQL")))
      throw new Error(`the new query button offered ${JSON.stringify(asked)}`);
    await stage("new-query-menu", { frontBefore, viaButton, asked });
    // ⌘T cannot ask: with the menu closed again it says where to choose, and opens nothing.
    await key("Escape");
    await waitFor(() => !menuItemWith("New query on "), "the menu to close");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "t", metaKey: true, bubbles: true }));
    const shortcutToast = (await waitFor(() => withText("[data-sonner-toast]", "Several connections are open"), "what ⌘T says with two connections")).textContent.replace(/\s+/g, " ").trim();
    if (all('[role="tab"]').some((t) => t.title.startsWith("Query "))) throw new Error("⌘T opened a query tab without a connection to open it on");
    document.querySelector("[data-new-query]").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
    await waitFor(() => menuItemWith("New query on "), "the menu again");
    await click(menuItemWith("New query on Demo PostgreSQL"), "new query on postgres");
    const viaMenu = (await waitFor(() => frontTab()?.title.startsWith("Query 1 — ") && frontTab(), "query tab from the menu")).title;
    if (viaMenu !== "Query 1 — Demo PostgreSQL") throw new Error(`the menu opened “${viaMenu}”`);
    await click(frontTab().querySelector("button"), "close the menu's query tab");
    await waitFor(() => !all('[role="tab"]').some((t) => t.title.startsWith("Query 1")), "that tab to close");
    await click(all('[role="tab"]').find((t) => t.title === frontBefore), "back to the tab that was in front");
    await waitFor(() => frontTab()?.title === frontBefore, "that tab in front again");
    log.push({ stage: "new-query-button", frontBefore, viaButton, asked, shortcutToast, viaMenu });

    await rightClick(treeRow("Demo MySQL"));
    await click(await waitFor(() => menuItemWith("New query"), "new query menu"), "new query");
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
      // Closing counts: the exit animation does not always run when the window is covered.
      return !menu || menu.dataset.state === "closed";
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

    // 8d. SQL console: the statement at the cursor, and saved scripts
    const sqlEditor = () => sqlTab().__editor;
    const sqlTitle = () => document.querySelector(`${panelSel} [data-sql-title]`)?.textContent.trim();
    const sqlSummary = () => document.querySelector(`${panelSel} [data-sql-summary]`)?.textContent.trim() ?? "";
    const sqlEdited = () => !!document.querySelector(`${panelSel} [data-sql-edited]`);
    const tabStrip = () => all('[role="tablist"] [role="tab"]');
    const tabNamed = (name) => tabStrip().find((x) => x.querySelector(".truncate")?.textContent.trim() === name);
    // The editor's lines are 21px: which of them the margin marks as the statement at the cursor.
    const markedLines = () =>
      all(`${panelSel} .sql-current-statement`)
        .map((e) => Math.round(parseFloat(e.parentElement.style.top) / 21) + 1)
        .sort((a, b) => a - b)
        .join(",");
    const cursorTo = (lineNumber, column) => sqlEditor().setPosition({ lineNumber, column });
    // A key pressed in the editor itself, so that its own key bindings are what answers.
    const editorKey = (init) =>
      sqlTab().querySelector("textarea, .native-edit-context").dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
    const closed = (sel) => all(sel).every((el) => el.dataset.state === "closed");
    const newQueryTab = async (title) => {
      await rightClick(treeRow("Demo MySQL"));
      await click(await waitFor(() => menuItemWith("New query"), "new query menu"), "new query");
      await waitFor(() => sqlTitle() === title && sqlTab()?.__editor, `query tab ${title}`, 30000);
    };
    const scriptsOf = JSON.parse((await invoke("get_settings")).workspace).tabs.find((x) => x.kind === "sql").connectionId;
    const storedScripts = async () => (await invoke("list_scripts", { id: scriptsOf })).map((x) => ({ name: x.name, sql: x.sql }));

    // Three statements; the second has a comment above it and one after its semicolon.
    sqlEditor().setValue("SELECT 1 AS one;\n\n-- สอง\nSELECT 2 AS two; -- จบ\nSELECT 3\n  AS three");
    const marked = {};
    for (const [name, line, column] of [["first", 1, 4], ["emptyLineAbove", 2, 1], ["inside", 4, 10], ["afterSemicolon", 4, 24], ["lastLine", 6, 3]]) {
      cursorTo(line, column);
      await sleep(350); // the script is analysed a moment after it changes
      marked[name] = markedLines();
    }
    if (JSON.stringify(marked) !== JSON.stringify({ first: "1", emptyLineAbove: "4", inside: "4", afterSemicolon: "4", lastLine: "5,6" })) {
      throw new Error(`the margin marks the wrong statement: ${JSON.stringify(marked)}`);
    }
    // From the menu beside Run: only the statement the cursor is in runs.
    cursorTo(4, 24);
    document.querySelector(`${panelSel} [data-sql-run-menu]`).dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
    const runMenu = (await waitFor(() => document.querySelector("[data-sql-run-current]"), "run menu")).parentElement.textContent.replace(/\s+/g, " ").trim();
    await click(document.querySelector("[data-sql-run-current]"), "run the statement at the cursor");
    await waitFor(() => gridCells().join() === "2", "the second statement only", 20000);
    const ranFromMenu = { summary: sqlSummary().replace(/ · .*/, ""), cells: gridCells().join(), column: document.querySelector(`${panelSel} [role="columnheader"] button`)?.textContent.trim() };
    // From the keyboard, with other text selected: ⇧⌘↵ still runs the statement at the cursor.
    sqlEditor().setSelection({ startLineNumber: 1, startColumn: 1, endLineNumber: 6, endColumn: 3 });
    editorKey({ key: "Enter", code: "Enter", keyCode: 13, metaKey: true, shiftKey: true });
    await waitFor(() => gridCells().join() === "3", "the third statement only", 20000);
    const ranFromKeyboard = { summary: sqlSummary().replace(/ · .*/, ""), cells: gridCells().join() };
    await showMessages();
    ranFromKeyboard.messages = messages().map((m) => m.text.slice(0, 40));
    // The comment after the second statement's semicolon is not sent with the third.
    if (ranFromKeyboard.messages.length !== 1 || !ranFromKeyboard.messages[0].startsWith("SELECT 3 AS three")) {
      throw new Error(`⇧⌘↵ ran something else: ${JSON.stringify(ranFromKeyboard)}`);
    }
    cursorTo(4, 10);
    await waitFor(() => markedLines() === "4", "the mark follows the cursor");
    await stage("sql-run-current", { marked, runMenu, ranFromMenu, ranFromKeyboard });

    // A second query tab is saved under a name (⌘S asks for one) and becomes that script.
    const sqlTabsBefore = tabStrip().length;
    await newQueryTab("Query 2");
    const scriptName = "Orders by status ยอดตามสถานะ";
    const scriptText = "-- ยอดรวมต่อสถานะ\nSELECT status, COUNT(*) AS n\nFROM puricat_e2e.orders\nGROUP BY status ORDER BY status";
    const saveButton = () => document.querySelector(`${panelSel} [data-sql-save]`);
    const nothingToSave = saveButton().disabled;
    sqlEditor().setValue(scriptText);
    await waitFor(() => !saveButton().disabled, "save button enabled");
    editorKey({ key: "s", code: "KeyS", keyCode: 83, metaKey: true });
    const nameBox = await waitFor(() => document.querySelector("[data-sql-scripts][data-state=open] [data-script-name] input"), "name of the new script");
    await type(nameBox, `  ${scriptName} `);
    await stage("sql-script-name", { nothingToSave });
    await click(document.querySelector("[data-script-create]"), "save the new script");
    await waitFor(() => sqlTitle() === scriptName && closed("[data-sql-scripts]"), "tab named after its script");
    const savedNew = {
      tab: frontTab().querySelector(".truncate").textContent.trim(),
      edited: sqlEdited(),
      saveDisabled: saveButton().disabled,
      stored: await storedScripts(),
      scriptId: typeof JSON.parse((await invoke("get_settings")).workspace).tabs.find((x) => x.title === scriptName)?.scriptId,
    };
    if (savedNew.stored.length !== 1 || savedNew.stored[0].name !== scriptName || savedNew.stored[0].sql !== scriptText || savedNew.edited || !savedNew.saveDisabled) {
      throw new Error(`the script was not saved as written: ${JSON.stringify(savedNew)}`);
    }

    // Changing the text marks the tab; closing it asks; ⌘S writes the change.
    sqlEditor().setValue(`${scriptText}\n-- แก้ครั้งที่ 1`);
    await waitFor(() => sqlEdited() && frontTab().querySelector('[role="img"]'), "edited marks");
    const editedMarks = { header: document.querySelector(`${panelSel} [data-sql-edited]`).textContent.trim(), tab: frontTab().querySelector('[role="img"]').getAttribute("aria-label") };
    await click(frontTab().querySelector("button"), "close the edited script");
    const closeDialog = await waitFor(() => document.querySelector("[data-close-dialog]"), "close dialog");
    const closeAsk = { text: closeDialog.textContent.replace(/\s+/g, " ").trim(), buttons: [...closeDialog.querySelectorAll("button")].map((b) => b.textContent.trim()) };
    await stage("sql-script-close", { editedMarks, closeAsk });
    await click(exact("[data-close-dialog] button", "Cancel"), "cancel closing");
    await waitFor(() => !document.querySelector("[data-close-dialog]") && sqlTitle() === scriptName && sqlEdited(), "the tab stayed, still edited");
    editorKey({ key: "s", code: "KeyS", keyCode: 83, metaKey: true });
    await waitFor(() => !sqlEdited(), "saved from the keyboard");
    const savedByKey = (await storedScripts())[0].sql.endsWith("-- แก้ครั้งที่ 1");

    // From a third, blank tab: the script is already open, so its tab comes to the front.
    await newQueryTab("Query 2");
    const scriptsToggle = () => document.querySelector(`${panelSel} [data-sql-scripts-toggle]`);
    const scriptRows = () => all("[data-sql-scripts][data-state=open] [data-script-row]");
    const openScripts = async () => {
      await click(scriptsToggle(), "saved scripts");
      return waitFor(() => scriptRows()[0], "saved scripts list");
    };
    const closeScripts = () =>
      waitFor(() => {
        document.querySelector("[data-sql-scripts][data-state=open]")?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        return closed("[data-sql-scripts]");
      }, "saved scripts closed");
    await openScripts();
    const listedElsewhere = scriptRows().map((r) => r.textContent.replace(/\s+/g, " ").trim());
    await click(scriptRows()[0].querySelector("button"), "open the script");
    await waitFor(() => sqlTitle() === scriptName && closed("[data-sql-scripts]"), "the tab that has the script");
    const reopened = { tabs: tabStrip().length - sqlTabsBefore, value: sqlEditor().getValue().endsWith("-- แก้ครั้งที่ 1") };

    // A name is used once per connection, whatever its case; then the script is renamed.
    await openScripts();
    const listedHere = scriptRows().map((r) => r.textContent.replace(/\s+/g, " ").trim());
    await click(exact("[data-sql-scripts][data-state=open] button", "Save as a new script…"), "save as");
    const copyBox = await waitFor(() => document.querySelector("[data-sql-scripts][data-state=open] [data-script-name] input"), "name box");
    await waitFor(() => copyBox.value !== "", "a suggested name");
    const suggestedName = copyBox.value;
    await type(copyBox, scriptName.toUpperCase());
    await click(document.querySelector("[data-script-create]"), "save under a taken name");
    const taken = (await waitFor(() => document.querySelector("[data-sql-scripts][data-state=open] [data-script-problem]"), "name taken")).textContent.trim();
    await stage("sql-scripts", { listedElsewhere, listedHere, suggestedName, taken });
    await click(exact("[data-sql-scripts][data-state=open] [data-script-name] button", "Cancel"), "cancel the name");
    await click(scriptRows()[0].querySelector('button[aria-label="Rename"]'), "rename");
    await type(await waitFor(() => document.querySelector('[data-sql-scripts][data-state=open] input[aria-label="Name of the script"]'), "rename box"), "Status totals");
    await click(exact("[data-sql-scripts][data-state=open] button", "Rename"), "confirm rename");
    await waitFor(() => sqlTitle() === "Status totals" && tabNamed("Status totals"), "the tab follows its script's name");
    await closeScripts();

    // Closing with changes: "Save" in the dialog writes them and closes the tab.
    sqlEditor().setValue(`${scriptText}\n-- แก้ครั้งที่ 2`);
    await waitFor(sqlEdited, "edited again");
    await click(frontTab().querySelector("button"), "close the edited script");
    await click(await waitFor(() => document.querySelector("[data-close-save]"), "save in the close dialog"), "save and close");
    await waitFor(() => !document.querySelector("[data-close-dialog]") && !tabNamed("Status totals") && sqlTitle() === "Query 2", "saved and closed");
    const savedOnClose = await storedScripts();

    // Into the blank tab in front: it becomes the script. Then two editors are open, and
    // each one's ⌘↵ must run its own text.
    await openScripts();
    await click(scriptRows()[0].querySelector("button"), "open the script here");
    await waitFor(() => sqlTitle() === "Status totals" && sqlEditor().getValue() === `${scriptText}\n-- แก้ครั้งที่ 2` && !sqlEdited(), "loaded into the blank tab");
    const loadedHere = { tabs: tabStrip().length - sqlTabsBefore, bound: typeof JSON.parse((await invoke("get_settings")).workspace).tabs.find((x) => x.title === "Status totals")?.scriptId };
    await click(tabNamed("Query 1"), "first query tab");
    await waitFor(() => sqlTitle() === "Query 1", "first query tab in front");
    sqlEditor().setValue("SELECT 11 AS own_tab");
    await sleep(80);
    editorKey({ key: "Enter", code: "Enter", keyCode: 13, metaKey: true });
    await waitFor(() => gridCells().join() === "11", "⌘↵ ran this tab's text", 20000);
    await click(tabNamed("Status totals"), "script tab");
    await waitFor(() => sqlTitle() === "Status totals", "script tab in front");
    const ownKeys = { first: "11", other: sqlSummary() };

    // Deleting asks in place; the tab keeps its text and is a plain query tab again.
    await openScripts();
    await click(scriptRows()[0].querySelector('button[aria-label="Delete"]'), "delete");
    const deleteAsk = (await waitFor(() => document.querySelector("[data-sql-scripts][data-state=open] [data-script-deleting]"), "delete question")).textContent.replace(/\s+/g, " ").trim();
    await click(exact("[data-sql-scripts][data-state=open] [data-script-deleting] button", "Delete"), "confirm delete");
    await waitFor(() => document.querySelector("[data-sql-scripts][data-state=open] [data-script-empty]"), "no script left");
    await closeScripts();
    const afterDelete = {
      stored: (await storedScripts()).length,
      text: sqlEditor().getValue() === `${scriptText}\n-- แก้ครั้งที่ 2`,
      bound: typeof JSON.parse((await invoke("get_settings")).workspace).tabs.find((x) => x.title === "Status totals")?.scriptId,
      canSaveAgain: !saveButton().disabled,
    };
    await click(frontTab().querySelector("button"), "close the plain tab");
    await waitFor(() => tabStrip().length === sqlTabsBefore && sqlTitle() === "Query 1", "back to the first query tab");
    log.push({ stage: "sql-scripts-more", savedNew, savedByKey, reopened, savedOnClose, loadedHere, ownKeys, deleteAsk, afterDelete });
    if (
      !savedByKey ||
      reopened.tabs !== 2 ||
      !reopened.value ||
      loadedHere.tabs !== 1 ||
      loadedHere.bound !== "number" ||
      savedOnClose.length !== 1 ||
      savedOnClose[0].name !== "Status totals" ||
      !savedOnClose[0].sql.endsWith("-- แก้ครั้งที่ 2") ||
      ownKeys.other !== "" ||
      afterDelete.stored !== 0 ||
      !afterDelete.text ||
      afterDelete.bound !== "undefined" ||
      !afterDelete.canSaveAgain
    ) {
      throw new Error(`saved scripts went wrong: ${JSON.stringify({ savedByKey, reopened, savedOnClose, loadedHere, ownKeys, afterDelete })}`);
    }

    // ---- Smart Paste from a file ------------------------------------------------------
    // The native Open dialog is replaced by the runner's list of files (PURICAT_E2E_PICK):
    // the CSV and the workbook exported just above, a Thai CSV that is not UTF-8, a CSV that
    // is too long, and then nothing, which is what cancelling the dialog gives.
    await click(treeRow("customers"), "mysql customers");
    await waitFor(() => editState() === "editable" && cellText(0, 1) === "สมชาย ใจดี", "customers tab");
    const customerRows = dataRows().length;
    // A file cannot say NULL: an empty cell comes back as the empty text in a text column, so
    // the rows whose city is NULL are the only ones the exported files would change.
    const nullCities = dataRows().filter((_, r) => cellText(r, 2) === "NULL").length;
    const openFile = async () => click(await waitFor(() => smart()?.querySelector("[data-smart-open-file]:not(:disabled)"), "open-file button"), "open a file");
    const fileBar = () => smart().querySelector("[data-smart-file]")?.textContent.replace(/\s+/g, " ").trim();
    const smartSource = () => document.querySelector("[data-smart-source]")?.textContent.trim() ?? "";
    const roundTrip = async (what) => {
      await click(smartButton("Preview"), `preview ${what}`);
      await waitFor(() => document.querySelector("[data-smart-preview]") && smartCounts().unchanged !== undefined, `${what}: preview`, 15000);
      const counts = smartCounts();
      if (counts.unchanged !== customerRows - nullCities || counts.update !== nullCities || counts.insert || counts.problems) {
        throw new Error(`${what}: an exported file should come back identical except for ${nullCities} NULL cities of ${customerRows} rows: ${JSON.stringify(counts)} ${JSON.stringify(smartRows().slice(0, 12))}`);
      }
      return counts;
    };
    await click(panelButton("Smart Paste…"), "open smart paste");
    await waitFor(smart, "smart paste dialog");

    await openFile();
    await waitFor(() => smartSource().includes("export.csv"), "the CSV read");
    const csv = {
      source: smartSource(),
      bar: fileBar(),
      separator: smart().querySelector("[data-smart-separator]").selectedOptions[0].textContent.trim(),
      headersDetected: all('[data-smart-paste] input[type="checkbox"]')[0].checked,
      mapping: all('[data-smart-paste] select[aria-label^="Table column"]').map((el) => el.value),
    };
    csv.counts = await roundTrip("the CSV");
    await stage("smart-file-csv", { customerRows, nullCities, csv });

    await openFile();
    await waitFor(() => smartSource().includes("export.xlsx"), "the workbook read");
    const workbook = { source: smartSource(), bar: fileBar(), mapping: all('[data-smart-paste] select[aria-label^="Table column"]').map((el) => el.value) };
    workbook.counts = await roundTrip("the workbook");
    await stage("smart-file-xlsx", { workbook });

    // Not UTF-8: refused with the reason, read once its encoding is named.
    await openFile();
    const notUtf8 = (await waitFor(() => smart().querySelector("[data-smart-file-error]"), "refusal of the encoding")).textContent.replace(/\s+/g, " ").trim();
    await choose(smart().querySelector("[data-smart-encoding]"), "windows-874");
    await waitFor(() => smartSource().includes("thai874.csv") && smart().textContent.includes("ทดสอบภาษาไทย"), "the Thai CSV read");
    const thai = { notUtf8, source: smartSource(), bar: fileBar() };
    await stage("smart-file-encoding", { thai });

    // More rows than one run takes: nothing is offered for mapping.
    await openFile();
    const tooMany = (await waitFor(() => smart().querySelector("[data-smart-too-many]"), "too-many state", 15000)).textContent.replace(/\s+/g, " ").trim();
    // A cancelled dialog changes nothing.
    await openFile();
    await sleep(400);
    const afterCancel = !!smart().querySelector("[data-smart-too-many]");
    // The runner's list is used up: a request must come back as cancelled, at once. A native
    // dialog would leave it unanswered (and open on the screen for the rest of the run).
    const pastTheList = await Promise.race([invoke("pick_table_file"), sleep(3000).then(() => "no answer")]);
    if (pastTheList !== null) throw new Error(`a file request past the runner's list was not a cancelled dialog: ${JSON.stringify(pastTheList)}`);
    await stage("smart-file-limit", { tooMany, afterCancel });
    await click(smart().querySelector('button[aria-label="Close"]'), "close smart paste");
    await waitFor(() => !smart(), "smart paste closed");

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

    // ---- AI panel: detection and a chat, through stand-in CLIs (e2e/fixtures/bin) ---------
    // No account and no network are involved: the stand-ins print the stream the real CLIs print.
    await click(document.querySelector('button[aria-label="Toggle AI panel"]'), "ai toggle");
    const aiCard = () => all("aside").at(-1);
    const agentStatus = () => aiCard()?.querySelector("[data-agent-status]")?.textContent.replace(/\s+/g, " ").trim() ?? "";
    await waitFor(() => /Signed in|Not installed|Not signed in/.test(agentStatus()), "claude detection", 30000);
    const claude = agentStatus();
    const modelSelect = () => aiCard().querySelector("[data-agent-model]");
    const setValue = async (el, value) => {
      const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLSelectElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
      await sleep(60);
    };
    await waitFor(() => modelSelect()?.options.length >= 6, "claude models", 20000);
    const claudeModels = [...modelSelect().options].map((o) => o.textContent);

    // the context follows the tab in front (the PostgreSQL orders table) and is shown before it is sent
    const contextLabel = aiCard().querySelector("[data-agent-context]").textContent.trim();
    await click(aiCard().querySelector("[data-agent-context]"), "context");
    const contextPreview = (await waitFor(() => document.querySelector("[data-agent-context-preview]"), "context preview", 20000)).textContent;
    if (!contextPreview.includes("Engine: PostgreSQL") || !/\nTable orders\b/.test(contextPreview) || /127\.0\.0\.1|__PG_USER__@/.test(contextPreview)) {
      throw new Error(`unexpected agent context: ${contextPreview.slice(0, 600)}`);
    }
    await waitFor(() => {
      const preview = document.querySelector("[data-agent-context-preview]");
      preview?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      return !preview || preview.closest("[data-state]")?.dataset.state === "closed";
    }, "context closed");

    const lastAnswer = () => all('aside [data-agent-turn="assistant"]').at(-1);
    const ask = async (text) => {
      const answers = all('aside [data-agent-turn="assistant"]').length;
      await setValue(aiCard().querySelector("[data-agent-input]"), text);
      await click(aiCard().querySelector('button[aria-label="Send"]'), "send");
      // The answer's turn appears a moment after Send. Until it does, the "last answer" is
      // still the one before, already done: what is read next must be this question's.
      await waitFor(() => all('aside [data-agent-turn="assistant"]').length > answers, "the answer to appear", 20000);
    };
    const answered = (status, what) => waitFor(() => lastAnswer()?.dataset.status === status, what, 20000);
    const flat = (el) => el.textContent.replace(/\s+/g, " ").trim();

    await ask("นับจำนวน orders ให้หน่อย");
    await answered("done", "agent answer");
    const answer = {
      text: flat(lastAnswer()),
      code: lastAnswer().querySelector("[data-agent-code] pre")?.textContent,
      bold: lastAnswer().querySelector("strong")?.textContent,
      inlineCode: lastAnswer().querySelector("p code")?.textContent,
      question: flat(aiCard().querySelector('[data-agent-turn="user"]')),
    };
    if (answer.code !== "SELECT COUNT(*) AS orders\nFROM orders;" || answer.bold !== "อ่านอย่างเดียว") throw new Error(`answer rendered wrongly: ${JSON.stringify(answer)}`);
    await stage("agent-chat", { claude, claudeModels, contextLabel, contextPreview: contextPreview.slice(0, 500), answer });

    // ---- database tools: the AI looks things up itself, and changes need approval --------
    // The stand-in is a real MCP client: it starts the app's own bridge and calls the tools.
    const accessButton = () => aiCard().querySelector("[data-agent-access]");
    let accessShown = false;
    const setAccess = async (level) => {
      await click(accessButton(), "access");
      await click(await waitFor(() => document.querySelector(`[data-agent-access-option="${level}"]`), "access options"), `access ${level}`);
      if (level === "data" && !accessShown) {
        accessShown = true;
        await waitFor(() => document.querySelector('[data-agent-access-option="data"]')?.getAttribute("aria-checked") === "true", "access chosen");
        const options = all("[data-agent-access-option]").map((o) => `${o.getAttribute("aria-checked") === "true" ? "●" : "○"} ${flat(o)}`);
        if (options.length !== 3 || !options[2].startsWith("● Structure and rows")) throw new Error(`unexpected access options: ${options}`);
        await stage("agent-access", { options, button: flat(accessButton()) });
      }
      await waitFor(() => {
        const option = document.querySelector("[data-agent-access-option]");
        option?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        return !option || option.closest("[data-state]")?.dataset.state === "closed";
      }, "access closed");
      await waitFor(() => accessButton().dataset.agentAccess === level, `access is ${level}`);
    };
    const accessDefault = accessButton().dataset.agentAccess;
    if (accessDefault !== "none") throw new Error(`database tools are on by default: ${accessDefault}`);
    await setAccess("data");

    const pgId = (await invoke("list_connections")).find((c) => c.engine === "postgres").id;
    const readOne = async (sql) => {
      const outcome = await invoke("run_sql", { id: pgId, tab: "e2e-check", database: "puricat_e2e", sql, options: { maxRows: 10, changeLimit: null } });
      return outcome.statements[0].results[0].rows[0][0];
    };
    const firstTotal = () => readOne("SELECT total_amount FROM shop.orders WHERE id = 1").then(Number);
    const orderCount = () => readOne("SELECT COUNT(*) FROM shop.orders").then(Number);
    const lastProposal = () => all("aside [data-agent-proposal]").at(-1);
    const proposalIs = (state, what) => waitFor(() => lastProposal()?.dataset.state === state, what, 20000);

    const totalBefore = await firstTotal();
    await ask("ดูตาราง orders แล้วเสนอแก้ยอดของรายการแรกให้หน่อย");
    await answered("done", "answer with tools");
    const steps = [...lastAnswer().querySelectorAll("[data-agent-step]")].map((s) => `${s.dataset.agentStep}:${s.dataset.status}`);
    if (steps.join() !== "list_tables:ok,describe_table:ok,run_select:ok,run_select:error") throw new Error(`unexpected tool steps: ${steps}`);
    // every step opens to show exactly what the AI was given
    await click(lastAnswer().querySelector('[data-agent-step="run_select"][data-status="ok"] button'), "open query step");
    const rowsGiven = (await waitFor(() => lastAnswer().querySelector("[data-agent-step-output]"), "step output")).textContent;
    if (!/^4 rows\. /.test(rowsGiven) || !rowsGiven.includes("Completed | ")) throw new Error(`unexpected query output: ${rowsGiven}`);
    await click(lastAnswer().querySelector('[data-agent-step="run_select"][data-status="error"] button'), "open refused step");
    const refusal = (await waitFor(() => lastAnswer().querySelector('[data-agent-step="run_select"][data-status="error"] [data-agent-step-output]'), "refusal")).textContent;
    await proposalIs("pending", "a proposal that waits");
    const proposed = { sql: lastProposal().querySelector("pre").textContent, card: flat(lastProposal()), answer: flat(lastAnswer()).slice(-90) };
    if ((await firstTotal()) !== totalBefore) throw new Error("the proposal ran before it was approved");
    await stage("agent-tools", { accessDefault, steps, rowsGiven: rowsGiven.slice(0, 160), refusal, proposed });

    // approving is a second, deliberate step: the whole SQL and where it runs
    await click(withText("aside [data-agent-proposal] button", "Review and run"), "review");
    const reviewDialog = flat(await waitFor(() => document.querySelector("[data-agent-review]"), "review dialog"));
    await stage("agent-review", { reviewDialog });
    await click(exact("[data-agent-review] button", "Approve and run"), "approve");
    await proposalIs("done", "proposal run");
    const approved = { card: flat(lastProposal().querySelector("[data-agent-outcome]")), total: await firstTotal() };
    if (Math.abs(approved.total - totalBefore - 1) > 0.001) throw new Error(`approval did not change the row as proposed: ${totalBefore} -> ${approved.total}`);

    // a proposal that touches more rows than the connection's limit is rolled back, and can be left
    await ask("ลบคำสั่งซื้อทั้งหมดเลย");
    await answered("done", "second answer with tools");
    await proposalIs("pending", "delete proposal");
    const warned = flat(lastProposal());
    await click(withText("aside [data-agent-proposal][data-state=pending] button", "Review and run"), "review delete");
    await click(exact("[data-agent-review] button", "Approve and run"), "approve delete");
    await proposalIs("blocked", "delete held back");
    const held = { card: flat(lastProposal().querySelector("[data-agent-outcome]")), orders: await orderCount() };
    if (held.orders !== 1250) throw new Error(`the row limit did not hold: ${held.orders} orders left`);
    await stage("agent-held", { warned: warned.slice(0, 200), held });
    await click(exact("aside [data-agent-proposal][data-state=blocked] button", "Don't run it"), "leave it");
    await proposalIs("rejected", "delete not run");

    // what the limit held back runs after a third, separate confirmation that shows all of it:
    // the held statement and the one after it. The statement that ran before is not run again.
    const secondTotal = () => readOne("SELECT total_amount FROM shop.orders WHERE id = 2").then(Number);
    const thirdTotal = () => readOne("SELECT total_amount FROM shop.orders WHERE id = 3").then(Number);
    const secondBefore = await secondTotal();
    const thirdBefore = await thirdTotal();
    await ask("แก้หลายแถวพร้อมกัน");
    await answered("done", "answer with a wide update");
    await proposalIs("pending", "wide proposal");
    await click(withText("aside [data-agent-proposal][data-state=pending] button", "Review and run"), "review wide");
    await click(exact("[data-agent-review] button", "Approve and run"), "approve wide");
    await proposalIs("blocked", "wide update held back");
    const partly = { card: flat(lastProposal().querySelector("[data-agent-outcome]")), total: await secondTotal(), third: await thirdTotal() };
    if (Math.abs(partly.total - secondBefore - 1) > 0.001) throw new Error(`the statement before the limit did not run once: ${secondBefore} -> ${partly.total}`);
    if (partly.third !== thirdBefore) throw new Error(`a statement after the held one ran: ${thirdBefore} -> ${partly.third}`);
    await click(withText("aside [data-agent-proposal][data-state=blocked] button", "Run without the limit"), "lift");
    const liftButton = await waitFor(() => exact("[data-agent-review] button", "Run anyway"), "lift dialog");
    const liftDialog = flat(document.querySelector("[data-agent-review]"));
    const liftSql = document.querySelector("[data-agent-review] pre").textContent;
    // the dialog shows exactly what will run, and says that the statement after it runs too
    if (liftSql !== "UPDATE shop.orders SET meta = meta WHERE id <= 20;\nUPDATE shop.orders SET total_amount = total_amount + 1 WHERE id = 3" || !document.querySelector("[data-agent-lift-rest]")) {
      throw new Error(`the confirmation does not show everything that will run: ${liftDialog}`);
    }
    await stage("agent-lift", { partly, liftDialog });
    await click(liftButton, "run anyway");
    await proposalIs("done", "wide update run");
    const lifted = { card: flat(lastProposal().querySelector("[data-agent-outcome]")), total: await secondTotal(), third: await thirdTotal() };
    const outcomes = all("aside [data-agent-proposal]").at(-1).querySelectorAll("[data-agent-outcome] > li").length;
    if (lifted.total !== partly.total || Math.abs(lifted.third - thirdBefore - 1) > 0.001 || outcomes !== 3 || !lifted.card.includes("20 rows affected")) {
      throw new Error(`running what was held back went wrong: ${JSON.stringify(lifted)}`);
    }

    // MySQL, at the "Structure" level: the AI can look at tables but there is no query tool,
    // and an approved proposal runs on that connection
    const mysqlId = (await invoke("list_connections")).find((c) => c.engine === "mysql").id;
    const mysqlTotal = async () => {
      const outcome = await invoke("run_sql", { id: mysqlId, tab: "e2e-check-mysql", database: "puricat_e2e", sql: "SELECT total_amount FROM orders WHERE id = 1", options: { maxRows: 10, changeLimit: null } });
      return Number(outcome.statements[0].results[0].rows[0][0]);
    };
    const contextButton = () => aiCard().querySelector("[data-agent-context]").textContent.trim();
    await click(all('[role="tablist"] [role="tab"]').find((x) => x.textContent.includes("customers") && x.textContent.includes("Demo MySQL")), "mysql tab");
    await waitFor(() => contextButton().startsWith("Demo MySQL"), "mysql context");
    await setAccess("schema");
    const mysqlBefore = await mysqlTotal();
    await ask("ดูโครงสร้างแล้วเสนอแก้ยอดของรายการแรก");
    await answered("done", "answer on mysql");
    const mysqlSteps = [...lastAnswer().querySelectorAll("[data-agent-step]")].map((s) => `${s.dataset.agentStep}:${s.dataset.status}`);
    // no query step: at this level the reading tool is not offered at all
    if (mysqlSteps.join() !== "list_tables:ok,describe_table:ok") throw new Error(`unexpected tool steps on mysql: ${mysqlSteps}`);
    await click(lastAnswer().querySelector('[data-agent-step="describe_table"] button'), "open structure step");
    const structureGiven = (await waitFor(() => lastAnswer().querySelector("[data-agent-step-output]"), "structure output")).textContent;
    if (!/^orders\b/.test(structureGiven) || !structureGiven.includes("total_amount decimal(14,2) NOT NULL")) throw new Error(`unexpected structure: ${structureGiven}`);
    await proposalIs("pending", "mysql proposal");
    await click(withText("aside [data-agent-proposal][data-state=pending] button", "Review and run"), "review mysql");
    const mysqlReview = flat(await waitFor(() => document.querySelector("[data-agent-review]"), "mysql review"));
    if (!mysqlReview.includes("Runs onDemo MySQL")) throw new Error(`the review names the wrong connection: ${mysqlReview}`);
    await click(exact("[data-agent-review] button", "Approve and run"), "approve mysql");
    await proposalIs("done", "mysql proposal run");
    const mysqlAfter = await mysqlTotal();
    if (Math.abs(mysqlAfter - mysqlBefore - 1) > 0.001) throw new Error(`the MySQL proposal did not change the row as proposed: ${mysqlBefore} -> ${mysqlAfter}`);
    await invoke("close_console", { tab: "e2e-check-mysql" });
    await stage("agent-mysql", { context: contextButton(), mysqlSteps, structureGiven: structureGiven.slice(0, 200), mysqlReview: mysqlReview.slice(0, 160), total: [mysqlBefore, mysqlAfter] });
    await click(all('[role="tablist"] [role="tab"]').find((x) => x.textContent.includes("orders") && x.textContent.includes("Demo Postgr")), "back to postgres");
    await waitFor(() => contextButton().startsWith("Demo PostgreSQL"), "postgres context");
    await setAccess("data");

    // rejecting runs nothing
    await ask("เสนอแก้ยอดอีกครั้ง");
    await answered("done", "third answer with tools");
    await proposalIs("pending", "third proposal");
    await click(exact("aside [data-agent-proposal][data-state=pending] button", "Reject"), "reject");
    await proposalIs("rejected", "rejected");
    const rejected = { badge: flat(lastProposal().querySelector("[data-agent-proposal-state]")), total: await firstTotal(), orders: await orderCount() };
    if (rejected.total !== approved.total || rejected.orders !== 1250) throw new Error(`a rejected proposal changed data: ${JSON.stringify(rejected)}`);
    await invoke("close_console", { tab: "e2e-check" });

    // everything the AI read or proposed, and what was decided, is in the activity log
    await click(aiCard().querySelector('button[aria-label="AI activity log"]'), "activity log");
    await waitFor(() => document.querySelector("[data-agent-log-entry]"), "log entries", 20000);
    const logged = all("[data-agent-log-entry]").map((e) => e.dataset.agentLogEntry).reverse();
    for (const event of ["read", "read_failed", "proposed", "approved", "executed", "blocked", "rejected"]) {
      if (!logged.includes(event)) throw new Error(`the activity log has no "${event}" entry: ${logged}`);
    }
    await stage("agent-log", { logged, approved, lifted, rejected });
    await click(document.querySelector('[data-agent-log] button[aria-label="Close"]'), "close log");
    await waitFor(() => {
      const dialogEl = document.querySelector("[data-agent-log]");
      return !dialogEl || dialogEl.dataset.state === "closed";
    }, "log closed");
    await setAccess("none");

    // SQL from an answer opens in a new query tab on that connection; it is not run
    const tabsBefore = all('[role="tablist"] [role="tab"]').length;
    await click(withText("aside [data-agent-code] button", "Open in query tab"), "open in query tab");
    await waitFor(() => all('[role="tablist"] [role="tab"]').length === tabsBefore + 1 && sqlTab()?.__editor?.getValue() === answer.code, "sql opened in a tab", 30000);
    const opened = { tab: flat(document.querySelector('[role="tablist"] [role="tab"][aria-selected="true"]')), ran: !!document.querySelector(`${panelSel} [data-sql-summary]`) };
    if (opened.ran) throw new Error("SQL from the agent ran without being asked to");

    // a model the CLI rejects: its own words are shown, and the question can be asked again
    await setValue(modelSelect(), "\u0000custom");
    const modelInput = await waitFor(() => aiCard().querySelector("[data-agent-model-input]"), "model input");
    await type(modelInput, "broken");
    modelInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await waitFor(() => modelSelect()?.value === "broken", "custom model chosen");
    await ask("แล้วแยกตามสถานะล่ะ");
    await answered("error", "agent error");
    const modelRefused = flat(lastAnswer().querySelector('[role="alert"]'));
    await setValue(modelSelect(), "haiku");
    await click(withText("aside button", "Try again"), "retry");
    await answered("done", "retried answer");
    const retried = flat(lastAnswer()).slice(0, 60);

    // Stop ends a turn that is still streaming, and keeps what had arrived
    await ask("take your time");
    await waitFor(() => lastAnswer()?.dataset.status === "streaming" && lastAnswer().textContent.includes("Let me think"), "streaming answer", 20000);
    await click(aiCard().querySelector('button[aria-label="Stop"]'), "stop agent");
    await answered("stopped", "agent stopped");
    const stopped = flat(lastAnswer());

    // Codex: its model catalogue; the CLI's default is refused by the service, a listed model answers
    await click(exact('aside [role="radio"]', "Codex"), "codex");
    await waitFor(() => /codex-cli/.test(agentStatus()), "codex detection", 30000);
    const codex = agentStatus();
    await waitFor(() => [...(modelSelect()?.options ?? [])].some((o) => o.value === "stand-in-large"), "codex models", 20000);
    const codexModels = [...modelSelect().options].map((o) => o.textContent);
    await ask("what is in orders?");
    await answered("error", "codex refusal");
    const codexRefused = flat(lastAnswer().querySelector('[role="alert"]'));
    await setValue(modelSelect(), "stand-in-large");
    await click(withText("aside button", "Try again"), "retry codex");
    await answered("done", "codex answer");
    const codexAnswer = flat(lastAnswer());
    await stage("agent-codex", { codex, codexModels, codexRefused, codexAnswer, opened, modelRefused, retried, stopped });

    // ---- SQLite: a database file, from the connection form to a saved edit --------------
    await click(document.querySelector('button[aria-label="New connection"]'), "new connection");
    await waitFor(dialog, "connection dialog");
    await click(document.querySelector('[role="dialog"] [data-engine="sqlite"]'), "sqlite engine");
    const fileInput = await waitFor(() => document.querySelector('[role="dialog"] [data-sqlite-file]'), "the file field");
    const readOnlySwitch = () => document.querySelector('[role="dialog"] [role="switch"]');
    const sqliteForm = {
      // Nothing of a server is asked for a file, and a new file connection starts read-only.
      serverFields: ["Host", "Port", "Username", "Password", "TLS"].filter((label) =>
        all('[role="dialog"] label').some((l) => l.querySelector("span")?.textContent.trim().startsWith(label)),
      ),
      readOnlyByDefault: readOnlySwitch().getAttribute("aria-checked"),
    };
    if (sqliteForm.serverFields.length > 0 || sqliteForm.readOnlyByDefault !== "true") throw new Error(`the SQLite form is not what it should be: ${JSON.stringify(sqliteForm)}`);
    await type(field("Name"), "Demo SQLite");
    const testFile = async (path) => {
      await type(fileInput, path);
      await waitFor(() => !document.querySelector('[role="dialog"] [data-test-result]'), "the earlier test result to go");
      await click(exact('[role="dialog"] button', "Test connection"), "test");
      return (await waitFor(() => document.querySelector('[role="dialog"] [data-test-result]'), `test result for ${path}`, 20000)).textContent.replace(/\s+/g, " ").trim();
    };
    // A file that is something else, and a path that does not exist: clear refusals.
    const notADatabase = await testFile("__WORK__/export.csv");
    if (!notADatabase.includes("not a SQLite database")) throw new Error(`a CSV file was not refused as it should be: ${notADatabase}`);
    const missingFile = await testFile("__WORK__/nothing-here.sqlite");
    if (missingFile.startsWith("Connected")) throw new Error("a file that does not exist reported success");
    const sqliteTest = await testFile("__WORK__/demo.sqlite");
    if (!sqliteTest.startsWith("Connected to SQLite 3.")) throw new Error(`SQLite test failed: ${sqliteTest}`);
    await stage("dialog-sqlite", { sqliteForm, notADatabase, missingFile, sqliteTest });
    await save();

    // Its tables are listed at once (the file's one database is opened for you), and as the
    // connection is read-only the table cannot be edited.
    await click(treeRow("Demo SQLite"), "sqlite node");
    await click(await waitFor(() => treeRow("books"), "sqlite tables"), "books");
    await waitFor(() => cellText(0, 1) === "ธรรมบท ภาค ๑", "sqlite rows");
    const sqliteReadOnly = { state: editState(), firstRow: gridCells(), footer: footer(), tree: all("[data-tree-row]").map(rowLabel).filter((x) => ["main", "books", "tags", "long_books"].includes(x)) };
    if (sqliteReadOnly.state !== "read-only") throw new Error("a read-only SQLite connection shows an editable table");
    // 2^53 + 1 and a blob, exactly as stored.
    if (cellText(2, 2) !== "9007199254740993" || cellText(0, 6) !== "0x89504E47") throw new Error(`SQLite values are not shown exactly: ${cellText(2, 2)}, ${cellText(0, 6)}`);

    // Turn read-only off in the connection form; the session is opened again with it.
    await rightClick(treeRow("Demo SQLite"));
    await click(await waitFor(() => menuItem("Edit…"), "context menu"), "edit connection");
    await waitFor(() => document.querySelector('[role="dialog"] [data-sqlite-file]')?.value === "__WORK__/demo.sqlite", "the form of the saved connection");
    await click(readOnlySwitch(), "read-only off");
    await waitFor(() => readOnlySwitch().getAttribute("aria-checked") === "false", "switch off");
    await click(exact('[role="dialog"] button', "Save"), "save connection");
    await waitFor(() => !dialog(), "dialog to close");
    await click(treeRow("Demo SQLite"), "sqlite node again");
    await waitFor(() => editState() === "editable" && cellText(0, 1) === "ธรรมบท ภาค ๑", "an editable table", 20000);

    // An edit, and text that is not a number for a column of numbers: SQLite would store
    // it, PuriCat refuses the save and keeps what was typed.
    mouse(cellAt(0, 1), "dblclick");
    await typeInEditor("ธรรมบท ภาค ๑ (ตรวจแล้ว)");
    await waitFor(() => cellAt(0, 1)?.dataset.edited === "true", "staged title");
    mouse(cellAt(0, 3), "dblclick");
    await typeInEditor("many");
    await waitFor(() => cellAt(0, 3)?.dataset.edited === "true", "staged pages");
    await click(panelButton("Save"), "save with a bad number");
    const sqliteRefused = (await waitFor(() => document.querySelector(`${panelSel} [data-save-error]`), "the refusal")).textContent.replace(/\s+/g, " ").trim();
    if (!sqliteRefused.includes("not a number")) throw new Error(`the bad number was not refused as it should be: ${sqliteRefused}`);
    if (cellText(0, 1) !== "ธรรมบท ภาค ๑ (ตรวจแล้ว)") throw new Error("the staged title was lost after the refused save");
    await gridKey("z", { metaKey: true });
    await waitFor(() => cellText(0, 3) === "248", "the bad number undone");
    await click(panelButton("Save"), "save");
    await waitFor(() => !document.querySelector(`${panelSel} [data-dirty-bar]`) && cellText(0, 1) === "ธรรมบท ภาค ๑ (ตรวจแล้ว)" && cellAt(0, 1).dataset.edited !== "true", "saved and reloaded", 15000);
    await stage("sqlite-books", { sqliteReadOnly, sqliteRefused, firstRow: gridCells(), footer: footer() });

    // A query tab on the file: what SQLite really stores, and a statement of its own dialect.
    await rightClick(treeRow("Demo SQLite"));
    await click(await waitFor(() => menuItemWith("New query"), "new query menu"), "new query");
    await waitFor(() => sqlTab()?.__editor && activePanel().textContent.includes("Demo SQLite"), "sqlite query tab", 30000);
    await runSql("SELECT title, typeof(price) AS stored_as, price FROM books ORDER BY id;\nPRAGMA table_info(tags);");
    await waitFor(() => gridCells().includes("ธรรมบท ภาค ๑ (ตรวจแล้ว)"), "sqlite result rows", 20000);
    await stage("sqlite-sql", {
      row: gridCells(),
      tabs: all(`${panelSel} [role="tab"]`).map((b) => b.textContent.replace(/\s+/g, " ").trim()),
      summary: document.querySelector(`${panelSel} [data-sql-summary]`)?.textContent.trim(),
    });

    // ---- duplicate + delete through context menus -----------------------------------
    await rightClick(treeRow("Demo MySQL"));
    await click(await waitFor(() => menuItem("Duplicate"), "context menu"), "duplicate");
    const copy = await waitFor(() => treeRow("Demo MySQL copy"), "duplicated connection");
    // The first menu's "Delete…" belongs to the original: wait until no menu is open.
    await waitFor(() => all('[role="menu"]').every((m) => m.dataset.state === "closed"), "first context menu to close");
    // A script saved for the copy goes with it, and the dialog says so.
    const copyId = (await invoke("list_connections")).find((c) => c.name === "Demo MySQL copy").id;
    await invoke("create_script", { id: copyId, name: "left behind", sql: "SELECT 1" });
    await rightClick(copy);
    await click(await waitFor(() => menuItem("Delete…"), "context menu of the copy"), "delete");
    await waitFor(() => document.querySelector('[role="alertdialog"]'), "confirm dialog");
    const deleteText = document.querySelector('[role="alertdialog"]').textContent.replace(/\s+/g, " ").trim();
    if (!deleteText.includes("Demo MySQL copy")) throw new Error(`the delete dialog is not about the copy: ${deleteText}`);
    const scriptsNamed = (await waitFor(() => document.querySelector("[data-delete-scripts]"), "the scripts that go with it")).textContent.trim();
    await stage("delete-confirm", { text: deleteText, scriptsNamed });
    await click(exact('[role="alertdialog"] button', "Delete connection"), "confirm delete");
    await waitFor(() => !treeRow("Demo MySQL copy"), "connection removed");
    const scriptsLeft = (await invoke("list_scripts", { id: copyId })).length;
    if (scriptsLeft !== 0) throw new Error(`${scriptsLeft} script(s) outlived their connection`);

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
    // The filter bar is looked at in both themes and both languages: put it in front, in use.
    await click(all('[role="tablist"] [role="tab"]').find((x) => x.textContent.includes("orders") && x.textContent.includes("Demo Postgr")), "postgres orders tab");
    await click(await waitFor(() => document.querySelector(`${panelSel} [data-filter-toggle]`), "filter button"), "filter button");
    await waitFor(filterBar, "filter bar");
    await condition(0, "status", "contains", "pend");
    await click(panelButton("Apply"), "apply");
    await waitFor(() => filterCount() === "1" && columnCells(STATUS).every((v) => v === "Pending"), "filter in front");
    await click(panelButton("Add condition"), "a second, unwritten condition");
    const darkItem = await palette("dark theme");
    await waitFor(() => document.documentElement.dataset.theme === "dark", "dark theme");
    await stage("dark", { ran: darkItem, bg: getComputedStyle(document.body).backgroundColor });
    const thaiItem = await palette("language");
    await waitFor(() => document.documentElement.lang === "th", "thai");
    const thaiFilter = {
      button: document.querySelector(`${panelSel} [data-filter-toggle]`).textContent.trim(),
      labels: all(`${panelSel} [data-filter-row] > span`).map((x) => x.textContent.trim()),
      conditions: [...filterRows()[0].querySelectorAll("select")[1].options].map((o) => o.textContent.trim()),
      note: document.querySelector(`${panelSel} [data-filter-note]`).textContent.trim(),
      buttons: all(`${panelSel} [data-filter-bar] button`).map((b) => b.textContent.trim()).filter(Boolean),
    };
    if (!footer().includes("ใช้อยู่ 1 เงื่อนไข")) throw new Error(`the footer does not show the filter in Thai: ${footer()}`);
    await stage("thai-dark", { ran: thaiItem, footer: footer(), thaiFilter });
    // The query tab in Thai and dark: the mark in the margin and the saved scripts.
    await click(tabNamed("Query 1"), "first query tab");
    await waitFor(() => sqlTitle() === "Query 1", "query tab in front");
    sqlEditor().setValue("SELECT 1 AS one;\n\n-- สอง\nSELECT 2 AS two;\nSELECT 3");
    cursorTo(4, 5);
    await waitFor(() => markedLines() === "4", "mark in the margin");
    await click(scriptsToggle(), "saved scripts");
    const thaiScripts = (await waitFor(() => document.querySelector("[data-sql-scripts][data-state=open] [data-script-empty]"), "empty list in Thai")).parentElement.parentElement.textContent.replace(/\s+/g, " ").trim();
    await stage("thai-dark-sql", { thaiScripts, save: saveButton().getAttribute("aria-label") });
    await closeScripts();
    // The SQLite connection form in Thai and dark.
    await rightClick(treeRow("Demo SQLite"));
    await click(await waitFor(() => menuItem("แก้ไข…"), "context menu in Thai"), "edit the sqlite connection");
    await waitFor(() => document.querySelector('[role="dialog"] [data-sqlite-file]')?.value === "__WORK__/demo.sqlite", "the sqlite form in Thai");
    const thaiSqlite = {
      labels: all('[role="dialog"] label > span:first-child').map((x) => x.textContent.replace(/\s+/g, " ").trim()),
      buttons: all('[role="dialog"] button').map((b) => b.textContent.replace(/\s+/g, " ").trim()).filter(Boolean),
      readOnly: document.querySelector('[role="dialog"] [role="switch"]').getAttribute("aria-checked"),
    };
    if (!thaiSqlite.labels.includes("ไฟล์ฐานข้อมูล") || !thaiSqlite.buttons.includes("เลือกไฟล์…")) throw new Error(`the SQLite form is not in Thai: ${JSON.stringify(thaiSqlite)}`);
    await stage("thai-dark-sqlite", { thaiSqlite });
    await click(exact('[role="dialog"] button', "ยกเลิก"), "cancel the form");
    await waitFor(() => !dialog(), "dialog to close");
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
