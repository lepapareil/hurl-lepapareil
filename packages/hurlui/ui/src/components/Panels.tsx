// Non-request tabs: file as text, runner, environments, preferences, welcome.

import {
  IconAlertTriangle,
  IconCheck,
  IconCircleCheck,
  IconCircleDashed,
  IconCircleX,
  IconFiles,
  IconFolderOpen,
  IconLoader2,
  IconPencil,
  IconPlayerPlay,
  IconPlus,
  IconTrash,
} from '@tabler/icons-react';
import { memo, useEffect, useMemo, useState } from 'react';
import { pickFile } from '../api';
import {
  envDraftKey,
  filesUnder,
  findNode,
  useStore,
  type Prefs,
  type RunnerFile,
  type RunnerState,
  type Tab,
  type Theme,
} from '../store';
import type { Variable } from '../types';
import { askText } from '../ui';
import { Logo } from './Chrome';
import { HurlOptionsForm, OptionCheck } from './HurlOptions';
import { CodeEditor } from './CodeEditor';
import { DraftBanner, SyntaxBanner } from './EntryPanel';
import { formatDuration } from './ResponsePane';
import { RunResults } from './RunResults';
import { EditableTable, SplitPane, VirtualList, type Column } from './ui';

// --- File as text -----------------------------------------------------------------

export function FilePanel({ tab }: { tab: Tab }) {
  const file = useStore((s) => s.files[tab.path]);
  const run = useStore((s) => s.fileRuns[tab.path]);
  const layout = useStore((s) => s.prefs.layout);
  const { setRawDraft, saveTab, runFile, cancelRun, patchTab } = useStore.getState();
  const results = useMemo(() => (run?.result ? [{ path: tab.path, result: run.result }] : []), [run?.result, tab.path]);
  // A file removed from the disk keeps its tab as long as it holds edits.
  if (!file && tab.rawDraft === undefined) return <div className="panel-empty">This file no longer exists.</div>;
  const dirty = tab.rawDraft !== undefined;

  return (
    <div className="entry-panel">
      <div className="panel-toolbar">
        <span className="panel-path" title={tab.path}>
          {tab.path}
        </span>
        <button type="button" className="btn btn-outline" disabled={!dirty} onClick={() => saveTab(tab.id)}>
          Save
        </button>
        {run?.running ? (
          <button type="button" className="btn btn-outline" onClick={() => cancelRun(run.runId)}>
            Cancel
          </button>
        ) : (
          <button type="button" className="btn btn-primary" onClick={() => runFile(tab.collection, tab.path)}>
            Run File
          </button>
        )}
      </div>
      <DraftBanner tab={tab} />
      {file?.syntaxError && <SyntaxBanner message={file.syntaxError} />}
      <SplitPane
        direction={layout}
        initial={0.55}
        first={
          <div className="editor-box editor-full">
            <CodeEditor
              value={tab.rawDraft ?? file?.raw ?? ''}
              language="hurl"
              onChange={(raw) => setRawDraft(tab.id, raw)}
            />
          </div>
        }
        second={
          <RunResults
            collection={tab.collection}
            files={results}
            pane={tab.responsePane}
            onPane={(responsePane) => patchTab(tab.id, { responsePane })}
            running={run?.running}
            onCancel={() => cancelRun(run?.runId)}
            empty="Run the file to see the result of each request."
          />
        }
      />
    </div>
  );
}

// --- Runner -----------------------------------------------------------------------

function RunnerIcon({ status }: { status: RunnerFile['status'] }) {
  switch (status) {
    case 'running':
      return <IconLoader2 size={16} stroke={1.5} className="spin" />;
    case 'passed':
      return <IconCircleCheck size={16} stroke={1.5} className="passed" />;
    case 'failed':
      return <IconCircleX size={16} stroke={1.5} className="failed" />;
    default:
      return <IconCircleDashed size={16} stroke={1.5} className="muted" />;
  }
}

/** Height of a row of the runner list, in pixels (the list is windowed). */
const RUNNER_ROW_HEIGHT = 30;

/**
 * One file of a run. Memoized: a status change replaces the object of that
 * file only (see `patchRunnerFile`), so the other rows are not rendered again.
 */
const RunnerFileRow = memo(function RunnerFileRow(props: {
  tabId: string;
  collection: string;
  file: RunnerFile;
  active: boolean;
}) {
  const { file } = props;
  return (
    <button
      type="button"
      className={`runner-file ${props.active ? 'active' : ''}`}
      onClick={() => useStore.getState().selectRunnerFile(props.tabId, file.path)}
    >
      <RunnerIcon status={file.status} />
      <span className="runner-file-name" title={file.path}>
        {file.path.slice(props.collection.length + 1) || file.name}
      </span>
      {file.result && (
        <span className="muted">
          {file.result.entries.length} req · {formatDuration(file.result.durationMs)}
        </span>
      )}
    </button>
  );
});

/**
 * Results of a run: the same panes as the response of a request, over all
 * the requests of the run. When several files were run, their list is on the
 * left: it shows where the run is, and restricts the results to one file.
 */
function RunnerResults({ tab, runner }: { tab: Tab; runner: RunnerState }) {
  const { patchTab, cancelRunner, selectRunnerFile } = useStore.getState();
  const done = useMemo(
    () =>
      runner.files.flatMap((f) =>
        f.result && (runner.selected === null || f.path === runner.selected) ? [{ path: f.path, result: f.result }] : [],
      ),
    [runner.files, runner.selected],
  );
  const results = (
    <RunResults
      collection={tab.collection}
      files={done}
      pane={tab.responsePane}
      onPane={(responsePane) => patchTab(tab.id, { responsePane })}
      // The results show as the files are run: only the wait for the first one hides the panes.
      running={runner.status === 'running' && done.length === 0}
      onCancel={() => cancelRunner(tab.id)}
      empty="No result for this file."
    />
  );
  if (runner.files.length < 2) return results;
  return (
    <SplitPane
      direction="horizontal"
      initial={0.28}
      first={
        <VirtualList
          className="runner-files"
          count={runner.files.length + 1}
          rowHeight={RUNNER_ROW_HEIGHT}
          rowKey={(i) => (i === 0 ? '' : runner.files[i - 1].path)}
          renderRow={(i) =>
            i === 0 ? (
              <button
                type="button"
                className={`runner-file ${runner.selected === null ? 'active' : ''}`}
                onClick={() => selectRunnerFile(tab.id, null)}
              >
                <IconFiles size={16} stroke={1.5} />
                <span className="runner-file-name">All files</span>
                <span className="muted">{runner.files.length}</span>
              </button>
            ) : (
              <RunnerFileRow
                tabId={tab.id}
                collection={tab.collection}
                file={runner.files[i - 1]}
                active={runner.files[i - 1].path === runner.selected}
              />
            )
          }
        />
      }
      second={results}
    />
  );
}

export function RunnerPanel({ tab }: { tab: Tab }) {
  const runner = useStore((s) => s.runners[tab.id]);
  const collection = useStore((s) => s.collections.find((c) => c.path === tab.collection));
  const environment = useStore((s) => s.activeEnv[tab.collection]);
  const { startRunner, cancelRunner, resetRunner } = useStore.getState();
  if (!runner || !collection) return <div className="panel-empty">This folder is closed.</div>;

  if (runner.status === 'idle') {
    // Only needed here: during a run this panel renders at every file status.
    const node = tab.path === collection.path ? null : findNode(collection.children, tab.path);
    const files = filesUnder(node ? [node] : tab.path === collection.path ? collection.children : []);
    const requests = files.reduce((n, f) => n + f.entries.length, 0);
    return (
      <div className="runner runner-idle">
        <h2>Runner</h2>
        <p className="muted">{tab.path}</p>
        <p>
          You have <strong>{requests}</strong> request{requests === 1 ? '' : 's'} in <strong>{files.length}</strong>{' '}
          Hurl file{files.length === 1 ? '' : 's'}.
        </p>
        <p className="muted">
          Environment: {environment ?? 'none'}. Each file is run by <code>hurl</code>, one after the other.
        </p>
        <OptionCheck long="continue-on-error" />
        <div className="runner-actions">
          <button
            type="button"
            className="btn btn-primary"
            disabled={files.length === 0}
            onClick={() => startRunner(tab.id)}
          >
            <IconPlayerPlay size={15} stroke={1.5} /> Run {files.length} File{files.length === 1 ? '' : 's'}
          </button>
        </div>
      </div>
    );
  }

  const counts: Record<RunnerFile['status'], number> = { queued: 0, running: 0, passed: 0, failed: 0, cancelled: 0 };
  for (const f of runner.files) counts[f.status]++;
  return (
    <div className="runner">
      <div className="panel-toolbar">
        <span className="runner-stats">
          <strong>
            {runner.files.length} file{runner.files.length === 1 ? '' : 's'}
          </strong>
          <span className="passed">Passed {counts.passed}</span>
          <span className="failed">Failed {counts.failed}</span>
          {counts.cancelled > 0 && <span className="muted">Skipped {counts.cancelled}</span>}
          {runner.status === 'running' && (
            <span className="muted">
              <IconLoader2 size={13} stroke={1.5} className="spin" /> Running
            </span>
          )}
        </span>
        {runner.status === 'running' ? (
          <button type="button" className="btn btn-outline" onClick={() => cancelRunner(tab.id)}>
            Cancel Execution
          </button>
        ) : (
          <>
            <button type="button" className="btn btn-primary" onClick={() => startRunner(tab.id)}>
              Run Again
            </button>
            <button type="button" className="btn btn-outline" onClick={() => resetRunner(tab.id)}>
              Reset
            </button>
          </>
        )}
      </div>
      <RunnerResults tab={tab} runner={runner} />
    </div>
  );
}

// --- Environments -----------------------------------------------------------------

const VARIABLE_COLUMNS: Column<Variable>[] = [
  { key: 'name', label: 'Name', placeholder: 'base_url', width: '30%' },
  { key: 'value', label: 'Value', placeholder: 'Value', masked: (v) => v.secret },
  { key: 'secret', label: 'Secret', type: 'checkbox', width: '70px' },
];

export function EnvironmentsPanel({ tab }: { tab: Tab }) {
  const collection = useStore((s) => s.collections.find((c) => c.path === tab.collection));
  const active = useStore((s) => s.activeEnv[tab.collection]);
  const envDrafts = useStore((s) => s.envDrafts);
  const { createEnvironment, saveEnvironment, renameEnvironment, deleteEnvironment, setActiveEnv, setEnvDraft } =
    useStore.getState();
  const environments = collection?.environments ?? [];
  const [selected, setSelected] = useState<string | null>(active ?? environments[0]?.name ?? null);
  const current = environments.find((e) => e.name === selected);
  // Unsaved variables live in the store, per environment: they survive a tab
  // switch and can never be saved into another environment.
  const draft = current ? envDrafts[envDraftKey(tab.collection, current.name)] : undefined;
  const setDraft = (variables: Variable[] | null) => {
    if (current) setEnvDraft(tab.collection, current.name, variables);
  };

  useEffect(() => {
    if (!current && environments.length > 0) setSelected(environments[0].name);
  }, [current, environments]);

  if (!collection) return <div className="panel-empty">This folder is closed.</div>;

  const select = (name: string) => setSelected(name);
  const create = async () => {
    const name = await askText('Create Environment', 'Name', '', 'Create');
    if (name && (await createEnvironment(collection.path, name))) {
      select(name);
      if (!active) setActiveEnv(collection.path, name);
    }
  };
  const rename = async () => {
    if (!current) return;
    const name = await askText('Rename Environment', 'New name', current.name, 'Rename');
    if (name && name !== current.name && (await renameEnvironment(collection.path, current.name, name))) {
      select(name);
    }
  };
  const save = async () => {
    if (current && draft) await saveEnvironment(collection.path, current.name, draft);
  };

  return (
    <div className="environments">
      <div className="env-list">
        <div className="env-list-header">
          <span>Environments</span>
          <button type="button" className="icon-btn" title="Create environment" onClick={create}>
            <IconPlus size={16} stroke={1.5} />
          </button>
        </div>
        {environments.map((e) => (
          <div
            key={e.name}
            className={`env-item ${e.name === selected ? 'active' : ''}`}
            onClick={() => select(e.name)}
          >
            <span className="tree-label">{e.name}</span>
            {envDrafts[envDraftKey(tab.collection, e.name)] && <span className="draft-dot" title="Unsaved changes" />}
            <button
              type="button"
              className={`icon-btn ${e.name === active ? 'on' : ''}`}
              title={e.name === active ? 'Active environment' : 'Use this environment'}
              onClick={(ev) => {
                ev.stopPropagation();
                setActiveEnv(collection.path, e.name === active ? null : e.name);
              }}
            >
              <IconCheck size={15} stroke={e.name === active ? 2.5 : 1.2} />
            </button>
          </div>
        ))}
        {environments.length === 0 && <div className="tree-empty">No environment yet.</div>}
      </div>
      <div className="env-details">
        {current ? (
          <>
            <div className="panel-toolbar">
              <h3>{current.name}</h3>
              <button type="button" className="icon-btn" title="Rename" onClick={rename}>
                <IconPencil size={15} stroke={1.5} />
              </button>
              <button
                type="button"
                className="icon-btn"
                title="Delete"
                onClick={() => deleteEnvironment(collection.path, current.name)}
              >
                <IconTrash size={15} stroke={1.5} />
              </button>
              <div className="statusbar-spacer" />
              <button type="button" className="btn btn-outline" disabled={!draft} onClick={() => setDraft(null)}>
                Reset
              </button>
              <button type="button" className="btn btn-primary" disabled={!draft} onClick={save}>
                Save
              </button>
            </div>
            <div className="pane-content">
              <EditableTable
                columns={VARIABLE_COLUMNS}
                rows={draft ?? current.variables}
                onChange={setDraft}
                makeRow={() => ({ name: '', value: '', secret: false })}
                addLabel="Add Variable"
              />
              <p className="hint">
                Variables are saved in <code>{current.path}</code>, a Hurl variables file passed with{' '}
                <code>--variables-file</code>. Secrets stay out of the folder, in the application data of this
                machine, and are given to Hurl as secrets: they are redacted from its logs and reports.
              </p>
            </div>
          </>
        ) : (
          <div className="pane-empty">
            <button type="button" className="btn btn-primary" onClick={create}>
              Create Environment
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

// --- Preferences ------------------------------------------------------------------

export function PreferencesPanel() {
  const prefs = useStore((s) => s.prefs);
  const appInfo = useStore((s) => s.appInfo);
  const setPrefs = useStore((s) => s.setPrefs);
  const [hurlPath, setHurlPath] = useState(prefs.hurlPath);
  // `hurl 9.0.0 (x86_64-pc-linux-gnu) libcurl/...`
  const hurlVersion = appInfo?.hurlVersion?.split(' ')[1];

  return (
    <div className="preferences">
      <h2>Preferences</h2>

      <h4 className="section-title">Appearance</h4>
      <div className="form-grid">
        <label>Theme</label>
        <select value={prefs.theme} onChange={(e) => setPrefs({ theme: e.target.value as Theme })}>
          <option value="system">System</option>
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
        <label>Response pane</label>
        <select value={prefs.layout} onChange={(e) => setPrefs({ layout: e.target.value as Prefs['layout'] })}>
          <option value="horizontal">On the right</option>
          <option value="vertical">Below</option>
        </select>
      </div>

      <h4 className="section-title">Hurl binary</h4>
      <div className="form-row">
        <input
          aria-label="Path of the hurl binary"
          spellCheck={false}
          placeholder="Detected automatically (PATH and usual install directories)"
          value={hurlPath}
          onChange={(e) => setHurlPath(e.target.value)}
          onBlur={() => hurlPath !== prefs.hurlPath && setPrefs({ hurlPath: hurlPath.trim() })}
        />
        <button
          type="button"
          className="btn btn-outline"
          onClick={async () => {
            const path = await pickFile('hurl binary');
            if (path) {
              setHurlPath(path);
              setPrefs({ hurlPath: path });
            }
          }}
        >
          <IconFolderOpen size={15} stroke={1.5} /> Browse
        </button>
      </div>
      {appInfo?.hurlVersion ? (
        <p className="hint">
          <IconCheck size={14} stroke={2} className="passed" /> {appInfo.hurlVersion}
          <br />
          hurl: <code>{appInfo.hurlPath}</code>
          <br />
          hurlfmt: <code>{appInfo.hurlfmtPath ?? 'not found (no syntax check, no curl import)'}</code>
          {hurlVersion !== appInfo.version && (
            <>
              <br />
              <IconAlertTriangle size={14} stroke={2} /> This is not hurl {appInfo.version}, which Hurl UI is shipped
              with: the options below are the ones of hurl {appInfo.version}.
            </>
          )}
        </p>
      ) : (
        <p className="hint failed">
          <IconAlertTriangle size={14} stroke={2} /> hurl was not found. Install it from https://hurl.dev or set its
          path above.
        </p>
      )}

      <h4 className="section-title">Requests</h4>
      <div className="form-stack">
        <label className="check">
          <input
            type="checkbox"
            checked={prefs.chainEntries}
            onChange={(e) => setPrefs({ chainEntries: e.target.checked })}
          />
          Send: run the previous requests of the file first (captured variables)
        </label>
      </div>
      <div className="form-grid">
        <label>Log level</label>
        <select
          value={prefs.verbosity}
          onChange={(e) => setPrefs({ verbosity: e.target.value as Prefs['verbosity'] })}
        >
          <option value="none">Errors only</option>
          <option value="verbose">Verbose</option>
          <option value="very-verbose">Very verbose, with bodies</option>
        </select>
        <label>Extra arguments</label>
        <input
          spellCheck={false}
          placeholder="Added as they are at the end of the command line"
          value={prefs.extraArgs}
          onChange={(e) => setPrefs({ extraArgs: e.target.value })}
        />
      </div>

      <h4 className="section-title">Hurl options</h4>
      <p className="hint">
        These options are added to every <code>hurl</code> command line. An <code>[Options]</code> section in a
        request takes precedence.
      </p>
      <HurlOptionsForm />
    </div>
  );
}

// --- Welcome ------------------------------------------------------------------------

export function Welcome() {
  const appInfo = useStore((s) => s.appInfo);
  const { openCollectionDialog, createCollectionDialog, openPreferences } = useStore.getState();
  return (
    <div className="welcome">
      <Logo size={56} />
      <h1>Hurl UI</h1>
      <p className="muted">Explore and run the Hurl files of your folders.</p>
      <div className="welcome-actions">
        <button type="button" className="btn btn-primary" onClick={openCollectionDialog}>
          <IconFolderOpen size={15} stroke={1.5} /> Open Folder
        </button>
        <button type="button" className="btn btn-outline" onClick={createCollectionDialog}>
          <IconPlus size={15} stroke={1.5} /> Create Folder
        </button>
      </div>
      {appInfo && !appInfo.hurlVersion && (
        <button type="button" className="banner banner-error" onClick={openPreferences}>
          <IconAlertTriangle size={16} stroke={1.5} /> hurl was not found on this machine. Click to set its path.
        </button>
      )}
      {appInfo?.hurlVersion && (
        <p className="muted" title={appInfo.hurlVersion}>
          {appInfo.hurlVersion.split(' ').slice(0, 2).join(' ')} · {appInfo.hurlPath}
        </p>
      )}
    </div>
  );
}
