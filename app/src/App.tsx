import React, { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion, setTheme as setAppTheme } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import "./App.css";

type Logon = {
  id: string;
  client: string;
  user: string;
  language: string;
  password: string;
};

type System = {
  id: string;
  name: string;
  sid: string;
  host: string;
  sysnr: string;
  logons: Logon[];
  folder: string[];
  notes: string;
  environment: string;
  favorite: boolean;
  lastUsed: number;
};

type Store = { sapguiPath: string; systems: System[]; folders: string[][] };

const ENVIRONMENTS = ["", "Development", "QA", "Production"];
const RECENT_COUNT = 5;

const EnvBadge = ({ env }: { env: string }) =>
  env ? <span className={`env env-${env.toLowerCase().replace(/\s+/g, "-")}`}>{env}</span> : null;

const ICON = { width: 16, height: 16, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor",
  strokeWidth: 1.8, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": true } as const;

const FolderIcon = () => (
  <svg {...ICON} className="icon">
    <path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2h8.5A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z" />
  </svg>
);

const ConnectionIcon = () => (
  <svg {...ICON} className="icon">
    <rect x="3" y="4" width="18" height="12" rx="1.5" />
    <path d="M8 20h8M12 16v4" />
  </svg>
);

const ChevronIcon = ({ open }: { open: boolean }) => (
  <svg {...ICON} className="chevron">
    <path d={open ? "M6 9l6 6 6-6" : "M9 6l6 6-6 6"} />
  </svg>
);

const CopyIcon = () => (
  <svg {...ICON}>
    <rect x="9" y="9" width="11" height="11" rx="1.5" />
    <path d="M5 15H4.5A1.5 1.5 0 0 1 3 13.5v-9A1.5 1.5 0 0 1 4.5 3h9A1.5 1.5 0 0 1 15 4.5V5" />
  </svg>
);

const EyeIcon = ({ off }: { off: boolean }) => (
  <svg {...ICON}>
    <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" />
    <circle cx="12" cy="12" r="3" />
    {off && <path d="M4 4l16 16" />}
  </svg>
);

const LaunchIcon = () => (
  <svg {...ICON}>
    <path d="M7 4.5v15l12-7.5z" />
  </svg>
);

const EditIcon = () => (
  <svg {...ICON}>
    <path d="M4 20l1-4.5L16.5 4 20 7.5 8.5 19z" />
    <path d="M14 6.5l3.5 3.5" />
  </svg>
);

const DeleteIcon = () => (
  <svg {...ICON}>
    <path d="M4 7h16M9.5 7V4.5h5V7M6.5 7l.8 12.5h9.4L17.5 7M10.5 11v5M13.5 11v5" />
  </svg>
);

const StarIcon = () => (
  <svg {...ICON}>
    <path d="M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.9L12 16.9l-5.2 2.8 1-5.9L3.5 9.7l5.9-.8z" />
  </svg>
);

const THEMES = ["system", "light", "dark"] as const;
type Theme = (typeof THEMES)[number];
const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

function loadTheme(): Theme {
  try {
    return THEMES.find((t) => t === localStorage.getItem("theme")) ?? "system";
  } catch {
    return "system";
  }
}

function showTheme(theme: Theme) {
  const dark = theme === "dark" || (theme === "system" && darkQuery.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

// Before the first render, so the window does not flash in the other theme.
showTheme(loadTheme());

const RELEASES_URL = "https://github.com/tariktekin8/sapgui-launcher/releases/latest";

// A system leaves the width of the folder chevron free, so all icons of one level share a column.
const TREE = { start: 8, chevron: 16, indent: 16 };
const SIDEBAR = { min: 220, max: 480, default: 260 };
const clampWidth = (w: number) => Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, Math.round(w)));

const missingFields = (l: Logon) => (["client", "user", "password"] as const).filter((key) => !l[key]);
const isComplete = (l: Logon) => missingFields(l).length === 0;

const StatusDot = ({ ok, okText, missingText }: { ok: boolean; okText: string; missingText: string }) => (
  <span role="img" className={ok ? "dot ok" : "dot missing"}
    aria-label={ok ? okText : missingText} title={ok ? okText : missingText} />
);

type Folder = { name: string; path: string[]; folders: Folder[]; systems: System[] };

function buildTree(systems: System[], folders: string[][]): Folder {
  const root: Folder = { name: "", path: [], folders: [], systems: [] };
  const nodeAt = (path: string[]) => {
    let node = root;
    for (const name of path) {
      let child = node.folders.find((f) => f.name === name);
      if (!child) {
        child = { name, path: [...node.path, name], folders: [], systems: [] };
        node.folders.push(child);
      }
      node = child;
    }
    return node;
  };
  folders.forEach(nodeAt);
  for (const s of systems) nodeAt(s.folder).systems.push(s);
  return root;
}

function folderPaths(node: Folder): string[][] {
  return [...node.folders].sort((a, b) => a.name.localeCompare(b.name)).flatMap((f) => [f.path, ...folderPaths(f)]);
}

// from is the folder being edited; without it the form adds a new one.
type FolderForm = { name: string; parent: string[]; from?: string[] };

type Drag = { label: string; x: number; y: number; target: string | null };
const DRAG_THRESHOLD = 5;
const SCROLL_EDGE = 24;

const startsWith = (path: string[], start: string[]) => start.every((name, i) => path[i] === name);

type Question = { title: string; text: string; action: string; run: () => void };

function ConfirmDialog({ question, onClose }: { question: Question; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (!ref.current?.open) ref.current?.showModal();
  }, []);
  // close() gives the focus back; the close event comes too late to unmount on it.
  function close() {
    ref.current?.close();
    onClose();
  }
  return (
    <dialog ref={ref} className="confirm" aria-labelledby="confirm-title" aria-describedby="confirm-text"
      onCancel={(e) => { e.preventDefault(); close(); }}>
      <h2 id="confirm-title">{question.title}</h2>
      <p id="confirm-text">{question.text}</p>
      <div className="row">
        <button type="button" autoFocus onClick={close}>Cancel</button>
        <button type="button" className="danger primary"
          onClick={() => {
            close();
            question.run();
          }}>
          {question.action}
        </button>
      </div>
    </dialog>
  );
}

function FolderSelect({ paths, value, onChange }: { paths: string[][]; value: string[]; onChange: (path: string[]) => void }) {
  return (
    <select value={JSON.stringify(value)} onChange={(e) => onChange(JSON.parse(e.target.value))}>
      <option value="[]">None</option>
      {paths.map((p) => <option key={JSON.stringify(p)} value={JSON.stringify(p)}>{p.join(" / ")}</option>)}
    </select>
  );
}

// size: xs/sm/md/sel are fixed widths with centered text; grow fills the row. sel fits the longest environment.
type Size = "xs" | "sm" | "md" | "sel" | "grow";
type SystemField = { key: "sid" | "name" | "environment" | "host" | "sysnr" | "folder" | "notes"; label: string; size: Size };

const SECTIONS: { title: string; rows: SystemField[][] }[] = [
  { title: "System", rows: [[
    { key: "sid", label: "SID", size: "xs" },
    { key: "name", label: "Name", size: "grow" },
    { key: "environment", label: "Environment", size: "sel" },
  ]] },
  { title: "Connection", rows: [[
    { key: "host", label: "Host", size: "md" },
    { key: "sysnr", label: "System number", size: "sm" },
    { key: "folder", label: "Folder", size: "grow" },
  ]] },
  { title: "Notes", rows: [[{ key: "notes", label: "Notes", size: "grow" }]] },
];

// No '&' or '=' (they may break the SAP GUI for Java conn string), no space, '!' or '?' (SAP rules).
const PASSWORD_CHARS = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789#$%*+-_.:@";

function generatePassword(length = 16): string {
  const bytes = crypto.getRandomValues(new Uint32Array(length));
  return Array.from(bytes, (b) => PASSWORD_CHARS[b % PASSWORD_CHARS.length]).join("");
}

const emptySystem = (): System => ({
  id: crypto.randomUUID(),
  name: "", sid: "", host: "", sysnr: "00", logons: [], folder: [], notes: "",
  environment: "", favorite: false, lastUsed: 0,
});

const emptyLogon = (): Logon => ({ id: crypto.randomUUID(), client: "", user: "", language: "EN", password: "" });

export default function App() {
  const [store, setStore] = useState<Store>({ sapguiPath: "", systems: [], folders: [] });
  const [theme, setTheme] = useState(loadTheme);
  const [loaded, setLoaded] = useState(false);
  const [filter, setFilter] = useState("");
  const [version, setVersion] = useState("");
  const [update, setUpdate] = useState<Update | null>(null);
  const [updateState, setUpdateState] = useState<"available" | "installing" | "failed">("available");
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    try {
      const w = Number(localStorage.getItem("sidebarWidth"));
      return w ? clampWidth(w) : SIDEBAR.default;
    } catch {
      return SIDEBAR.default;
    }
  });
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editing, setEditing] = useState<System | null>(null);
  // true while the form was opened with Add, which offers a system or a folder
  const [adding, setAdding] = useState(false);
  const [folderForm, setFolderForm] = useState<FolderForm | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [editingLogon, setEditingLogon] = useState<Logon | null>(null);
  const [question, setQuestion] = useState<Question | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [shown, setShown] = useState<Set<string>>(new Set());
  const [message, setToast] = useState<{ id: number; text: string; error: boolean } | null>(null);

  function setMessage(text: string, error = false) {
    setToast(text ? { id: Date.now(), text, error } : null);
  }

  useEffect(() => {
    if (!message) return;
    const t = setTimeout(() => setToast(null), message.error ? 8000 : 3000);
    return () => clearTimeout(t);
  }, [message]);

  async function run<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
    setMessage("");
    try {
      return await invoke<T>(cmd, args);
    } catch (e) {
      setMessage(String(e), true);
    }
  }

  async function apply(cmd: string, args?: Record<string, unknown>) {
    const next = await run<Store>(cmd, args);
    if (next) setStore(next);
    return next;
  }

  // Looks for a newer published GitHub release once at startup; offline or errors stay silent.
  useEffect(() => {
    getVersion().then(setVersion);
    check().then(setUpdate).catch(() => {});
  }, []);

  async function installUpdate() {
    if (!update) return;
    setUpdateState("installing");
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch {
      // e.g. no write access or no administrator password: the user updates by hand
      setUpdateState("failed");
    }
  }

  useEffect(() => {
    const offStore = listen<Store>("store", (e) => setStore(e.payload));
    const offError = listen<string>("launch-error", (e) => setMessage(e.payload, true));
    const offHidden = listen("hidden", () => setShown(new Set()));
    return () => {
      offStore.then((f) => f());
      offError.then((f) => f());
      offHidden.then((f) => f());
    };
  }, []);

  useEffect(() => {
    apply("get_store").then((s) => setLoaded(s !== undefined));
  }, []);

  const visible = useMemo(() => {
    const f = filter.trim().toUpperCase();
    return [...store.systems]
      .sort((a, b) => a.name.localeCompare(b.name))
      .filter((s) => {
        if (!f) return true;
        const users = s.logons.map((l) => `${l.client} ${l.user}`).join(" ");
        return `${s.name} ${s.sid} ${users} ${s.folder.join(" ")}`.toUpperCase().includes(f);
      });
  }, [store.systems, filter]);

  useEffect(() => {
    try {
      localStorage.setItem("sidebarWidth", String(sidebarWidth));
    } catch {
      // width is only a convenience; ignore blocked storage
    }
  }, [sidebarWidth]);

  useEffect(() => {
    try {
      localStorage.setItem("theme", theme);
    } catch {
      // the theme still applies until the app closes
    }
    const show = () => showTheme(theme);
    show();
    // title bar and native menus
    setAppTheme(theme === "system" ? null : theme).catch(() => {});
    darkQuery.addEventListener("change", show);
    return () => darkQuery.removeEventListener("change", show);
  }, [theme]);

  // The sidebar starts at the window's left edge, so the pointer x is the new width.
  function startResize(e: React.PointerEvent) {
    e.preventDefault();
    const move = (ev: PointerEvent) => setSidebarWidth(clampWidth(ev.clientX));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("resizing");
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    document.body.classList.add("resizing");
  }

  const tree = useMemo(() => buildTree(visible, filter.trim() ? [] : store.folders), [visible, filter, store.folders]);
  const allFolders = useMemo(() => folderPaths(buildTree(store.systems, store.folders)), [store]);
  const isOpen = (key: string) => filter.trim() !== "" || !collapsed.has(key);

  function toggleFolder(key: string) {
    const next = new Set(collapsed);
    if (!next.delete(key)) next.add(key);
    setCollapsed(next);
  }

  const favorites = store.systems.filter((s) => s.favorite).sort((a, b) => a.name.localeCompare(b.name));
  const recent = store.systems
    .filter((s) => !s.favorite && s.lastUsed > 0)
    .sort((a, b) => b.lastUsed - a.lastUsed)
    .slice(0, RECENT_COUNT);

  const dropTarget = drag?.target ?? null;

  function renderItem(s: System, depth: number, key: string, inTree = false) {
    const ready = s.logons.some(isComplete);
    const sub = s.logons.length === 1
      ? [s.sid, s.logons[0].client, s.logons[0].user]
      : [s.sid, s.logons.length ? `${s.logons.length} users` : "No users"];
    return (
      <li key={key} className="system" data-folder={inTree ? JSON.stringify(s.folder) : ""}>
        <button
          type="button"
          style={{ paddingLeft: TREE.start + TREE.chevron + depth * TREE.indent }}
          className={s.id === selectedId ? "item selected" : "item"}
          aria-current={s.id === selectedId || undefined}
          onClick={() => select(s.id)}
          onPointerDown={inTree ? (e) => dragSystem(e, s) : undefined}
        >
          <span className="item-icon">
            <ConnectionIcon />
            <StatusDot ok={ready} okText="Ready to launch" missingText="Needs a user with client, user and password" />
          </span>
          <span className="item-text">
            <strong>
              <span className="name" title={s.name || s.host}>{s.name || s.host}</span>
              {s.favorite && <span role="img" aria-label="Favorite" title="Favorite" className="favorite"><StarIcon /></span>}
            </strong>
            <span className="sub">
              <EnvBadge env={s.environment} />
              <span className="sub-text">{sub.filter(Boolean).join(" · ")}</span>
            </span>
          </span>
        </button>
      </li>
    );
  }

  // Fixed folders cannot be edited or deleted. path is set only for the one that takes drops.
  function renderFixedFolder(name: string, content: React.ReactNode, path = "") {
    const open = isOpen(name);
    return (
      <li data-folder={path} className={path && dropTarget === path ? "drop" : undefined}>
        <div className="folder-row">
          <button type="button" className="folder" style={{ paddingLeft: TREE.start }} aria-expanded={open}
            onClick={() => toggleFolder(name)}>
            <ChevronIcon open={open} />
            <FolderIcon />
            <span className="folder-name">{name}</span>
          </button>
        </div>
        {open && <ul>{content}</ul>}
      </li>
    );
  }

  function renderShortcuts(name: string, systems: System[], emptyText: string) {
    return renderFixedFolder(name, systems.length
      ? systems.map((s) => renderItem(s, 1, `${name}-${s.id}`))
      : <li className="empty" style={{ paddingLeft: TREE.start + TREE.chevron + TREE.indent }}>{emptyText}</li>);
  }

  function renderTree(node: Folder, depth: number): React.ReactNode {
    const indent = { paddingLeft: TREE.start + depth * TREE.indent };
    return (
      <>
        {[...node.folders].sort((a, b) => a.name.localeCompare(b.name)).map((f) => {
          const key = JSON.stringify(f.path);
          const open = isOpen(key);
          return (
            <li key={key} data-folder={key} className={dropTarget === key ? "drop" : undefined}>
              <div className="folder-row">
                <button type="button" className="folder" style={indent} aria-expanded={open}
                  onClick={() => toggleFolder(key)} onPointerDown={(e) => dragFolder(e, f)}>
                  <ChevronIcon open={open} />
                  <FolderIcon />
                  <span className="folder-name" title={f.name}>{f.name}</span>
                </button>
                <button type="button" className="mini" onClick={() => startEditFolder(f)}
                  aria-label={`Edit folder ${f.name}`} title="Edit folder"><EditIcon /></button>
                <button type="button" className="mini danger" onClick={() => askDeleteFolder(f)}
                  aria-label={`Delete folder ${f.name}`} title="Delete folder"><DeleteIcon /></button>
              </div>
              {open && <ul>{renderTree(f, depth + 1)}</ul>}
            </li>
          );
        })}
        {node.systems.map((s) => renderItem(s, depth, `t-${s.id}`, true))}
      </>
    );
  }

  const selected = store.systems.find((s) => s.id === selectedId) ?? null;
  const isNew = editing !== null && !store.systems.some((s) => s.id === editing.id);
  const isNewLogon = editingLogon !== null && !selected?.logons.some((l) => l.id === editingLogon.id);

  function closeForms() {
    setEditing(null);
    setAdding(false);
    setFolderForm(null);
    setEditingLogon(null);
    setShown(new Set());
    setShowSettings(false);
  }

  function select(id: string) {
    closeForms();
    setSelectedId(id);
  }

  function startEdit(system: System) {
    closeForms();
    setEditing({ ...system });
  }

  function startEditFolder(f: Folder) {
    closeForms();
    setFolderForm({ name: f.name, parent: f.path.slice(0, -1), from: f.path });
  }

  function startEditLogon(logon: Logon) {
    closeForms();
    setEditingLogon({ ...logon });
  }

  function toggleShown(id: string) {
    const next = new Set(shown);
    if (!next.delete(id)) next.add(id);
    setShown(next);
  }

  async function copy(text: string, label: string) {
    const secret = label === "Password";
    if ((await run("copy_text", { text, secret })) !== undefined) {
      setMessage(secret ? "Password copied. It is cleared from the clipboard in 30 seconds." : `${label} copied.`);
    }
  }

  async function launch(s: System, logon: Logon) {
    if (await apply("launch_system", { id: s.id, logonId: logon.id })) {
      setMessage(`SAP GUI started for ${logon.user || "the user"}.`);
    }
  }

  async function testConnection(s: System) {
    if (!s.host) {
      setMessage("Add a host to this system first.", true);
      return;
    }
    setMessage(`Connecting to ${s.host}…`);
    const ms = await run<number>("check_connection", { host: s.host, sysnr: s.sysnr });
    if (ms !== undefined) setMessage(`${s.host} is reachable on port 32${s.sysnr} (${ms} ms).`);
  }

  function duplicate(s: System) {
    setSelectedId(null);
    startEdit({
      ...s,
      id: crypto.randomUUID(),
      name: `${s.name} (copy)`,
      logons: s.logons.map((l) => ({ ...l, id: crypto.randomUUID() })),
      favorite: false,
      lastUsed: 0,
    });
  }

  async function saveSystem() {
    if (!editing) return;
    if (await apply("save_system", { system: editing })) {
      closeForms();
      setSelectedId(editing.id);
      if (adding) setEditingLogon(emptyLogon());
    }
  }

  async function saveFolder() {
    if (!folderForm) return;
    const { name, parent, from } = folderForm;
    const path = [...parent, name.trim()];
    if (await (from ? apply("move_folder", { from, to: path }) : apply("add_folder", { path }))) {
      closeForms();
      setMessage(from ? "Folder saved." : "Folder added.");
    }
  }

  async function deleteFolder(f: Folder) {
    const parent = f.path.slice(0, -1).join(" / ");
    if (await applyMove("delete_folder", { path: f.path })) {
      setMessage(`Folder deleted. Its content is now ${parent ? `in ${parent}` : "at the top level"}.`);
    }
  }

  function askDeleteFolder(f: Folder) {
    const isEmpty = !store.systems.some((s) => startsWith(s.folder, f.path))
      && !allFolders.some((p) => p.length > f.path.length && startsWith(p, f.path));
    setQuestion({
      title: `Delete folder "${f.name}"?`,
      text: isEmpty ? "The folder is empty." : "Its content moves one folder up. No system is deleted.",
      action: "Delete folder",
      run: () => deleteFolder(f),
    });
  }

  function askDeleteLogon(system: System, l: Logon) {
    setQuestion({
      title: l.user ? `Delete user "${l.user}"?` : "Delete this user?",
      text: `The user and the saved password are removed from "${system.name || system.host}". This cannot be undone.`,
      action: "Delete user",
      run: () => removeLogon(system, l.id),
    });
  }

  function askDeleteSystem(s: System) {
    const users = s.logons.length;
    setQuestion({
      title: `Delete system "${s.name || s.host}"?`,
      text: users === 0 ? "This cannot be undone."
        : `${users === 1 ? "Its user and saved password are" : `Its ${users} users and their saved passwords are`} deleted too. This cannot be undone.`,
      action: "Delete system",
      run: () => removeSystem(s.id),
    });
  }

  // An open edit form must not write the old folder back on Save.
  async function applyMove(cmd: string, args: Record<string, unknown>) {
    const next = await apply(cmd, args);
    setEditing((e) => {
      const moved = next?.systems.find((s) => s.id === e?.id);
      return e && moved ? { ...e, folder: moved.folder } : e;
    });
    return next;
  }

  function dragSystem(e: React.PointerEvent, s: System) {
    startDrag(e, s.name || s.host,
      (path) => JSON.stringify(path) !== JSON.stringify(s.folder),
      (path) => applyMove("save_system", { system: { ...s, folder: path } }));
  }

  function dragFolder(e: React.PointerEvent, f: Folder) {
    const parent = JSON.stringify(f.path.slice(0, -1));
    startDrag(e, f.name,
      (path) => JSON.stringify(path) !== parent && !startsWith(path, f.path),
      (path) => applyMove("move_folder", { from: f.path, to: [...path, f.name] }));
  }

  // Pointer events, because HTML drag and drop needs different webview settings on each OS.
  function startDrag(e: React.PointerEvent, label: string, canDrop: (path: string[]) => boolean, drop: (path: string[]) => void) {
    if (e.button !== 0 || e.pointerType === "touch") return;
    const list = e.currentTarget.closest(".list");
    const { clientX, clientY } = e;
    let active = false;
    let target: string | null = null;
    const move = (ev: PointerEvent) => {
      if (!active && Math.hypot(ev.clientX - clientX, ev.clientY - clientY) < DRAG_THRESHOLD) return;
      active = true;
      document.body.classList.add("dragging");
      if (list) {
        const box = list.getBoundingClientRect();
        if (ev.clientY < box.top + SCROLL_EDGE) list.scrollTop -= SCROLL_EDGE;
        if (ev.clientY > box.bottom - SCROLL_EDGE) list.scrollTop += SCROLL_EDGE;
      }
      const path = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>("[data-folder]")?.dataset.folder;
      target = path && canDrop(JSON.parse(path)) ? path : null;
      setDrag({ label, x: ev.clientX, y: ev.clientY, target });
    };
    const end = (ev: Event) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("keydown", escape);
      document.body.classList.remove("dragging");
      setDrag(null);
      if (!active) return;
      // The release must not also count as a click on the row.
      const stop = (click: Event) => { click.stopPropagation(); click.preventDefault(); };
      window.addEventListener("click", stop, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", stop, true));
      if (ev.type === "pointerup" && target) drop(JSON.parse(target));
    };
    const escape = (ev: KeyboardEvent) => { if (ev.key === "Escape") end(ev); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("keydown", escape);
  }

  async function saveLogon() {
    if (!selected || !editingLogon) return;
    const logons = isNewLogon
      ? [...selected.logons, editingLogon]
      : selected.logons.map((l) => (l.id === editingLogon.id ? editingLogon : l));
    if (await apply("save_system", { system: { ...selected, logons } })) closeForms();
  }

  async function removeLogon(system: System, id: string) {
    await apply("save_system", { system: { ...system, logons: system.logons.filter((l) => l.id !== id) } });
  }

  async function removeSystem(id: string) {
    if (await apply("delete_system", { id })) setSelectedId(null);
  }

  async function importLandscape() {
    const before = store.systems.length;
    const next = await apply("import_landscape");
    if (!next) return;
    const added = next.systems.length - before;
    setMessage(added === 0 ? "No new systems in SAP Logon." : added === 1 ? "Imported 1 system." : `Imported ${added} systems.`);
  }

  async function chooseSapgui() {
    const next = await run<Store | null>("choose_sapgui");
    if (!next) return;
    setStore(next);
    setMessage("SAP GUI saved.");
  }

  function renderSections(renderField: (f: SystemField) => React.ReactNode) {
    return SECTIONS.map((section) => (
      <section key={section.title} className="section">
        <h3>{section.title}</h3>
        {section.rows.map((row, i) => (
          <div key={i} className="form-row">{row.map(renderField)}</div>
        ))}
      </section>
    ));
  }

  function renderAddSwitch() {
    return adding && (
      <div className="row" role="group" aria-label="What to add">
        <button type="button" aria-pressed={!folderForm} onClick={() => setFolderForm(null)}>System</button>
        <button type="button" aria-pressed={folderForm !== null}
          onClick={() => setFolderForm(folderForm ?? { name: "", parent: [] })}>Folder</button>
      </div>
    );
  }

  function renderFolderForm(folder: FolderForm) {
    const { from } = folder;
    return (
      <form className="form panel" onSubmit={(e) => { e.preventDefault(); saveFolder(); }}>
        <div className="title-row"><h2>{from ? "Edit folder" : "New folder"}</h2></div>
        <div className="panel-body">
          {renderAddSwitch()}
          <section className="section">
            <div className="form-row">
              <label className="field grow">
                Name
                <input value={folder.name} required autoComplete="off" spellCheck={false} autoFocus
                  onChange={(e) => setFolderForm({ ...folder, name: e.target.value })} />
              </label>
              <label className="field grow">
                Parent folder
                <FolderSelect paths={allFolders.filter((p) => !from || !startsWith(p, from))} value={folder.parent}
                  onChange={(parent) => setFolderForm({ ...folder, parent })} />
              </label>
            </div>
          </section>
        </div>
        <div className="row actions">
          <button type="submit" className="primary">Save</button>
          <button type="button" onClick={closeForms}>Cancel</button>
        </div>
      </form>
    );
  }

  function renderSystemForm(system: System) {
    return (
      <form className="form panel" onSubmit={(e) => { e.preventDefault(); saveSystem(); }}>
        <div className="title-row"><h2>{!isNew ? "Edit system" : adding ? "New system" : "Duplicate system"}</h2></div>
        <div className="panel-body">
          {renderAddSwitch()}
          {renderSections((f) => (
            <label key={f.key} className={`field ${f.size}`}>
              <span className={f.key === "notes" ? "sr-only" : undefined}>{f.label}</span>
              {f.key === "environment" ? (
                <select value={system.environment}
                  onChange={(e) => setEditing({ ...system, environment: e.target.value })}>
                  {ENVIRONMENTS.map((env) => <option key={env} value={env}>{env || "None"}</option>)}
                </select>
              ) : f.key === "folder" ? (
                <FolderSelect paths={allFolders} value={system.folder}
                  onChange={(folder) => setEditing({ ...system, folder })} />
              ) : f.key === "notes" ? (
                <textarea rows={5} value={system.notes}
                  onChange={(e) => setEditing({ ...system, notes: e.target.value })} />
              ) : (
                <input
                  value={system[f.key]}
                  onChange={(e) => {
                    const { value } = e.target;
                    const clean = f.key === "sid" ? value.toUpperCase() : f.key === "sysnr" ? value.replace(/\D/g, "").slice(0, 2) : value;
                    setEditing({ ...system, [f.key]: clean });
                  }}
                  required={f.key === "name"}
                  inputMode={f.key === "sysnr" ? "numeric" : undefined}
                  autoComplete="off"
                  spellCheck={false}
                />
              )}
            </label>
          ))}
        </div>
        <div className="row actions">
          <button type="submit" className="primary">Save</button>
          <button type="button" onClick={closeForms}>Cancel</button>
        </div>
      </form>
    );
  }

  function renderLogonForm(system: System, logon: Logon) {
    const set = (patch: Partial<Logon>) => setEditingLogon({ ...logon, ...patch });
    const show = shown.has(logon.id);
    return (
      <form className="form panel" onSubmit={(e) => { e.preventDefault(); saveLogon(); }}>
        <div className="title-row"><h2>{isNewLogon ? "New user" : "Edit user"}</h2></div>
        <div className="panel-body">
          <p className="empty">{[system.sid, system.name].filter(Boolean).join(" · ")}</p>
          <section className="section">
            <div className="form-row">
              <label className="field xs">
                Client
                <input value={logon.client} inputMode="numeric" autoComplete="off" autoFocus
                  onChange={(e) => set({ client: e.target.value.replace(/\D/g, "").slice(0, 3) })} />
              </label>
              <label className="field grow">
                User
                <input value={logon.user} autoComplete="off" spellCheck={false}
                  onChange={(e) => set({ user: e.target.value })} />
              </label>
              <label className="field xs">
                Language
                <input value={logon.language} autoComplete="off" spellCheck={false}
                  onChange={(e) => set({ language: e.target.value.toUpperCase() })} />
              </label>
            </div>
            <div className="form-row">
              <label className="field grow">
                Password
                <div className="password">
                  <input type={show ? "text" : "password"} value={logon.password} autoComplete="off" spellCheck={false}
                    onChange={(e) => set({ password: e.target.value })} />
                  <button type="button" onClick={() => toggleShown(logon.id)}
                    aria-label={show ? "Hide password" : "Show password"} title={show ? "Hide password" : "Show password"}>
                    <EyeIcon off={show} />
                  </button>
                  <button type="button" aria-label="Generate password"
                    onClick={() => {
                      set({ password: generatePassword() });
                      setShown(new Set([...shown, logon.id]));
                    }}>
                    Generate
                  </button>
                </div>
              </label>
            </div>
          </section>
        </div>
        <div className="row actions">
          <button type="submit" className="primary">Save</button>
          <button type="button" onClick={closeForms}>Cancel</button>
        </div>
      </form>
    );
  }

  function renderUsers(system: System) {
    return (
      <section className="section">
        <h3>Users</h3>
        {system.logons.length === 0 ? (
          <p className="empty">No users yet. Add one to launch this system.</p>
        ) : (
          <table className="users">
            <thead>
              <tr>
                <th><span className="sr-only">Status</span></th>
                <th>Client</th>
                <th>User</th>
                <th>Language</th>
                <th>Password</th>
                <th><span className="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {system.logons.map((l) => (
                <tr key={l.id}>
                  <td><StatusDot ok={isComplete(l)} okText="Ready to launch" missingText={`Missing: ${missingFields(l).join(", ")}`} /></td>
                  <td>{l.client || "—"}</td>
                  <td>
                    <span className="cell">
                      <strong>{l.user || "—"}</strong>
                      {l.user && <button type="button" className="mini" onClick={() => copy(l.user, "User")}
                        aria-label={`Copy user ${l.user}`} title="Copy user"><CopyIcon /></button>}
                    </span>
                  </td>
                  <td>{l.language || "—"}</td>
                  <td>
                    {l.password ? (
                      <span className="cell">
                        <span className="secret">{shown.has(l.id) ? l.password : "•".repeat(10)}</span>
                        <button type="button" className="mini" onClick={() => toggleShown(l.id)}
                          aria-label={shown.has(l.id) ? "Hide password" : "Show password"}
                          title={shown.has(l.id) ? "Hide password" : "Show password"}>
                          <EyeIcon off={shown.has(l.id)} />
                        </button>
                        <button type="button" className="mini" onClick={() => copy(l.password, "Password")}
                          aria-label={`Copy password of ${l.user}`} title="Copy password"><CopyIcon /></button>
                      </span>
                    ) : "—"}
                  </td>
                  <td>
                    <span className="cell end">
                      <button type="button" className="mini primary" onClick={() => launch(system, l)}
                        aria-label={`Launch as ${l.user}`} title="Launch"><LaunchIcon /></button>
                      <button type="button" className="mini" onClick={() => startEditLogon(l)}
                        aria-label={`Edit user ${l.user}`} title="Edit user"><EditIcon /></button>
                      <button type="button" className="mini danger" onClick={() => askDeleteLogon(system, l)}
                        aria-label={`Delete user ${l.user}`} title="Delete user"><DeleteIcon /></button>
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <div className="row">
          <button type="button" onClick={() => startEditLogon(emptyLogon())}>Add user</button>
        </div>
      </section>
    );
  }

  function renderDetails() {
    if (showSettings) {
      return (
        <div className="form panel">
          <div className="title-row"><h2>Settings</h2></div>
          <div className="panel-body">
            <div className="form-row">
              <label className="field grow">
                Theme
                <select value={theme} onChange={(e) => setTheme(e.target.value as Theme)}>
                  <option value="system">System</option>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                </select>
              </label>
            </div>
            <div className="field" role="group" aria-labelledby="sapgui-label">
              <span id="sapgui-label">SAP GUI</span>
              <div className="picker">
                <span className="path">{store.sapguiPath || "Not found"}</span>
                <button type="button" onClick={chooseSapgui}>Choose file…</button>
              </div>
            </div>
          </div>
          <div className="row actions">
            <button type="button" onClick={() => setShowSettings(false)}>Close</button>
            <span className="version">Version {version}</span>
          </div>
        </div>
      );
    }

    if (folderForm) return renderFolderForm(folderForm);

    if (editing) return renderSystemForm(editing);

    if (!selected) {
      if (!loaded) return null;
      return (
        <p className="empty">
          {store.systems.length ? "Select a system on the left." : "Import your systems from SAP Logon, or add one."}
        </p>
      );
    }

    if (editingLogon) return renderLogonForm(selected, editingLogon);

    return (
      <div className="form panel readonly">
        <div className="title-row">
          <h2>{selected.name || selected.host || "System"}</h2>
          <button type="button" className={selected.favorite ? "star on" : "star"}
            aria-pressed={selected.favorite}
            onClick={() => apply("set_favorite", { id: selected.id, favorite: !selected.favorite })}>
            <StarIcon />
            {selected.favorite ? "Favorite" : "Add to favorites"}
          </button>
        </div>
        <div className="panel-body">
          {renderSections((f) => (
            <div key={f.key} className={f.key === "notes" ? "field wide notes" : "field"}>
              <span>{f.label}</span>
              {f.key === "environment" && selected.environment ? (
                <span className="value"><EnvBadge env={selected.environment} /></span>
              ) : (
                <span className="value">{(f.key === "folder" ? selected.folder.join(" / ") : selected[f.key]) || "—"}</span>
              )}
            </div>
          )).slice(0, 2)}
          {renderUsers(selected)}
          {renderSections((f) => (
            <div key={f.key} className="field wide notes">
              <span className="sr-only">{f.label}</span>
              <span className="value">{selected[f.key] || "—"}</span>
            </div>
          )).slice(2)}
        </div>
        <div className="row actions">
          <button type="button" onClick={() => startEdit(selected)}>Edit system</button>
          <button type="button" onClick={() => duplicate(selected)}>Duplicate</button>
          <button type="button" onClick={() => testConnection(selected)}>Test connection</button>
          <button type="button" className="danger" onClick={() => askDeleteSystem(selected)}>Delete system</button>
        </div>
      </div>
    );
  }

  // Kept between renders: typing in a form, resizing and dragging must not rebuild a long list.
  const list = useMemo(() => (
    <ul className="list" aria-label="Systems" data-folder="[]">
      {!filter.trim() && store.systems.length > 0 && (
        <>
          {renderShortcuts("Favorites", favorites, "No favorites yet.")}
          {renderShortcuts("Recent", recent, "No recent systems yet.")}
        </>
      )}
      {(tree.systems.length > 0 || tree.folders.length > 0) && renderFixedFolder("All systems", renderTree(tree, 1), "[]")}
      {filter.trim() && visible.length === 0 && <li className="empty">No system matches "{filter.trim()}".</li>}
      {loaded && !filter.trim() && store.systems.length === 0 && <li className="empty">No systems yet.</li>}
    </ul>
  ), [tree, store, filter, loaded, selectedId, collapsed, dropTarget]);

  return (
    <div className="layout">
      <aside className="sidebar" style={{ width: sidebarWidth }}>
        <input
          className="search"
          placeholder="Search systems"
          aria-label="Search systems"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          autoFocus
        />
        {list}
        {update && (
          <div className={updateState === "failed" ? "update failed" : "update"} role="status">
            {updateState === "failed" ? (
              <>
                <span>Automatic update failed. Download version {update.version} and install it.</span>
                <button onClick={() => openUrl(RELEASES_URL)}>Download</button>
              </>
            ) : (
              <>
                <span>Version {update.version} is available.</span>
                <div className="row">
                  <button className="primary" onClick={installUpdate} disabled={updateState === "installing"}>
                    {updateState === "installing" ? "Updating…" : "Update now"}
                  </button>
                  <button onClick={() => openUrl(RELEASES_URL)}>What's new</button>
                </div>
              </>
            )}
          </div>
        )}
        <div className="row">
          <button onClick={() => { setSelectedId(null); startEdit(emptySystem()); setAdding(true); }}>Add</button>
          <button onClick={importLandscape} title="Import systems from SAP Logon">Import</button>
          <button onClick={() => { closeForms(); setShowSettings(true); }}>Settings</button>
        </div>
      </aside>

      <div
        className="resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        aria-valuenow={sidebarWidth}
        aria-valuemin={SIDEBAR.min}
        aria-valuemax={SIDEBAR.max}
        tabIndex={0}
        onPointerDown={startResize}
        onDoubleClick={() => setSidebarWidth(SIDEBAR.default)}
        onKeyDown={(e) => {
          const step = e.key === "ArrowLeft" ? -16 : e.key === "ArrowRight" ? 16 : 0;
          if (step) setSidebarWidth(clampWidth(sidebarWidth + step));
        }}
      />

      {question && <ConfirmDialog question={question} onClose={() => setQuestion(null)} />}
      {drag && <div className="drag-label" style={{ left: drag.x + 12, top: drag.y + 8 }}>{drag.label}</div>}

      <main className="details">
        {renderDetails()}
        {message && (
          <div key={message.id} className={message.error ? "toast error" : "toast"}
            role={message.error ? "alert" : "status"}>
            {message.text}
          </div>
        )}
      </main>
    </div>
  );
}
