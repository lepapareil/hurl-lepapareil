// Left sidebar: the opened folders, their Hurl files and their requests.

import {
  IconAlertTriangle,
  IconChevronRight,
  IconCopy,
  IconDatabase,
  IconDots,
  IconFileText,
  IconFolder,
  IconFolderOpen,
  IconFolderPlus,
  IconPencil,
  IconPlayerPlay,
  IconPlayerPlayFilled,
  IconPlayerStopFilled,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconTrash,
  IconX,
} from '@tabler/icons-react';
import { memo, useDeferredValue, useMemo, useState, type MouseEvent, type ReactNode } from 'react';
import { baseName, entryKey, ids, useStore } from '../store';
import { emptyEntry, type Collection, type EntrySummary, type TreeNode } from '../types';
import { askForm, askText, showMenu, type MenuItem } from '../ui';
import { Dropdown, VirtualList } from './ui';

export const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];
const ICON = { size: 15, stroke: 1.5 };

export function methodClass(method: string): string {
  return `method method-${method.toLowerCase()}`;
}

/** `{{base_url}}/users` and `https://host/users` are both shown as `/users`. */
export function displayUrl(url: string): string {
  const m = /^(?:[a-z]+:\/\/[^/]+|\{\{[^}]+\}\})(\/.*)$/i.exec(url);
  return m ? m[1] : url;
}

/** Creates an empty Hurl file: its requests are added afterwards, from the file. */
export async function newFileInFolder(collection: string, parent: string) {
  const name = await askText('New Hurl File', 'File name', '', 'Create');
  if (name?.trim()) await useStore.getState().newFile(collection, parent, name.trim());
}

export async function newRequestInFile(collection: string, path: string) {
  const v = await askForm({
    title: `New Request in ${baseName(path)}`,
    confirmLabel: 'Add',
    fields: [
      { name: 'name', label: 'Name (optional)', placeholder: 'Create a user', optional: true },
      { name: 'method', label: 'Method', options: METHODS, initial: 'GET' },
      { name: 'url', label: 'URL', placeholder: 'https://example.org/api/users' },
    ],
  });
  if (!v) return;
  const entry = { ...emptyEntry(), docs: v.name.trim(), method: v.method, url: v.url.trim() };
  await useStore.getState().addEntry(collection, path, entry);
}

async function newFolder(collection: string, parent: string) {
  const name = await askText('New Folder', 'Folder name', '', 'Create');
  if (name) await useStore.getState().newFolder(collection, parent, name);
}

async function rename(collection: string, path: string) {
  const current = baseName(path).replace(/\.hurl$/i, '');
  const name = await askText('Rename', 'New name', current, 'Rename');
  if (name && name !== current) await useStore.getState().renamePath(collection, path, name);
}

async function cloneFile(collection: string, path: string) {
  const current = baseName(path).replace(/\.hurl$/i, '');
  const name = await askText('Clone File', 'Name of the copy', `${current}-copy`, 'Clone');
  if (name) await useStore.getState().cloneFile(collection, path, name);
}

function collectionMenu(c: Collection): MenuItem[] {
  const s = useStore.getState();
  return [
    { label: 'New Hurl File', icon: <IconPlus {...ICON} />, onClick: () => newFileInFolder(c.path, c.path) },
    { label: 'New Folder', icon: <IconFolderPlus {...ICON} />, onClick: () => newFolder(c.path, c.path) },
    { label: 'Run', icon: <IconPlayerPlay {...ICON} />, onClick: () => s.openRunner(c.path, c.path) },
    { label: 'Environments', icon: <IconDatabase {...ICON} />, onClick: () => s.openEnvironments(c.path) },
    { label: 'Show in File Manager', icon: <IconFolderOpen {...ICON} />, onClick: () => s.reveal(c.path) },
    { label: 'Refresh', icon: <IconRefresh {...ICON} />, onClick: () => s.refreshCollection(c.path) },
    { separator: true },
    { label: 'Close', icon: <IconX {...ICON} />, onClick: () => s.closeCollection(c.path) },
  ];
}

function folderMenu(collection: string, path: string): MenuItem[] {
  const s = useStore.getState();
  return [
    { label: 'New Hurl File', icon: <IconPlus {...ICON} />, onClick: () => newFileInFolder(collection, path) },
    { label: 'New Folder', icon: <IconFolderPlus {...ICON} />, onClick: () => newFolder(collection, path) },
    { label: 'Run', icon: <IconPlayerPlay {...ICON} />, onClick: () => s.openRunner(collection, path) },
    { label: 'Rename', icon: <IconPencil {...ICON} />, onClick: () => rename(collection, path) },
    { label: 'Show in File Manager', icon: <IconFolderOpen {...ICON} />, onClick: () => s.reveal(path) },
    { separator: true },
    { label: 'Delete', icon: <IconTrash {...ICON} />, danger: true, onClick: () => s.deletePath(collection, path) },
  ];
}

function fileMenu(collection: string, path: string): MenuItem[] {
  const s = useStore.getState();
  return [
    { label: 'New Request', icon: <IconPlus {...ICON} />, onClick: () => newRequestInFile(collection, path) },
    { label: 'Run', icon: <IconPlayerPlay {...ICON} />, onClick: () => s.openRunner(collection, path) },
    { label: 'Edit as Text', icon: <IconFileText {...ICON} />, onClick: () => s.openFile(collection, path) },
    { label: 'Rename', icon: <IconPencil {...ICON} />, onClick: () => rename(collection, path) },
    { label: 'Clone', icon: <IconCopy {...ICON} />, onClick: () => cloneFile(collection, path) },
    { label: 'Show in File Manager', icon: <IconFolderOpen {...ICON} />, onClick: () => s.reveal(path) },
    { separator: true },
    { label: 'Delete', icon: <IconTrash {...ICON} />, danger: true, onClick: () => s.deletePath(collection, path) },
  ];
}

function entryMenu(collection: string, path: string, index: number): MenuItem[] {
  const s = useStore.getState();
  return [
    { label: 'Run', icon: <IconPlayerPlay {...ICON} />, onClick: () => s.runEntry(collection, path, index) },
    { label: 'Clone', icon: <IconCopy {...ICON} />, onClick: () => s.cloneEntry(collection, path, index) },
    { label: 'Edit File as Text', icon: <IconFileText {...ICON} />, onClick: () => s.openFile(collection, path) },
    { separator: true },
    {
      label: 'Delete',
      icon: <IconTrash {...ICON} />,
      danger: true,
      onClick: () => s.deleteEntry(collection, path, index),
    },
  ];
}

function matches(text: string, query: string): boolean {
  return text.toLowerCase().includes(query);
}

/** Keeps the nodes (and the requests) matching the search. */
function filterNodes(nodes: TreeNode[], query: string): TreeNode[] {
  if (!query) return nodes;
  return nodes.flatMap((n): TreeNode[] => {
    if (n.type === 'folder') {
      const children = filterNodes(n.children, query);
      return children.length || matches(n.name, query) ? [{ ...n, children }] : [];
    }
    if (matches(n.name, query)) return [n];
    const entries = n.entries.filter((e) => matches(`${e.method} ${e.name} ${e.url}`, query));
    return entries.length ? [{ ...n, entries }] : [];
  });
}

// --- Visible rows -----------------------------------------------------------------

/**
 * Height of every row of the tree, in pixels. The tree is windowed (only the
 * rows in view are mounted), which needs rows of one known height: the styles
 * get this value through `--row-height` (see `VirtualList`).
 */
export const TREE_ROW_HEIGHT = 26;

/** One line of the tree, as shown: the tree flattened along what is expanded. */
type TreeRow = { key: string; depth: number } & (
  | { kind: 'collection'; collection: Collection; open: boolean }
  | { kind: 'folder'; collection: string; node: Extract<TreeNode, { type: 'folder' }>; open: boolean }
  | { kind: 'file'; collection: string; node: Extract<TreeNode, { type: 'file' }>; open: boolean }
  | { kind: 'entry'; collection: string; path: string; entry: EntrySummary }
  | { kind: 'hint'; text: string }
  | { kind: 'unavailable'; path: string }
);

/**
 * Rows of the tree from top to bottom: each collection, followed when it is
 * open by its content (the children of a folder and the requests of a file
 * come right under it, one level deeper), then the unavailable collections.
 * A search keeps the matching rows only and opens everything.
 */
function visibleRows(
  collections: Collection[],
  unavailable: string[],
  expanded: Record<string, boolean>,
  query: string,
): TreeRow[] {
  const rows: TreeRow[] = [];
  const forceOpen = query !== '';
  const isOpen = (path: string) => forceOpen || Boolean(expanded[path]);

  for (const collection of collections) {
    // The same folder can be shown by two collections (one opened inside another).
    const key = (kind: string, id: string) => `${collection.path}\n${kind}\n${id}`;
    const addNodes = (nodes: TreeNode[], depth: number) => {
      for (const node of nodes) {
        const open = isOpen(node.path);
        if (node.type === 'folder') {
          rows.push({ kind: 'folder', key: key('folder', node.path), depth, collection: collection.path, node, open });
          if (open) addNodes(node.children, depth + 1);
          continue;
        }
        rows.push({ kind: 'file', key: key('file', node.path), depth, collection: collection.path, node, open });
        if (!open) continue;
        for (const entry of node.entries) {
          rows.push({
            kind: 'entry',
            key: key('entry', entryKey(node.path, entry.index)),
            depth: depth + 1,
            collection: collection.path,
            path: node.path,
            entry,
          });
        }
      }
    };

    const open = isOpen(collection.path);
    rows.push({ kind: 'collection', key: key('collection', ''), depth: 0, collection, open });
    if (!open) continue;
    const children = filterNodes(collection.children, query);
    if (children.length) {
      addNodes(children, 1);
    } else {
      const text = query ? 'No matching request' : 'No Hurl file yet. Right-click to add one.';
      rows.push({ kind: 'hint', key: key('hint', ''), depth: 1, text });
    }
  }
  // A search is about requests: these have none to show.
  if (query === '') {
    for (const path of unavailable) rows.push({ kind: 'unavailable', key: `\nunavailable\n${path}`, depth: 0, path });
  }
  return rows;
}

// --- Rows -------------------------------------------------------------------------

function Row(props: {
  depth: number;
  active?: boolean;
  title?: string;
  onClick: () => void;
  onDoubleClick?: () => void;
  menu: () => MenuItem[];
  children: ReactNode;
}) {
  const openMenu = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    showMenu(e, props.menu());
  };
  return (
    <div
      className={`tree-row ${props.active ? 'active' : ''}`}
      style={{ paddingLeft: 8 + props.depth * 16 }}
      title={props.title}
      onClick={props.onClick}
      onDoubleClick={props.onDoubleClick}
      onContextMenu={openMenu}
      role="treeitem"
    >
      {props.children}
      <button type="button" className="row-menu" onClick={openMenu} aria-label="Actions">
        <IconDots size={16} stroke={1.5} />
      </button>
    </div>
  );
}

function Chevron({ open }: { open: boolean }) {
  return <IconChevronRight size={14} stroke={1.5} className={`chevron ${open ? 'open' : ''}`} />;
}

/** Button in front of the name of a row: runs it, or stops its run. */
function RunButton(props: { what: string; running: boolean; onRun: () => void; onStop: () => void }) {
  const label = props.running ? `Stop the run of ${props.what}` : `Run ${props.what}`;
  return (
    <button
      type="button"
      className={`row-run ${props.running ? 'running' : ''}`}
      title={label}
      aria-label={label}
      onClick={(e) => {
        // The row itself opens or folds what it shows.
        e.stopPropagation();
        if (props.running) props.onStop();
        else props.onRun();
      }}
      onDoubleClick={(e) => e.stopPropagation()}
    >
      {props.running ? <IconPlayerStopFilled size={12} /> : <IconPlayerPlayFilled size={12} />}
    </button>
  );
}

/** Runs a folder or a file, in its runner. Subscribes to the state of this runner only. */
function RunTargetButton({ collection, path, what }: { collection: string; path: string; what: string }) {
  const id = ids.runner(path);
  const running = useStore((s) => s.runners[id]?.status === 'running');
  const { runTarget, cancelRunner } = useStore.getState();
  return (
    <RunButton
      what={what}
      running={running}
      onRun={() => void runTarget(collection, path)}
      onStop={() => cancelRunner(id)}
    />
  );
}

/** Subscribes to its own state (shown in the active tab, last result): a run only renders its row. */
function EntryRow(props: { collection: string; path: string; entry: EntrySummary; depth: number }) {
  const { collection, path, entry } = props;
  const active = useStore((s) => {
    const tab = s.tabs.find((t) => t.id === s.activeTabId);
    return tab?.kind === 'entry' && tab.path === path && tab.index === entry.index;
  });
  const run = useStore((s) => s.entryRuns[entryKey(path, entry.index)]);
  const openEntry = useStore((s) => s.openEntry);
  const { runEntry, cancelRun } = useStore.getState();
  const failed = run?.result ? run.result.failed : run?.meta ? !run.meta.success : false;
  return (
    <Row
      depth={props.depth}
      active={active}
      title={`${entry.method} ${entry.url}`}
      onClick={() => openEntry(collection, path, entry.index)}
      menu={() => entryMenu(collection, path, entry.index)}
    >
      {/* A request has nothing to unfold: the room of the chevron is kept, so
          that it is one level under its file. */}
      <span className="chevron-space" />
      <RunButton
        what="this request"
        running={Boolean(run?.running)}
        onRun={() => void runEntry(collection, path, entry.index)}
        onStop={() => cancelRun(run?.runId)}
      />
      <span className={methodClass(entry.method)}>
        {entry.method.length > 5 ? entry.method.slice(0, 3) : entry.method}
      </span>
      <span className="tree-label">{entry.name || displayUrl(entry.url)}</span>
      {run && !run.running && (run.result || run.meta) && (
        <span className={`run-dot ${failed ? 'failed' : 'passed'}`} title={failed ? 'Failed' : 'Passed'} />
      )}
    </Row>
  );
}

function UnavailableCollection({ path }: { path: string }) {
  const { retryCollection, forgetCollection } = useStore.getState();
  return (
    <Row
      depth={0}
      title={`${path}\nThis folder cannot be read right now. Click to try again.`}
      onClick={() => retryCollection(path)}
      menu={() => [
        { label: 'Try Again', icon: <IconRefresh {...ICON} />, onClick: () => retryCollection(path) },
        { separator: true },
        { label: 'Remove from Workspace', icon: <IconX {...ICON} />, onClick: () => forgetCollection(path) },
      ]}
    >
      <IconAlertTriangle size={15} stroke={1.5} className="tree-icon failed" />
      <span className="tree-label muted">{baseName(path)}</span>
      <span className="tree-count">unavailable</span>
    </Row>
  );
}

/**
 * One row of the tree. Memoized: the rows keep their identity as long as the
 * tree, what is expanded and the search do not change, so scrolling only
 * renders the rows which enter the window.
 */
const TreeRowView = memo(function TreeRowView({ row }: { row: TreeRow }) {
  const { toggle, openFile } = useStore.getState();
  switch (row.kind) {
    case 'collection': {
      const { collection } = row;
      return (
        <Row
          depth={row.depth}
          title={collection.path}
          onClick={() => toggle(collection.path)}
          menu={() => collectionMenu(collection)}
        >
          <Chevron open={row.open} />
          <RunTargetButton collection={collection.path} path={collection.path} what="this folder" />
          <IconFolder size={15} stroke={1.5} className="tree-icon" />
          <span className="tree-label collection-name">{collection.name}</span>
        </Row>
      );
    }
    case 'folder': {
      const { collection, node } = row;
      return (
        <Row depth={row.depth} onClick={() => toggle(node.path)} menu={() => folderMenu(collection, node.path)}>
          <Chevron open={row.open} />
          <RunTargetButton collection={collection} path={node.path} what="this folder" />
          <IconFolder size={15} stroke={1.5} className="tree-icon" />
          <span className="tree-label">{node.name}</span>
        </Row>
      );
    }
    case 'file': {
      const { collection, node } = row;
      return (
        <Row
          depth={row.depth}
          title={`${node.path}\nDouble-click to edit the file as text`}
          onClick={() => toggle(node.path)}
          onDoubleClick={() => openFile(collection, node.path)}
          menu={() => fileMenu(collection, node.path)}
        >
          <Chevron open={row.open} />
          <RunTargetButton collection={collection} path={node.path} what="this file" />
          <IconFileText size={15} stroke={1.5} className="tree-icon" />
          <span className="tree-label">{node.name}</span>
          <span className="tree-count">{node.entries.length}</span>
        </Row>
      );
    }
    case 'entry':
      return <EntryRow collection={row.collection} path={row.path} entry={row.entry} depth={row.depth} />;
    case 'hint':
      // One row high like the others: the full text is in the tooltip when it is cut.
      return (
        <div className="tree-empty" title={row.text}>
          {row.text}
        </div>
      );
    default:
      return <UnavailableCollection path={row.path} />;
  }
});

export const Sidebar = memo(function Sidebar() {
  const collections = useStore((s) => s.collections);
  const unavailable = useStore((s) => s.unavailable);
  const expanded = useStore((s) => s.expanded);
  const width = useStore((s) => s.sidebarWidth);
  const setLayout = useStore((s) => s.setLayout);
  const openCollectionDialog = useStore((s) => s.openCollectionDialog);
  const createCollectionDialog = useStore((s) => s.createCollectionDialog);
  const [searching, setSearching] = useState(false);
  const [search, setSearch] = useState('');
  // The input shows each keystroke at once; the tree follows when React has
  // time for it (filtering a large collection must not hold the typing back).
  const deferredSearch = useDeferredValue(search);
  const query = searching ? deferredSearch.trim().toLowerCase() : '';
  const rows = useMemo(
    () => visibleRows(collections, unavailable, expanded, query),
    [collections, unavailable, expanded, query],
  );

  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    const move = (ev: PointerEvent) => setLayout({ sidebarWidth: Math.min(600, Math.max(220, ev.clientX)) });
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
      document.body.classList.remove('dragging');
    };
    document.body.classList.add('dragging');
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  };

  return (
    <aside className="sidebar" style={{ width }}>
      <div className="sidebar-header">
        <span className="sidebar-title">Folders</span>
        <button
          type="button"
          className={`icon-btn ${searching ? 'on' : ''}`}
          title="Search requests"
          onClick={() => setSearching(!searching)}
        >
          <IconSearch size={15} stroke={1.5} />
        </button>
        <Dropdown
          className="icon-btn"
          title="Add a folder"
          items={[
            { label: 'Create folder', icon: <IconPlus {...ICON} />, onClick: createCollectionDialog },
            { label: 'Open folder', icon: <IconFolderOpen {...ICON} />, onClick: openCollectionDialog },
          ]}
        >
          <IconPlus size={16} stroke={1.5} />
        </Dropdown>
      </div>
      {searching && (
        <div className="sidebar-search">
          <input
            autoFocus
            spellCheck={false}
            placeholder="Search requests..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      )}
      {collections.length === 0 && unavailable.length === 0 ? (
        <div className="sidebar-tree" role="tree">
          <div className="sidebar-empty">
            <p>No folder opened.</p>
            <p>Open a folder of Hurl files.</p>
            <button type="button" className="btn btn-primary" onClick={openCollectionDialog}>
              Open Folder
            </button>
            <button type="button" className="btn btn-ghost" onClick={createCollectionDialog}>
              Create Folder
            </button>
          </div>
        </div>
      ) : (
        <VirtualList
          className="sidebar-tree"
          role="tree"
          count={rows.length}
          rowHeight={TREE_ROW_HEIGHT}
          rowKey={(i) => rows[i].key}
          renderRow={(i) => <TreeRowView row={rows[i]} />}
        />
      )}
      <div className="sidebar-resizer" onPointerDown={startResize} />
    </aside>
  );
});
