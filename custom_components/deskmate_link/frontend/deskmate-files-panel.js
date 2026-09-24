// Deskmate Files - Home Assistant sidebar page.
// Send files from this phone or laptop to a paired computer, and download
// files from the folders that computer shares. Every name coming from the
// computer is inserted with textContent, never as HTML.

const API = "deskmate_link/files";

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value);
  }
  for (const child of [].concat(children)) {
    if (child) node.append(child);
  }
  return node;
}

function formatSize(bytes) {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

function formatDate(seconds) {
  if (!seconds) return "";
  return new Date(seconds * 1000).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function errorText(err) {
  return err?.body?.message || err?.message || err?.error || String(err);
}

const STYLE = `
  :host { display: block; min-height: 100%; background: var(--primary-background-color); color: var(--primary-text-color); }
  .toolbar { display: flex; align-items: center; gap: 8px; height: var(--header-height, 56px); padding: 0 12px;
    background: var(--app-header-background-color, var(--primary-color)); color: var(--app-header-text-color, #fff);
    font-size: 20px; }
  .toolbar .title { flex: 1; }
  .toolbar select { font: inherit; font-size: 14px; padding: 4px 8px; border-radius: 4px; border: 0; max-width: 45vw; }
  main { max-width: 920px; margin: 0 auto; padding: 16px; display: grid; gap: 16px; }
  section { background: var(--card-background-color); border-radius: var(--ha-card-border-radius, 12px);
    border: 1px solid var(--divider-color); padding: 16px; }
  h2 { margin: 0 0 4px; font-size: 18px; font-weight: 500; }
  .muted { color: var(--secondary-text-color); font-size: 14px; line-height: 1.45; }
  .notice { font-size: 14px; line-height: 1.45; padding: 10px 12px; border-radius: 8px; margin-top: 12px;
    border: 1px solid var(--divider-color); }
  .notice.error { border-color: var(--error-color); color: var(--error-color); }
  .drop { margin-top: 12px; border: 2px dashed var(--divider-color); border-radius: 10px; padding: 24px 16px;
    text-align: center; transition: border-color 0.15s ease-out, background-color 0.15s ease-out; }
  .drop.over { border-color: var(--primary-color); background: rgba(var(--rgb-primary-color, 3, 169, 244), 0.06); }
  button { font: inherit; font-size: 14px; cursor: pointer; border-radius: 6px; padding: 8px 14px;
    border: 1px solid var(--primary-color); background: var(--primary-color); color: var(--text-primary-color, #fff); }
  button.quiet { background: transparent; color: var(--primary-color); }
  button:disabled { opacity: 0.5; cursor: default; }
  button:focus-visible, .row:focus-visible, .crumb:focus-visible { outline: 2px solid var(--primary-color); outline-offset: 2px; }
  .transfers { margin-top: 12px; display: grid; gap: 10px; }
  .transfer { display: grid; gap: 6px; }
  .transfer .line { display: flex; justify-content: space-between; gap: 12px; font-size: 14px; }
  .transfer .name { overflow-wrap: anywhere; }
  .bar { height: 6px; border-radius: 3px; background: var(--divider-color); overflow: hidden; }
  .bar > div { height: 100%; width: 100%; background: var(--primary-color); transform: scaleX(0); transform-origin: left; }
  .transfer.done .bar > div { background: var(--success-color, #43a047); }
  .transfer.failed .bar > div { background: var(--error-color); }
  .crumbs { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; margin: 12px 0 8px; font-size: 14px; }
  .crumb { background: none; border: 0; padding: 2px 4px; color: var(--primary-color); }
  .crumb.current { color: var(--primary-text-color); cursor: default; }
  .list { border-top: 1px solid var(--divider-color); }
  .row { display: grid; grid-template-columns: 24px 1fr auto; gap: 12px; align-items: center; padding: 10px 4px;
    border-bottom: 1px solid var(--divider-color); font-size: 14px; }
  .row.dir { cursor: pointer; }
  .row .meta { color: var(--secondary-text-color); font-size: 12px; }
  .row .label { overflow-wrap: anywhere; }
  ha-icon { --mdc-icon-size: 20px; color: var(--secondary-text-color); }
  :host([narrow]) main { padding: 8px; }
  :host([narrow]) .row { grid-template-columns: 24px 1fr; }
  :host([narrow]) .row button { grid-column: 2; justify-self: start; }
`;

class DeskmateFilesPanel extends HTMLElement {
  constructor() {
    super();
    this._devices = [];
    this._device = null;
    this._info = null;
    this._path = null;
  }

  set hass(hass) {
    const first = !this._hass;
    this._hass = hass;
    if (this._menu) this._menu.hass = hass;
    if (first) this._init();
  }

  set narrow(value) {
    this._narrow = value;
    this.toggleAttribute("narrow", Boolean(value));
    if (this._menu) this._menu.narrow = value;
  }

  set panel(_value) {}
  set route(_value) {}

  _init() {
    const root = this.attachShadow({ mode: "open" });
    root.append(el("style", { text: STYLE }));

    this._menu = document.createElement("ha-menu-button");
    this._menu.hass = this._hass;
    this._menu.narrow = this._narrow;
    this._select = el("select", { "aria-label": "Computer", onchange: () => this._pick(this._select.value) });
    root.append(el("div", { class: "toolbar" }, [this._menu, el("div", { class: "title", text: "Deskmate Files" }), this._select]));

    this._fileInput = el("input", { type: "file", multiple: "", hidden: "", onchange: () => this._queue(this._fileInput.files) });
    this._chooseButton = el("button", { text: "Choose files", onclick: () => this._fileInput.click() });
    this._drop = el("div", { class: "drop" }, [
      el("p", { class: "muted", text: "Drop files here or" }),
      this._chooseButton,
      this._fileInput,
    ]);
    this._drop.addEventListener("dragover", (event) => {
      event.preventDefault();
      this._drop.classList.add("over");
    });
    this._drop.addEventListener("dragleave", () => this._drop.classList.remove("over"));
    this._drop.addEventListener("drop", (event) => {
      event.preventDefault();
      this._drop.classList.remove("over");
      this._queue(event.dataTransfer.files);
    });
    this._sendTitle = el("h2", { text: "Send to computer" });
    this._inboxText = el("p", { class: "muted" });
    this._sendNotice = el("div", { class: "notice", hidden: "" });
    this._transfers = el("div", { class: "transfers" });

    this._browseTitle = el("h2", { text: "Files on computer" });
    this._browseText = el("p", { class: "muted" });
    this._crumbs = el("nav", { class: "crumbs", "aria-label": "Folder path" });
    this._list = el("div", { class: "list" });
    this._browseNotice = el("div", { class: "notice", hidden: "" });

    this._main = el("main", {}, [
      el("section", {}, [this._sendTitle, this._inboxText, this._sendNotice, this._drop, this._transfers]),
      el("section", {}, [this._browseTitle, this._browseText, this._browseNotice, this._crumbs, this._list]),
    ]);
    root.append(this._main);
    this._loadDevices();
  }

  _notice(target, message, isError = false) {
    target.hidden = !message;
    target.textContent = message || "";
    target.classList.toggle("error", isError);
  }

  async _loadDevices() {
    try {
      this._devices = await this._hass.callApi("GET", API);
    } catch (err) {
      this._notice(this._sendNotice, `Could not load computers: ${errorText(err)}`, true);
      return;
    }
    this._select.replaceChildren(
      ...this._devices.map((device) =>
        el("option", { value: device.entry_id, text: device.connected ? device.name : `${device.name} (offline)` }),
      ),
    );
    this._select.hidden = this._devices.length < 2;
    if (!this._devices.length) {
      this._notice(this._sendNotice, "No computer is paired yet. Add the Deskmate Link integration first.", true);
      this._drop.hidden = true;
      return;
    }
    const firstOnline = this._devices.find((device) => device.connected) || this._devices[0];
    this._select.value = firstOnline.entry_id;
    this._pick(firstOnline.entry_id);
  }

  async _pick(entryId) {
    this._device = this._devices.find((device) => device.entry_id === entryId);
    this._info = null;
    this._path = null;
    const name = this._device ? this._device.name : "computer";
    this._sendTitle.textContent = `Send to ${name}`;
    this._browseTitle.textContent = `Files on ${name}`;
    this._notice(this._sendNotice, "");
    this._notice(this._browseNotice, "");
    this._list.replaceChildren();
    this._crumbs.replaceChildren();
    if (!this._device.connected) {
      this._inboxText.textContent = "";
      this._browseText.textContent = "";
      this._drop.hidden = true;
      this._notice(this._sendNotice, `${name} is not connected right now. Start Deskmate on it and reload this page.`, true);
      return;
    }
    try {
      this._info = await this._hass.callApi("GET", `${API}/${entryId}/roots`);
    } catch (err) {
      this._notice(this._sendNotice, errorText(err), true);
      return;
    }
    this._renderInbox();
    this._renderRoots();
  }

  _renderInbox() {
    const inbox = this._info.inbox || {};
    const limit = inbox.max_bytes ? formatSize(inbox.max_bytes) : "";
    if (inbox.mode === "confirm" || inbox.mode === "automatic") {
      const how = inbox.mode === "confirm" ? "Someone at the computer accepts each file." : "Files are accepted automatically.";
      this._inboxText.textContent = `Saved to ${inbox.dir || "Downloads\\Deskmate"}. ${how} Up to ${limit} per file.`;
      this._drop.hidden = false;
    } else {
      this._inboxText.textContent = "";
      this._drop.hidden = true;
      this._notice(this._sendNotice, "Receiving files is turned off on this computer. Turn it on in Deskmate: Settings > Receive files.");
    }
  }

  _renderRoots() {
    const roots = this._info.roots || [];
    this._path = null;
    this._crumbs.replaceChildren();
    if (!roots.length) {
      this._browseText.textContent = "";
      this._list.replaceChildren();
      this._notice(this._browseNotice, "This computer shares no folders. Add one in Deskmate: Settings > File access.");
      return;
    }
    this._browseText.textContent = "Folders this computer shares. Read only.";
    this._notice(this._browseNotice, "");
    this._list.replaceChildren(
      ...roots.map((root) => this._row({ name: root, dir: true }, root)),
    );
  }

  async _open(path) {
    this._notice(this._browseNotice, "");
    let data;
    try {
      data = await this._hass.callApi("GET", `${API}/${this._device.entry_id}/list?path=${encodeURIComponent(path)}`);
    } catch (err) {
      this._notice(this._browseNotice, errorText(err), true);
      return;
    }
    this._path = path;
    this._renderCrumbs();
    const entries = data.entries || [];
    entries.sort((a, b) => Number(b.dir) - Number(a.dir));
    this._list.replaceChildren(
      ...(entries.length
        ? entries.map((entry) => this._row(entry, `${path.replace(/[\\/]+$/, "")}\\${entry.name}`))
        : [el("p", { class: "muted", text: "This folder is empty." })]),
    );
  }

  _renderCrumbs() {
    const roots = this._info.roots || [];
    const root = roots.find((candidate) => this._path.toLowerCase().startsWith(candidate.toLowerCase())) || "";
    const crumbs = [el("button", { class: "crumb", text: "Shared folders", onclick: () => this._renderRoots() })];
    let current = root;
    crumbs.push(el("span", { text: "/" }));
    crumbs.push(el("button", { class: "crumb", text: root, onclick: () => this._open(root) }));
    const rest = this._path.slice(root.length).split(/[\\/]+/).filter(Boolean);
    for (const part of rest) {
      current = `${current.replace(/[\\/]+$/, "")}\\${part}`;
      const target = current;
      crumbs.push(el("span", { text: "/" }));
      crumbs.push(el("button", { class: "crumb", text: part, onclick: () => this._open(target) }));
    }
    crumbs[crumbs.length - 1].classList.add("current");
    this._crumbs.replaceChildren(...crumbs);
  }

  _row(entry, fullPath) {
    const icon = el("ha-icon", { icon: entry.dir ? "mdi:folder" : "mdi:file-outline" });
    const meta = entry.dir ? "" : [formatSize(entry.size || 0), formatDate(entry.mtime)].filter(Boolean).join(" · ");
    const label = el("div", {}, [el("div", { class: "label", text: entry.name }), meta ? el("div", { class: "meta", text: meta }) : null]);
    if (entry.dir) {
      return el("div", {
        class: "row dir",
        role: "button",
        tabindex: "0",
        onclick: () => this._open(fullPath),
        onkeydown: (event) => {
          if (event.key === "Enter") this._open(fullPath);
        },
      }, [icon, label]);
    }
    const download = el("button", { class: "quiet", text: "Download", onclick: () => this._download(fullPath, entry.name, download) });
    return el("div", { class: "row" }, [icon, label, download]);
  }

  async _download(path, name, button) {
    button.disabled = true;
    try {
      const signed = await this._hass.callWS({
        type: "auth/sign_path",
        path: `/api/${API}/${this._device.entry_id}/download?path=${encodeURIComponent(path)}`,
        expires: 300,
      });
      const link = el("a", { href: signed.path, download: name });
      document.body.append(link);
      link.click();
      link.remove();
    } catch (err) {
      this._notice(this._browseNotice, errorText(err), true);
    } finally {
      button.disabled = false;
    }
  }

  _queue(files) {
    const list = Array.from(files || []);
    this._fileInput.value = "";
    const limit = (this._info && this._info.inbox && this._info.inbox.max_bytes) || 0;
    this._chain = (this._chain || Promise.resolve()).then(async () => {
      for (const file of list) {
        const item = this._transferRow(file);
        if (limit && file.size > limit) {
          this._finish(item, `Larger than the ${formatSize(limit)} limit set on the computer`, false);
          continue;
        }
        await this._upload(file, item);
      }
    });
  }

  _transferRow(file) {
    const status = el("span", { class: "muted", text: "Waiting" });
    const fill = el("div");
    const node = el("div", { class: "transfer" }, [
      el("div", { class: "line" }, [el("span", { class: "name", text: `${file.name} (${formatSize(file.size)})` }), status]),
      el("div", { class: "bar" }, [fill]),
    ]);
    this._transfers.prepend(node);
    return { node, status, fill, size: file.size };
  }

  _progress(item, sent) {
    const ratio = item.size ? Math.min(1, sent / item.size) : 1;
    item.fill.style.transform = `scaleX(${ratio.toFixed(4)})`;
    item.status.textContent = `${Math.round(ratio * 100)}%`;
  }

  _finish(item, message, ok) {
    item.node.classList.add(ok ? "done" : "failed");
    item.fill.style.transform = "scaleX(1)";
    item.status.textContent = message;
  }

  async _token() {
    const auth = this._hass.auth;
    if (auth.expired) await auth.refreshAccessToken();
    return auth.data.access_token;
  }

  _post(url, body, onProgress) {
    return new Promise((resolve, reject) => {
      this._token().then((token) => {
        const xhr = new XMLHttpRequest();
        xhr.open("POST", url);
        xhr.setRequestHeader("Authorization", `Bearer ${token}`);
        xhr.setRequestHeader("Content-Type", "application/octet-stream");
        xhr.upload.onprogress = (event) => onProgress(event.loaded);
        xhr.onload = () => {
          let data = {};
          try {
            data = JSON.parse(xhr.responseText || "{}");
          } catch (_err) {
            data = {};
          }
          if (xhr.status >= 200 && xhr.status < 300) resolve(data);
          else reject(new Error(data.message || `HTTP ${xhr.status}`));
        };
        xhr.onerror = () => reject(new Error("Network error"));
        xhr.send(body);
      }, reject);
    });
  }

  async _upload(file, item) {
    const base = `${API}/${this._device.entry_id}`;
    const confirm = this._info && this._info.inbox && this._info.inbox.mode === "confirm";
    item.status.textContent = confirm ? "Waiting for the computer to accept" : "Starting";
    let start;
    try {
      start = await this._hass.callApi("POST", `${base}/upload_start`, { name: file.name, size: file.size });
    } catch (err) {
      this._finish(item, errorText(err), false);
      return;
    }
    let offset = 0;
    try {
      while (offset < file.size) {
        const piece = file.slice(offset, offset + start.chunk_bytes);
        const sentBefore = offset;
        await this._post(`/api/${base}/upload_chunk?upload=${start.upload}&offset=${offset}`, piece, (loaded) =>
          this._progress(item, sentBefore + loaded),
        );
        offset += piece.size;
        this._progress(item, offset);
      }
      const done = await this._hass.callApi("POST", `${base}/upload_finish`, { upload: start.upload });
      this._finish(item, `Saved as ${done.name}`, true);
    } catch (err) {
      this._finish(item, errorText(err), false);
      this._hass.callApi("POST", `${base}/upload_abort`, { upload: start.upload }).catch(() => {});
    }
  }
}

customElements.define("deskmate-files-panel", DeskmateFilesPanel);
