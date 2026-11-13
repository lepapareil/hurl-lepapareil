// Application state (collections, tabs, runs, preferences) and its actions.

import { create } from 'zustand';
import { call, onEvent, pickDirectory } from './api';
import {
  entryName,
  isSet,
  isSettable,
  type AppInfo,
  type Collection,
  type Entry,
  type EntryResult,
  type HurlFile,
  type HurlOptions,
  type OptionValue,
  type RunMeta,
  type RunResult,
  type TreeNode,
  type Variable,
} from './types';
import { askConfirm, askText, toast, toastError } from './ui';

export type Theme = 'light' | 'dark' | 'system';

export interface Prefs {
  theme: Theme;
  layout: 'horizontal' | 'vertical';
  hurlPath: string;
  /**
   * Options of `hurl` added to every command line, by long name. Which
   * options exist, and what their values are, is told by the catalog of
   * options (`hurlOptions` of the state): only the options which are set are
   * kept here.
   */
  options: Record<string, OptionValue>;
  /** When sending a request, run the previous entries of the file first. */
  chainEntries: boolean;
  verbosity: 'none' | 'verbose' | 'very-verbose';
  extraArgs: string;
}

export const DEFAULT_PREFS: Prefs = {
  theme: 'system',
  layout: 'horizontal',
  hurlPath: '',
  options: {},
  chainEntries: false,
  verbosity: 'verbose',
  extraArgs: '',
};

/**
 * Preferences as they were saved, which may date from before the options of
 * `hurl` came from its catalog: the few options the preferences knew then had
 * a field of their own.
 */
function loadPrefs(saved: Record<string, unknown> | undefined): Prefs {
  const { insecure, location, continueOnError, maxTime, proxy, options, ...rest } = saved ?? {};
  const legacy: Record<string, OptionValue> = {};
  if (insecure === true) legacy['insecure'] = true;
  if (location === true) legacy['location'] = true;
  if (continueOnError === true) legacy['continue-on-error'] = true;
  if (typeof maxTime === 'number' && maxTime > 0) legacy['max-time'] = String(Math.round(maxTime));
  if (typeof proxy === 'string' && proxy.trim() !== '') legacy['proxy'] = proxy.trim();
  const current = options && typeof options === 'object' ? (options as Record<string, OptionValue>) : {};
  return { ...DEFAULT_PREFS, ...(rest as Partial<Prefs>), options: { ...legacy, ...current } };
}

/** Keeps the options which exist in this version of Hurl and can be set in the preferences. */
function knownOptions(options: Record<string, OptionValue>, catalog: HurlOptions): Record<string, OptionValue> {
  const settable = new Set(catalog.options.filter(isSettable).map((o) => o.long));
  return Object.fromEntries(Object.entries(options).filter(([long, value]) => settable.has(long) && isSet(value)));
}

export type TabKind = 'entry' | 'file' | 'runner' | 'environments' | 'preferences';

export interface Tab {
  id: string;
  kind: TabKind;
  collection: string;
  /** Hurl file (entry, file) or run target (runner). */
  path: string;
  /** 1-based position of the entry in its file; 0 for the other tabs and for orphans. */
  index: number;
  /** Unsaved edits of an entry. */
  draft?: Entry;
  /** Fingerprint of the entry the draft was started from. */
  draftBase?: string;
  /** Unsaved edits of a file opened as text. */
  rawDraft?: string;
  /** Text of the file when the raw draft was started. */
  rawBase?: string;
  /** The file changed on disk under the draft: saving overwrites that change. */
  conflict?: boolean;
  /** The edited request no longer exists in the file: its draft is kept but cannot be saved. */
  orphan?: boolean;
  pane: string;
  responsePane: string;
}

export interface EntryRun {
  running: boolean;
  runId?: string;
  result?: EntryResult;
  meta?: RunMeta;
  /** Order of arrival of the result. */
  seq?: number;
  /** The response bodies and the hurl output were dropped to bound memory. */
  slim?: boolean;
}

export interface FileRun {
  running: boolean;
  runId?: string;
  result?: RunResult;
}

export interface RunnerFile {
  path: string;
  name: string;
  status: 'queued' | 'running' | 'passed' | 'failed' | 'cancelled';
  result?: RunResult;
}

export interface RunnerState {
  status: 'idle' | 'running' | 'done';
  files: RunnerFile[];
  cancelRequested: boolean;
  selected: string | null;
  runId: string | null;
}

export interface LogItem {
  id: number;
  time: number;
  command: string;
  exitCode: number | null;
  durationMs: number;
  success: boolean;
  cancelled: boolean;
  output: string;
}

/** Runs kept in the console. */
const MAX_LOGS = 100;
/** Characters of output kept per console entry (its end: errors come last). */
const MAX_LOG_OUTPUT = 200_000;
/** Results keeping their response bodies and hurl output, besides those shown in a tab. */
const RECENT_RESULTS = 50;

export const entryKey = (path: string, index: number) => `${path}#${index}`;
export const envDraftKey = (collection: string, name: string) => `${collection}\n${name}`;

/** Identifiers of the tabs. */
export const ids = {
  entry: (path: string, index: number) => `entry:${path}#${index}`,
  file: (path: string) => `file:${path}`,
  runner: (target: string) => `runner:${target}`,
  environments: (collection: string) => `env:${collection}`,
  preferences: 'preferences',
};

export function isUnder(path: string, parent: string): boolean {
  return path === parent || path.startsWith(`${parent}/`) || path.startsWith(`${parent}\\`);
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function findNode(nodes: TreeNode[], path: string): TreeNode | null {
  for (const n of nodes) {
    if (n.path === path) return n;
    if (n.type === 'folder' && isUnder(path, n.path)) {
      const found = findNode(n.children, path);
      if (found) return found;
    }
  }
  return null;
}

export function filesUnder(nodes: TreeNode[]): Extract<TreeNode, { type: 'file' }>[] {
  return nodes.flatMap((n) => (n.type === 'file' ? [n] : filesUnder(n.children)));
}

function resolveTheme(theme: Theme): 'light' | 'dark' {
  if (theme !== 'system') return theme;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = resolveTheme(theme);
}

function metaOf(result: RunResult): RunMeta {
  const { entries: _entries, ...meta } = result;
  return meta;
}

function failedMeta(runId: string, file: string, error: unknown): RunMeta {
  return {
    runId,
    file,
    command: '',
    exitCode: null,
    cancelled: false,
    durationMs: 0,
    success: false,
    errors: error instanceof Error ? error.message : String(error),
    stderr: '',
  };
}

/**
 * Result without its response bodies (their size is kept). A body which the
 * core did not load holds no content and stays as it is: "dropped" would tell
 * the user to send the request again, which does not bring it back.
 */
function slimEntry(entry: EntryResult): EntryResult {
  return {
    ...entry,
    calls: entry.calls.map((c) =>
      c.response.body && !c.response.body.omitted
        ? {
            ...c,
            response: {
              ...c.response,
              body: { size: c.response.body.size, truncated: false, encoded: false, dropped: true },
            },
          }
        : c,
    ),
  };
}

/** What the runner and the file view show: statuses, asserts and errors only. */
function slimResult(result: RunResult): RunResult {
  return { ...result, stderr: '', entries: result.entries.map(slimEntry) };
}

/** Content of an entry, whatever its position in the file. */
function fingerprint(entry: Entry): string {
  return JSON.stringify({ ...entry, index: 0, startLine: 0, endLine: 0 });
}

const identity = (entry: Entry) => `${entry.method} ${entry.url}\n${entry.docs}`;

/**
 * Follows the entries of a file across a change made outside of the forms
 * (text edit, another program): for each previous entry, the 1-based index of
 * the entry which is the same request now, or null when it is gone. Entries
 * are matched by content first, then by method, URL and documentation; at the
 * same position first, then at the nearest one.
 */
function matchEntries(before: Entry[], after: Entry[]): (number | null)[] {
  const mapping: (number | null)[] = before.map(() => null);
  const taken = new Set<number>();
  for (const key of [fingerprint, identity]) {
    const beforeKeys = before.map(key);
    const afterKeys = after.map(key);
    beforeKeys.forEach((k, i) => {
      if (mapping[i] === null && !taken.has(i) && afterKeys[i] === k) {
        mapping[i] = i + 1;
        taken.add(i);
      }
    });
    beforeKeys.forEach((k, i) => {
      if (mapping[i] !== null) return;
      let best = -1;
      afterKeys.forEach((other, j) => {
        if (other === k && !taken.has(j) && (best < 0 || Math.abs(j - i) < Math.abs(best - i))) best = j;
      });
      if (best >= 0) {
        mapping[i] = best + 1;
        taken.add(best);
      }
    });
  }
  return mapping;
}

function omit<T>(record: Record<string, T>, remove: (key: string) => boolean): Record<string, T> {
  return Object.fromEntries(Object.entries(record).filter(([key]) => !remove(key)));
}

/** Path part of an entry key (`<path>#<index>`). */
const keyPath = (key: string) => key.slice(0, key.lastIndexOf('#'));

export function isDirty(tab: Tab, envDrafts: Record<string, Variable[]> = {}): boolean {
  if (tab.kind === 'environments') {
    return Object.keys(envDrafts).some((key) => key.startsWith(`${tab.collection}\n`));
  }
  return tab.draft !== undefined || tab.rawDraft !== undefined;
}

let runCounter = 0;
const newRunId = () => `run-${Date.now()}-${++runCounter}`;
let logCounter = 0;
let orphanCounter = 0;
let runSequence = 0;

interface State {
  ready: boolean;
  appInfo: AppInfo | null;
  /** Options of `hurl`, from the catalog shared with Hurl. */
  hurlOptions: HurlOptions | null;
  /** Predicates of an assert, from the core, which has them from Hurl. */
  hurlPredicates: string[];
  prefs: Prefs;
  collections: Collection[];
  /** Saved collections which could not be read at startup (unplugged drive...). */
  unavailable: string[];
  expanded: Record<string, boolean>;
  files: Record<string, HurlFile>;
  tabs: Tab[];
  activeTabId: string | null;
  activeEnv: Record<string, string>;
  /** Unsaved variables of environments, by `envDraftKey`. */
  envDrafts: Record<string, Variable[]>;
  entryRuns: Record<string, EntryRun>;
  fileRuns: Record<string, FileRun>;
  runners: Record<string, RunnerState>;
  logs: LogItem[];
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  consoleOpen: boolean;

  init(): Promise<void>;
  refreshAppInfo(): Promise<void>;
  setPrefs(patch: Partial<Prefs>): void;
  /** Sets an option of `hurl` in the preferences, or unsets it. */
  setOption(long: string, value: OptionValue | undefined): void;
  setLayout(patch: Partial<Pick<State, 'sidebarWidth' | 'sidebarCollapsed' | 'consoleOpen'>>): void;
  toggle(path: string, open?: boolean): void;

  openCollectionDialog(): Promise<void>;
  createCollectionDialog(): Promise<void>;
  openCollection(path: string): Promise<void>;
  closeCollection(path: string): Promise<void>;
  refreshCollection(path: string): Promise<void>;
  retryCollection(path: string): Promise<void>;
  forgetCollection(path: string): void;

  openEntry(collection: string, path: string, index: number): Promise<void>;
  openFile(collection: string, path: string): Promise<void>;
  openRunner(collection: string, target: string): void;
  /** Runs a folder or a file in its runner, shown in a tab. */
  runTarget(collection: string, target: string): Promise<void>;
  /** Opens a request and sends it. */
  runEntry(collection: string, path: string, index: number): Promise<void>;
  openEnvironments(collection: string): void;
  openPreferences(): void;
  setActiveTab(id: string): void;
  patchTab(id: string, patch: Partial<Tab>): void;
  closeTab(id: string): Promise<void>;
  closeTabs(keep: (tab: Tab) => boolean): Promise<void>;
  updateDraft(id: string, mutate: (entry: Entry) => void): void;
  replaceDraft(id: string, entry: Entry): void;
  setRawDraft(id: string, raw: string): void;
  saveTab(id: string): Promise<boolean>;
  revertTab(id: string): void;

  /** Creates an empty Hurl file and opens it. */
  newFile(collection: string, parent: string, name: string): Promise<void>;
  addEntry(collection: string, path: string, entry: Entry, after?: number): Promise<void>;
  cloneEntry(collection: string, path: string, index: number): Promise<void>;
  deleteEntry(collection: string, path: string, index: number): Promise<void>;
  newFolder(collection: string, parent: string, name: string): Promise<void>;
  renamePath(collection: string, path: string, name: string): Promise<void>;
  cloneFile(collection: string, path: string, name: string): Promise<void>;
  deletePath(collection: string, path: string): Promise<void>;
  reveal(path: string): void;

  setActiveEnv(collection: string, name: string | null): void;
  setEnvDraft(collection: string, name: string, variables: Variable[] | null): void;
  createEnvironment(collection: string, name: string): Promise<boolean>;
  saveEnvironment(collection: string, name: string, variables: Variable[]): Promise<boolean>;
  renameEnvironment(collection: string, name: string, newName: string): Promise<boolean>;
  deleteEnvironment(collection: string, name: string): Promise<void>;

  sendEntry(id: string): Promise<void>;
  runFile(collection: string, path: string): Promise<void>;
  cancelRun(runId: string | undefined): void;
  startRunner(id: string): Promise<void>;
  cancelRunner(id: string): void;
  resetRunner(id: string): void;
  /** Shows the results of one file of a run, or of all of them (`null`). */
  selectRunnerFile(id: string, path: string | null): void;
  clearLogs(): void;
}

/** Latest refresh started for each collection: older scans are discarded. */
const refreshSequence = new Map<string, number>();
let persistTimer: number | undefined;

function persistSoon() {
  window.clearTimeout(persistTimer);
  persistTimer = window.setTimeout(() => {
    const s = useStore.getState();
    if (!s.ready) return;
    const state = {
      // Unavailable collections stay saved: they come back with their drive.
      collections: [...s.collections.map((c) => c.path), ...s.unavailable],
      prefs: s.prefs,
      activeEnv: s.activeEnv,
      expanded: s.expanded,
      sidebarWidth: s.sidebarWidth,
    };
    call('save_state', { state }).catch(toastError);
  }, 400);
}

export const useStore = create<State>((set, get) => {
  const runArgs = () => {
    const p = get().prefs;
    return {
      hurlPath: p.hurlPath,
      options: p.options,
      verbosity: p.verbosity,
      extraArgs: p.extraArgs,
    };
  };
  const hurlPath = () => ({ hurlPath: get().prefs.hurlPath });
  const dirty = (tab: Tab) => isDirty(tab, get().envDrafts);

  const addTab = (tab: Tab) =>
    set((s) =>
      s.tabs.some((t) => t.id === tab.id)
        ? { activeTabId: tab.id }
        : { tabs: [...s.tabs, tab], activeTabId: tab.id },
    );

  const removeTabs = (remove: (tab: Tab) => boolean) =>
    set((s) => {
      const tabs = s.tabs.filter((t) => !remove(t));
      if (tabs.length === s.tabs.length) return {};
      let activeTabId = s.activeTabId;
      if (!tabs.some((t) => t.id === activeTabId)) {
        const position = s.tabs.findIndex((t) => t.id === s.activeTabId);
        const before = s.tabs.slice(0, position).reverse().find((t) => !remove(t));
        activeTabId = (before ?? tabs[tabs.length - 1])?.id ?? null;
      }
      const runners = omit(s.runners, (id) => !tabs.some((t) => t.id === id));
      return { tabs, activeTabId, runners };
    });

  /**
   * Asks before closing tabs holding unsaved edits. Used by the actions which
   * close tabs as a side effect (rename, delete, close a collection).
   */
  const confirmDiscard = async (affected: (tab: Tab) => boolean): Promise<boolean> => {
    const lost = get().tabs.filter((t) => affected(t) && dirty(t));
    if (lost.length === 0) return true;
    const names = lost.map((t) => (t.kind === 'environments' ? 'Environments' : baseName(t.path)));
    return askConfirm(
      'Unsaved changes',
      `${lost.length} tab${lost.length === 1 ? '' : 's'} with unsaved changes will be closed and the edits lost: ${[...new Set(names)].join(', ')}.`,
      'Discard changes',
      true,
    );
  };

  /**
   * Entry tabs and results are attached to a position in their file. After a
   * change of the file, moves them to the new position of their request
   * (`map`: previous index to new index, or null when the request is gone).
   * A tab with unsaved edits is never closed: it is flagged instead, either
   * as conflicting (its request changed on disk) or as orphan (no request to
   * save to any more).
   */
  const reconcile = (path: string, map: (index: number) => number | null, entries: Entry[]) =>
    set((s) => {
      let activeTabId = s.activeTabId;
      const tabs: Tab[] = [];
      for (const tab of s.tabs) {
        if (tab.kind !== 'entry' || tab.path !== path || tab.orphan) {
          tabs.push(tab);
          continue;
        }
        const index = map(tab.index);
        if (index === null) {
          if (!tab.draft) continue;
          const id = `orphan:${++orphanCounter}`;
          if (activeTabId === tab.id) activeTabId = id;
          tabs.push({ ...tab, id, index: 0, orphan: true, conflict: false });
          continue;
        }
        const conflict =
          tab.draft !== undefined &&
          tab.draftBase !== undefined &&
          tab.draftBase !== fingerprint(entries[index - 1]);
        if (index === tab.index && Boolean(tab.conflict) === conflict) {
          tabs.push(tab);
          continue;
        }
        const id = ids.entry(path, index);
        if (activeTabId === tab.id) activeTabId = id;
        tabs.push({ ...tab, id, index, conflict });
      }
      if (activeTabId && !tabs.some((t) => t.id === activeTabId)) {
        activeTabId = tabs[tabs.length - 1]?.id ?? null;
      }
      const prefix = `${path}#`;
      const entryRuns: Record<string, EntryRun> = {};
      for (const [key, run] of Object.entries(s.entryRuns)) {
        const rest = key.startsWith(prefix) ? key.slice(prefix.length) : '';
        if (!/^\d+$/.test(rest)) {
          entryRuns[key] = run;
          continue;
        }
        const index = map(Number(rest));
        if (index !== null) entryRuns[entryKey(path, index)] = run;
      }
      return { tabs, activeTabId, entryRuns };
    });

  /**
   * Stores a file which was read or written and realigns the tabs on it.
   * `map` is given by the operations which know how they moved the entries
   * (insert, delete, save in place); otherwise entries are matched by content.
   */
  const setFile = (file: HurlFile, map?: (index: number) => number | null) => {
    const previous = get().files[file.path];
    set((s) => ({ files: { ...s.files, [file.path]: file } }));
    const count = file.entries.length;
    if (map) {
      reconcile(file.path, map, file.entries);
    } else if (!previous) {
      reconcile(file.path, (i) => (i >= 1 && i <= count ? i : null), file.entries);
    } else if (previous.raw !== file.raw) {
      const matches = matchEntries(previous.entries, file.entries);
      reconcile(file.path, (i) => matches[i - 1] ?? null, file.entries);
    }
    // A text draft started from another version of the file is in conflict.
    set((s) => ({
      tabs: s.tabs.map((t) => {
        if (t.kind !== 'file' || t.path !== file.path || t.rawDraft === undefined) return t;
        const conflict = t.rawBase !== undefined && t.rawBase !== file.raw;
        return Boolean(t.conflict) === conflict ? t : { ...t, conflict };
      }),
    }));
  };

  /** The file disappeared (deleted, renamed, unreadable). */
  const fileGone = (path: string) => {
    set((s) => ({
      files: omit(s.files, (p) => p === path),
      fileRuns: omit(s.fileRuns, (p) => p === path),
    }));
    reconcile(path, () => null, []);
    // A text tab with edits stays: saving it writes the file again.
    removeTabs((t) => t.kind === 'file' && t.path === path && t.rawDraft === undefined);
    set((s) => ({
      tabs: s.tabs.map((t) => (t.kind === 'file' && t.path === path ? { ...t, conflict: true } : t)),
    }));
  };

  /** Forgets what is known of the files under a path which was renamed or deleted. */
  const forgetPaths = (root: string) =>
    set((s) => ({
      files: omit(s.files, (p) => isUnder(p, root)),
      fileRuns: omit(s.fileRuns, (p) => isUnder(p, root)),
      entryRuns: omit(s.entryRuns, (key) => isUnder(keyPath(key), root)),
    }));

  const setEntryRun = (key: string, run: EntryRun) =>
    set((s) => ({ entryRuns: { ...s.entryRuns, [key]: run } }));

  /** Stores the outcome of a `hurl` execution: console log and per entry results. */
  const recordRun = (result: RunResult) => {
    const meta = metaOf(result);
    const output =
      result.stderr.length > MAX_LOG_OUTPUT
        ? `[...]\n${result.stderr.slice(-MAX_LOG_OUTPUT)}`
        : result.stderr;
    const log: LogItem = {
      id: ++logCounter,
      time: Date.now(),
      command: result.command,
      exitCode: result.exitCode,
      durationMs: result.durationMs,
      success: result.success,
      cancelled: result.cancelled,
      output,
    };
    set((s) => {
      const entryRuns = { ...s.entryRuns };
      // A retried entry appears several times: the last attempt wins.
      for (const e of result.entries) {
        entryRuns[entryKey(result.file, e.index)] = { running: false, result: e, meta, seq: ++runSequence };
      }
      // Running a whole collection must not hold every response in memory:
      // only the requests shown in a tab and the latest results keep theirs.
      const shown = new Set(s.tabs.filter((t) => t.kind === 'entry').map((t) => entryKey(t.path, t.index)));
      const full = Object.entries(entryRuns)
        .filter(([key, run]) => run.result && !run.slim && !shown.has(key))
        .sort(([, a], [, b]) => (a.seq ?? 0) - (b.seq ?? 0));
      for (const [key, run] of full.slice(0, Math.max(0, full.length - RECENT_RESULTS))) {
        entryRuns[key] = {
          ...run,
          slim: true,
          result: slimEntry(run.result!),
          meta: run.meta && { ...run.meta, stderr: '' },
        };
      }
      return { entryRuns, logs: [...s.logs.slice(-(MAX_LOGS - 1)), log] };
    });
  };

  const patchRunner = (id: string, patch: Partial<RunnerState>) =>
    set((s) => (s.runners[id] ? { runners: { ...s.runners, [id]: { ...s.runners[id], ...patch } } } : {}));

  const patchRunnerFile = (id: string, path: string, patch: Partial<RunnerFile>) =>
    set((s) => {
      const runner = s.runners[id];
      if (!runner) return {};
      const files = runner.files.map((f) => (f.path === path ? { ...f, ...patch } : f));
      return { runners: { ...s.runners, [id]: { ...runner, files } } };
    });

  const runnerFiles = (collection: string, target: string): RunnerFile[] => {
    const col = get().collections.find((c) => c.path === collection);
    if (!col) return [];
    const node = target === collection ? null : findNode(col.children, target);
    const nodes = node ? [node] : target === collection ? col.children : [];
    return filesUnder(nodes).map((f) => ({ path: f.path, name: f.name, status: 'queued' }));
  };

  /**
   * Brings the files shown in tabs up to date with the scanned collection.
   * Only the files whose stamp changed are read again: an unchanged file
   * costs no call to the core. (A read parses the file again; its hurlfmt
   * syntax check is cached by content in the core, so hurlfmt only runs for
   * a text which was not checked recently.) A file modified in the last
   * seconds gets a new stamp at every scan and is always read again.
   */
  const reloadOpenFiles = async (collection: Collection) => {
    const stamps = new Map(filesUnder(collection.children).map((f) => [f.path, f.stamp]));
    const paths = new Set(
      get()
        .tabs.filter((t) => t.collection === collection.path && (t.kind === 'entry' || t.kind === 'file'))
        .map((t) => t.path),
    );
    for (const path of paths) {
      const stamp = stamps.get(path);
      if (stamp === undefined) {
        fileGone(path);
        continue;
      }
      if (get().files[path]?.stamp === stamp) continue;
      try {
        setFile(await call<HurlFile>('read_file', { path, ...hurlPath() }));
      } catch {
        fileGone(path);
      }
    }
  };

  const addCollection = async (path: string) => {
    const collection = await call<Collection>('scan_collection', { path });
    set((s) => ({
      collections: [...s.collections.filter((c) => c.path !== path), collection],
      unavailable: s.unavailable.filter((p) => p !== path),
      expanded: { ...s.expanded, [path]: true },
    }));
    call('watch_collection', { path }).catch(() => {});
    persistSoon();
  };

  return {
    ready: false,
    appInfo: null,
    hurlOptions: null,
    hurlPredicates: [],
    prefs: DEFAULT_PREFS,
    collections: [],
    unavailable: [],
    expanded: {},
    files: {},
    tabs: [],
    activeTabId: null,
    activeEnv: {},
    envDrafts: {},
    entryRuns: {},
    fileRuns: {},
    runners: {},
    logs: [],
    sidebarWidth: 260,
    sidebarCollapsed: false,
    consoleOpen: false,

    async init() {
      type Saved = Partial<Pick<State, 'prefs' | 'activeEnv' | 'expanded' | 'sidebarWidth'>> & {
        collections?: string[];
      };
      let saved: Saved | null = null;
      try {
        saved = await call<Saved | null>('load_state');
      } catch (e) {
        toastError(e);
      }
      const prefs = loadPrefs(saved?.prefs as Record<string, unknown> | undefined);
      let hurlOptions: HurlOptions | null = null;
      let hurlPredicates: string[] = [];
      try {
        hurlPredicates = await call<string[]>('hurl_predicates');
        hurlOptions = await call<HurlOptions>('hurl_options');
        // Saved by another version of the application: an option which does
        // not exist here would be refused at every run.
        prefs.options = knownOptions(prefs.options, hurlOptions);
      } catch (e) {
        toastError(e);
      }
      applyTheme(prefs.theme);
      window
        .matchMedia('(prefers-color-scheme: dark)')
        .addEventListener('change', () => applyTheme(get().prefs.theme));
      set({
        prefs,
        hurlOptions,
        hurlPredicates,
        activeEnv: saved?.activeEnv ?? {},
        expanded: saved?.expanded ?? {},
        sidebarWidth: saved?.sidebarWidth ?? 260,
      });
      await get().refreshAppInfo();
      const collections: Collection[] = [];
      const unavailable: string[] = [];
      for (const path of new Set(saved?.collections ?? [])) {
        try {
          collections.push(await call<Collection>('scan_collection', { path }));
          call('watch_collection', { path }).catch(() => {});
        } catch {
          // Not reachable right now (network or removable drive, permissions):
          // kept in the workspace until the user removes it.
          unavailable.push(path);
        }
      }
      set({ collections, unavailable, ready: true });
      onEvent<{ path: string }>('collection-changed', (p) => void get().refreshCollection(p.path));
    },

    async refreshAppInfo() {
      try {
        set({ appInfo: await call<AppInfo>('app_info', hurlPath()) });
      } catch (e) {
        toastError(e);
      }
    },

    setPrefs(patch) {
      set((s) => ({ prefs: { ...s.prefs, ...patch } }));
      if (patch.theme) applyTheme(patch.theme);
      if (patch.hurlPath !== undefined) void get().refreshAppInfo();
      persistSoon();
    },

    setOption(long, value) {
      const { [long]: _, ...options } = get().prefs.options;
      get().setPrefs({ options: value !== undefined && isSet(value) ? { ...options, [long]: value } : options });
    },

    setLayout(patch) {
      set(patch);
      persistSoon();
    },

    toggle(path, open) {
      set((s) => ({ expanded: { ...s.expanded, [path]: open ?? !s.expanded[path] } }));
      persistSoon();
    },

    async openCollectionDialog() {
      const path = await pickDirectory('Open Folder');
      if (path) await get().openCollection(path);
    },

    async createCollectionDialog() {
      const parent = await pickDirectory('Location of the new folder');
      if (!parent) return;
      const name = await askText('Create Folder', 'Name', '', 'Create');
      if (!name) return;
      try {
        const path = await call<string>('create_collection', { parent, name });
        await get().openCollection(path);
      } catch (e) {
        toastError(e);
      }
    },

    async openCollection(path) {
      if (get().collections.some((c) => c.path === path)) {
        get().toggle(path, true);
        return;
      }
      try {
        await addCollection(path);
      } catch (e) {
        toastError(e);
      }
    },

    async closeCollection(path) {
      if (!(await confirmDiscard((t) => t.collection === path))) return;
      removeTabs((t) => t.collection === path);
      forgetPaths(path);
      set((s) => ({
        collections: s.collections.filter((c) => c.path !== path),
        envDrafts: omit(s.envDrafts, (key) => key.startsWith(`${path}\n`)),
      }));
      call('unwatch_collection', { path }).catch(() => {});
      persistSoon();
    },

    async refreshCollection(path) {
      if (!get().collections.some((c) => c.path === path)) return;
      const sequence = (refreshSequence.get(path) ?? 0) + 1;
      refreshSequence.set(path, sequence);
      try {
        const collection = await call<Collection>('scan_collection', { path });
        // A newer refresh was started meanwhile: its result is the one to show.
        if (refreshSequence.get(path) !== sequence) return;
        set((s) => ({
          collections: s.collections.map((c) => (c.path === path ? collection : c)),
          // Drafts of environments which no longer exist have nowhere to go.
          envDrafts: omit(
            s.envDrafts,
            (key) =>
              key.startsWith(`${path}\n`) &&
              !collection.environments.some((e) => envDraftKey(path, e.name) === key),
          ),
        }));
        const active = get().activeEnv[path];
        if (active && !collection.environments.some((e) => e.name === active)) {
          get().setActiveEnv(path, null);
        }
        await reloadOpenFiles(collection);
      } catch (e) {
        toastError(e);
      }
    },

    async retryCollection(path) {
      try {
        await addCollection(path);
      } catch (e) {
        toastError(e);
      }
    },

    forgetCollection(path) {
      set((s) => ({ unavailable: s.unavailable.filter((p) => p !== path) }));
      persistSoon();
    },

    async openEntry(collection, path, index) {
      let file: HurlFile;
      try {
        // Always read again: the file may have changed while no tab showed it.
        file = await call<HurlFile>('read_file', { path, ...hurlPath() });
        setFile(file);
      } catch (e) {
        toastError(e);
        return;
      }
      if (index > file.entries.length) {
        toast(`"${baseName(path)}" changed: this request no longer exists.`, 'error');
        void get().refreshCollection(collection);
        return;
      }
      addTab({
        id: ids.entry(path, index),
        kind: 'entry',
        collection,
        path,
        index,
        pane: 'params',
        responsePane: 'response',
      });
    },

    async openFile(collection, path) {
      try {
        setFile(await call<HurlFile>('read_file', { path, ...hurlPath() }));
      } catch (e) {
        toastError(e);
        return;
      }
      addTab({ id: ids.file(path), kind: 'file', collection, path, index: 0, pane: '', responsePane: '' });
    },

    openRunner(collection, target) {
      const id = ids.runner(target);
      if (!get().runners[id]) {
        set((s) => ({
          runners: {
            ...s.runners,
            [id]: { status: 'idle', files: [], cancelRequested: false, selected: null, runId: null },
          },
        }));
      }
      addTab({ id, kind: 'runner', collection, path: target, index: 0, pane: '', responsePane: '' });
    },

    async runTarget(collection, target) {
      get().openRunner(collection, target);
      await get().startRunner(ids.runner(target));
    },

    async runEntry(collection, path, index) {
      await get().openEntry(collection, path, index);
      const tab = get().tabs.find((t) => t.kind === 'entry' && t.path === path && t.index === index);
      if (tab) await get().sendEntry(tab.id);
    },

    openEnvironments(collection) {
      addTab({
        id: ids.environments(collection),
        kind: 'environments',
        collection,
        path: '',
        index: 0,
        pane: '',
        responsePane: '',
      });
    },

    openPreferences() {
      addTab({
        id: ids.preferences,
        kind: 'preferences',
        collection: '',
        path: '',
        index: 0,
        pane: '',
        responsePane: '',
      });
    },

    setActiveTab(id) {
      set({ activeTabId: id });
    },

    patchTab(id, patch) {
      set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)) }));
    },

    async closeTab(id) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab) return;
      if (dirty(tab)) {
        const name = tab.kind === 'environments' ? 'Environments' : baseName(tab.path);
        const discard = await askConfirm(
          'Unsaved changes',
          `"${name}" has unsaved changes. Close it and lose them?`,
          'Discard changes',
          true,
        );
        if (!discard) return;
      }
      removeTabs((t) => t.id === id);
      if (tab.kind === 'environments') {
        set((s) => ({ envDrafts: omit(s.envDrafts, (key) => key.startsWith(`${tab.collection}\n`)) }));
      }
    },

    async closeTabs(keep) {
      for (const tab of get().tabs.filter((t) => !keep(t))) {
        await get().closeTab(tab.id);
      }
    },

    updateDraft(id, mutate) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.kind !== 'entry') return;
      const base = tab.draft ?? get().files[tab.path]?.entries[tab.index - 1];
      if (!base) return;
      const draft = structuredClone(base);
      mutate(draft);
      get().patchTab(id, tab.draft ? { draft } : { draft, draftBase: fingerprint(base) });
    },

    replaceDraft(id, entry) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.kind !== 'entry') return;
      const source = get().files[tab.path]?.entries[tab.index - 1];
      const draftBase = tab.draftBase ?? (source ? fingerprint(source) : undefined);
      get().patchTab(id, { draft: entry, draftBase });
    },

    setRawDraft(id, raw) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.kind !== 'file') return;
      const file = get().files[tab.path];
      if (file && raw === file.raw) {
        get().patchTab(id, { rawDraft: undefined, rawBase: undefined, conflict: false });
      } else {
        get().patchTab(id, { rawDraft: raw, rawBase: tab.rawBase ?? file?.raw });
      }
    },

    async saveTab(id) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab) return false;
      const overwrite = () =>
        askConfirm(
          'File changed on disk',
          `"${baseName(tab.path)}" changed on disk since you started editing. Save anyway and overwrite that change?`,
          'Overwrite',
          true,
        );
      try {
        let file: HurlFile;
        if (tab.kind === 'entry' && tab.draft) {
          if (tab.orphan) {
            toast(
              'This request no longer exists in the file, so its edits cannot be saved. Copy what you need, then close the tab.',
              'error',
            );
            return false;
          }
          if (tab.conflict && !(await overwrite())) return false;
          const sent = tab.draft;
          file = await call<HurlFile>('save_entry', {
            path: tab.path,
            index: tab.index,
            entry: sent,
            ...hurlPath(),
          });
          // Saving in place does not move the entries.
          setFile(file, (i) => i);
          // Edits made while the save was in flight stay, on top of what was saved.
          const typedMeanwhile = get().tabs.find((t) => t.id === id)?.draft !== sent;
          get().patchTab(
            id,
            typedMeanwhile
              ? { draftBase: fingerprint(file.entries[tab.index - 1]), conflict: false }
              : { draft: undefined, draftBase: undefined, conflict: false },
          );
        } else if (tab.kind === 'file' && tab.rawDraft !== undefined) {
          const onDisk = get().files[tab.path];
          const changed = !onDisk || (tab.rawBase !== undefined && onDisk.raw !== tab.rawBase);
          if (changed && !(await overwrite())) return false;
          const sent = tab.rawDraft;
          file = await call<HurlFile>('save_raw', { path: tab.path, raw: sent, ...hurlPath() });
          setFile(file);
          const typedMeanwhile = get().tabs.find((t) => t.id === id)?.rawDraft !== sent;
          get().patchTab(
            id,
            typedMeanwhile
              ? { rawBase: file.raw, conflict: false }
              : { rawDraft: undefined, rawBase: undefined, conflict: false },
          );
        } else {
          return true;
        }
        if (file.syntaxError) toast('Saved, but the file has a syntax error (see the banner).', 'error');
        void get().refreshCollection(tab.collection);
        return true;
      } catch (e) {
        toastError(e);
        return false;
      }
    },

    revertTab(id) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab) return;
      if (tab.kind === 'environments') {
        set((s) => ({ envDrafts: omit(s.envDrafts, (key) => key.startsWith(`${tab.collection}\n`)) }));
        return;
      }
      if (tab.orphan || (tab.kind === 'file' && !get().files[tab.path])) {
        // Nothing left on disk to go back to.
        removeTabs((t) => t.id === id);
        return;
      }
      get().patchTab(id, {
        draft: undefined,
        draftBase: undefined,
        rawDraft: undefined,
        rawBase: undefined,
        conflict: false,
      });
    },

    async newFile(collection, parent, name) {
      try {
        const path = await call<string>('create_file', { parent, name, entry: null });
        set((s) => ({ expanded: { ...s.expanded, [parent]: true, [path]: true } }));
        await get().refreshCollection(collection);
        // No request yet: the file is opened as text, ready to be written.
        await get().openFile(collection, path);
        persistSoon();
      } catch (e) {
        toastError(e);
      }
    },

    async addEntry(collection, path, entry, after) {
      try {
        const file = await call<HurlFile>('add_entry', { path, entry, after: after ?? null, ...hurlPath() });
        // The entries after the insertion point move down by one: so do their
        // tabs (unsaved edits included) and results.
        setFile(file, (i) => (after === undefined || i <= after ? i : i + 1));
        set((s) => ({ expanded: { ...s.expanded, [path]: true } }));
        await get().refreshCollection(collection);
        await get().openEntry(collection, path, after !== undefined ? after + 1 : file.entries.length);
      } catch (e) {
        toastError(e);
      }
    },

    async cloneEntry(collection, path, index) {
      try {
        const file = await call<HurlFile>('read_file', { path, ...hurlPath() });
        const source = file.entries[index - 1];
        if (!source) return;
        const name = entryName(source.docs);
        const docs = name ? source.docs.replace(name, `${name} (copy)`) : source.docs;
        await get().addEntry(collection, path, { ...source, docs }, index);
      } catch (e) {
        toastError(e);
      }
    },

    async deleteEntry(collection, path, index) {
      const ok = await askConfirm(
        'Delete request',
        `Delete request ${index} of "${baseName(path)}"? This rewrites the file.`,
        'Delete',
        true,
      );
      if (!ok) return;
      try {
        const file = await call<HurlFile>('delete_entry', { path, index, ...hurlPath() });
        // The tab of the deleted request goes with it; the following ones move up.
        removeTabs((t) => t.kind === 'entry' && t.path === path && t.index === index);
        setFile(file, (i) => (i < index ? i : i === index ? null : i - 1));
        await get().refreshCollection(collection);
      } catch (e) {
        toastError(e);
      }
    },

    async newFolder(collection, parent, name) {
      try {
        await call('create_folder', { parent, name });
        get().toggle(parent, true);
        await get().refreshCollection(collection);
      } catch (e) {
        toastError(e);
      }
    },

    async renamePath(collection, path, name) {
      const affected = (t: Tab) => t.collection === collection && t.path !== '' && isUnder(t.path, path);
      if (!(await confirmDiscard(affected))) return;
      try {
        await call('rename_path', { path, name });
        removeTabs(affected);
        forgetPaths(path);
        await get().refreshCollection(collection);
      } catch (e) {
        toastError(e);
      }
    },

    async cloneFile(collection, path, name) {
      try {
        await call('clone_file', { path, name });
        await get().refreshCollection(collection);
      } catch (e) {
        toastError(e);
      }
    },

    async deletePath(collection, path) {
      const ok = await askConfirm(
        'Delete',
        `Delete "${baseName(path)}" from the disk? This cannot be undone.`,
        'Delete',
        true,
      );
      if (!ok) return;
      try {
        await call('delete_path', { path });
        // Their file is gone: nothing could be saved from these tabs.
        removeTabs((t) => t.collection === collection && t.path !== '' && isUnder(t.path, path));
        forgetPaths(path);
        await get().refreshCollection(collection);
      } catch (e) {
        toastError(e);
      }
    },

    reveal(path) {
      call('reveal_path', { path }).catch(toastError);
    },

    setActiveEnv(collection, name) {
      set((s) => {
        const activeEnv = { ...s.activeEnv };
        if (name) activeEnv[collection] = name;
        else delete activeEnv[collection];
        return { activeEnv };
      });
      persistSoon();
    },

    setEnvDraft(collection, name, variables) {
      const key = envDraftKey(collection, name);
      set((s) => ({
        envDrafts: variables ? { ...s.envDrafts, [key]: variables } : omit(s.envDrafts, (k) => k === key),
      }));
    },

    async createEnvironment(collection, name) {
      try {
        // Refused when the name is taken: never replaces an environment.
        await call('create_environment', { collection, name });
        await get().refreshCollection(collection);
        return true;
      } catch (e) {
        toastError(e);
        return false;
      }
    },

    async saveEnvironment(collection, name, variables) {
      try {
        await call('save_environment', { collection, name, variables });
        get().setEnvDraft(collection, name, null);
        await get().refreshCollection(collection);
        return true;
      } catch (e) {
        toastError(e);
        return false;
      }
    },

    async renameEnvironment(collection, name, newName) {
      try {
        await call('rename_environment', { collection, name, newName });
        const draft = get().envDrafts[envDraftKey(collection, name)];
        get().setEnvDraft(collection, name, null);
        if (draft) get().setEnvDraft(collection, newName, draft);
        if (get().activeEnv[collection] === name) get().setActiveEnv(collection, newName);
        await get().refreshCollection(collection);
        return true;
      } catch (e) {
        toastError(e);
        return false;
      }
    },

    async deleteEnvironment(collection, name) {
      const ok = await askConfirm(
        'Delete environment',
        `Delete the environment "${name}" and its secrets?`,
        'Delete',
        true,
      );
      if (!ok) return;
      try {
        await call('delete_environment', { collection, name });
        get().setEnvDraft(collection, name, null);
        await get().refreshCollection(collection);
      } catch (e) {
        toastError(e);
      }
    },

    async sendEntry(id) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.kind !== 'entry' || tab.orphan) return;
      const key = entryKey(tab.path, tab.index);
      const previous = get().entryRuns[key];
      if (previous?.running) return;
      const runId = newRunId();
      setEntryRun(key, { ...previous, running: true, runId });
      try {
        // Hurl runs the file on disk: pending edits are saved first.
        if (tab.draft && !(await get().saveTab(id))) {
          setEntryRun(key, { ...previous, running: false });
          return;
        }
        const { prefs, activeEnv } = get();
        const result = await call<RunResult>('run', {
          runId,
          file: tab.path,
          collection: tab.collection,
          environment: activeEnv[tab.collection] ?? null,
          fromEntry: prefs.chainEntries ? null : tab.index,
          toEntry: tab.index,
          ...runArgs(),
        });
        recordRun(result);
        if (!result.entries.some((e) => e.index === tab.index)) {
          setEntryRun(key, { running: false, meta: metaOf(result) });
        }
      } catch (e) {
        setEntryRun(key, { running: false, meta: failedMeta(runId, tab.path, e) });
      }
    },

    async runFile(collection, path) {
      if (get().fileRuns[path]?.running) return;
      const runId = newRunId();
      const setRun = (run: FileRun) => set((s) => ({ fileRuns: { ...s.fileRuns, [path]: run } }));
      setRun({ ...get().fileRuns[path], running: true, runId });
      try {
        // Unsaved edits of this file would not be part of the run.
        for (const t of get().tabs.filter((t) => t.path === path && (t.draft || t.rawDraft !== undefined))) {
          if (!(await get().saveTab(t.id))) {
            setRun({ running: false });
            return;
          }
        }
        const result = await call<RunResult>('run', {
          runId,
          file: path,
          collection,
          environment: get().activeEnv[collection] ?? null,
          ...runArgs(),
        });
        recordRun(result);
        setRun({ running: false, result: slimResult(result) });
      } catch (e) {
        setRun({ running: false, result: { ...failedMeta(runId, path, e), entries: [] } });
      }
    },

    cancelRun(runId) {
      if (runId) call('cancel_run', { runId }).catch(toastError);
    },

    async startRunner(id) {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || get().runners[id]?.status === 'running') return;
      // Unsaved edits would not be part of the run.
      for (const t of get().tabs) {
        const edited = t.draft !== undefined || t.rawDraft !== undefined;
        if (edited && isUnder(t.path, tab.path) && !(await get().saveTab(t.id))) return;
      }
      const files = runnerFiles(tab.collection, tab.path);
      patchRunner(id, { status: 'running', files, cancelRequested: false, selected: null });
      for (const file of files) {
        if (!get().runners[id] || get().runners[id].cancelRequested) break;
        const runId = newRunId();
        patchRunner(id, { runId });
        patchRunnerFile(id, file.path, { status: 'running' });
        let result: RunResult;
        try {
          result = await call<RunResult>('run', {
            runId,
            file: file.path,
            collection: tab.collection,
            environment: get().activeEnv[tab.collection] ?? null,
            ...runArgs(),
          });
          recordRun(result);
        } catch (e) {
          result = { ...failedMeta(runId, file.path, e), entries: [] };
        }
        const status = result.cancelled ? 'cancelled' : result.success ? 'passed' : 'failed';
        patchRunnerFile(id, file.path, { status, result: slimResult(result) });
      }
      const runner = get().runners[id];
      if (!runner) return;
      patchRunner(id, {
        status: 'done',
        runId: null,
        files: runner.files.map((f) => (f.status === 'queued' ? { ...f, status: 'cancelled' } : f)),
      });
    },

    cancelRunner(id) {
      const runner = get().runners[id];
      if (!runner) return;
      patchRunner(id, { cancelRequested: true });
      get().cancelRun(runner.runId ?? undefined);
    },

    resetRunner(id) {
      patchRunner(id, { status: 'idle', files: [], cancelRequested: false, selected: null, runId: null });
    },

    selectRunnerFile(id, path) {
      patchRunner(id, { selected: path });
    },

    clearLogs() {
      set({ logs: [] });
    },
  };
});

/** Entry shown by an entry tab: its draft, or the entry of the loaded file. */
export function entryOfTab(state: Pick<State, 'files'>, tab: Tab): Entry | undefined {
  return tab.draft ?? state.files[tab.path]?.entries[tab.index - 1];
}
