// Request tab: URL bar, request pane (Params, Body, Headers, Auth, Vars, Assert,
// Options, Docs) and response pane.

import {
  IconAlertTriangle,
  IconChevronDown,
  IconDeviceFloppy,
  IconLink,
  IconLinkOff,
} from '@tabler/icons-react';
import { call, pickFile } from '../api';
import { entryKey, entryOfTab, useStore, type Tab } from '../store';
import type { Assert, BodyMode, Entry, KeyValue } from '../types';
import { askText, toast, toastError, type MenuItem } from '../ui';
import { CodeEditor } from './CodeEditor';
import { SectionValue, sectionRow } from './HurlOptions';
import { ResponsePane } from './ResponsePane';
import { METHODS, methodClass } from './Sidebar';
import { Dropdown, EditableTable, PaneTabs, SplitPane, type Column, type PaneTab } from './ui';

const KV: Column<KeyValue>[] = [
  { key: 'name', label: 'Name', placeholder: 'Name', width: '35%' },
  { key: 'value', label: 'Value', placeholder: 'Value' },
];
const newKv = (): KeyValue => ({ name: '', value: '' });

const BODY_MODES: { mode: BodyMode; label: string }[] = [
  { mode: 'none', label: 'No Body' },
  { mode: 'json', label: 'JSON' },
  { mode: 'xml', label: 'XML' },
  { mode: 'text', label: 'Text' },
  { mode: 'graphql', label: 'GraphQL' },
  { mode: 'form', label: 'Form URL Encoded' },
  { mode: 'multipart', label: 'Multipart Form' },
  { mode: 'file', label: 'File' },
  { mode: 'hurl', label: 'Raw Hurl' },
];

type Update = (mutate: (entry: Entry) => void) => void;

/** Shown when the file changed on disk under unsaved edits, or lost the edited request. */
export function DraftBanner({ tab }: { tab: Tab }) {
  const revertTab = useStore((s) => s.revertTab);
  if (!tab.conflict && !tab.orphan) return null;
  return (
    <div className="banner banner-warning">
      <IconAlertTriangle size={16} stroke={1.5} />
      <span>
        {tab.orphan
          ? 'This request no longer exists in the file. Your edits are kept here but cannot be saved: copy what you need.'
          : tab.kind === 'file'
            ? 'This file was changed or removed on disk since you started editing. Saving will overwrite it.'
            : 'This request changed on disk since you started editing. Saving will overwrite that change.'}
      </span>
      <button type="button" className="link-btn" onClick={() => revertTab(tab.id)}>
        {tab.orphan ? 'Close' : 'Discard my edits'}
      </button>
    </div>
  );
}

export function SyntaxBanner({ message }: { message: string }) {
  return (
    <div className="banner banner-error">
      <IconAlertTriangle size={16} stroke={1.5} />
      <pre>{message}</pre>
    </div>
  );
}

function QueryUrl({ tab, entry, update }: { tab: Tab; entry: Entry; update: Update }) {
  const running = useStore((s) => Boolean(s.entryRuns[entryKey(tab.path, tab.index)]?.running));
  const runId = useStore((s) => s.entryRuns[entryKey(tab.path, tab.index)]?.runId);
  const chain = useStore((s) => s.prefs.chainEntries);
  const { sendEntry, saveTab, cancelRun, setPrefs, replaceDraft } = useStore.getState();
  const dirty = tab.draft !== undefined;

  const methods: MenuItem[] = [
    ...METHODS.map((m) => ({ label: m, onClick: () => update((e) => void (e.method = m)) })),
    { separator: true },
    {
      label: '+ Add Custom',
      onClick: async () => {
        const m = await askText('Custom method', 'Method (uppercase letters)', '', 'Use');
        if (m && /^[A-Za-z]+$/.test(m)) update((e) => void (e.method = m.toUpperCase()));
        else if (m) toast('A Hurl method is made of letters only.', 'error');
      },
    },
  ];

  // Pasting a curl command fills the whole request (conversion by hurlfmt).
  const importCurl = async (curl: string) => {
    try {
      const imported = await call<Entry>('curl_to_entry', {
        curl,
        hurlPath: useStore.getState().prefs.hurlPath,
      });
      // The documentation (and so the name) of the request is kept. Its
      // position is not part of a draft: saving goes by the index of the tab.
      replaceDraft(tab.id, { ...imported, docs: entry.docs });
      toast('curl command imported', 'success');
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <div className="query-url">
      <div className="url-group">
        <Dropdown className={`method-select ${methodClass(entry.method)}`} items={methods} title="HTTP method">
          {entry.method}
          <IconChevronDown size={13} stroke={1.5} />
        </Dropdown>
        <input
          className="url-input"
          aria-label="URL"
          spellCheck={false}
          placeholder="Enter URL or paste a cURL request"
          value={entry.url}
          onChange={(e) => update((en) => void (en.url = e.target.value))}
          onKeyDown={(e) => e.key === 'Enter' && !e.ctrlKey && !e.metaKey && sendEntry(tab.id)}
          onPaste={(e) => {
            const text = e.clipboardData.getData('text');
            if (/^\s*curl\s/i.test(text)) {
              e.preventDefault();
              void importCurl(text);
            }
          }}
        />
        <button
          type="button"
          className={`icon-btn ${chain ? 'on' : ''}`}
          title={
            chain
              ? 'Chained: the previous requests of the file are run first (captures available)'
              : 'Isolated: only this request is run. Click to run the previous requests of the file first'
          }
          onClick={() => setPrefs({ chainEntries: !chain })}
        >
          {chain ? <IconLink size={16} stroke={1.5} /> : <IconLinkOff size={16} stroke={1.5} />}
        </button>
        <button
          type="button"
          className={`icon-btn ${dirty ? 'draft' : ''}`}
          title="Save (Ctrl+S)"
          disabled={!dirty || tab.orphan}
          onClick={() => saveTab(tab.id)}
        >
          <IconDeviceFloppy size={17} stroke={1.5} />
        </button>
      </div>
      {running ? (
        <button type="button" className="btn btn-outline send-btn" onClick={() => cancelRun(runId)}>
          Cancel
        </button>
      ) : (
        <button
          type="button"
          className="btn btn-primary send-btn"
          title="Send (Ctrl+Enter)"
          disabled={tab.orphan}
          onClick={() => sendEntry(tab.id)}
        >
          Send
        </button>
      )}
    </div>
  );
}

function BodyPane({ tab, entry, update }: { tab: Tab; entry: Entry; update: Update }) {
  const { mode, content, fenced } = entry.body;
  const setMode = (next: BodyMode) =>
    update((e) => {
      if (next !== e.body.mode && (next === 'json' || next === 'xml')) e.body.fenced = false;
      e.body.mode = next;
      // A Hurl request has either a body or a form section.
      if (next !== 'form') e.form = [];
      if (next !== 'multipart') e.multipart = [];
    });
  const setContent = (value: string) => update((e) => void (e.body.content = value));
  const prettify = () => {
    try {
      setContent(JSON.stringify(JSON.parse(content), null, 2));
    } catch {
      toast('Not plain JSON (templates such as {{var}} cannot be prettified).', 'error');
    }
  };

  return (
    <div className="pane-content pane-fill">
      <div className="pane-toolbar">
        <Dropdown
          className="select-btn"
          items={BODY_MODES.map((m) => ({ label: m.label, onClick: () => setMode(m.mode) }))}
        >
          {BODY_MODES.find((m) => m.mode === mode)?.label ?? mode}
          <IconChevronDown size={13} stroke={1.5} />
        </Dropdown>
        {(mode === 'json' || mode === 'xml') && (
          <label className="check" title="Write the body as a multiline string: any text is accepted, variables included">
            <input
              type="checkbox"
              checked={fenced}
              onChange={(e) => update((en) => void (en.body.fenced = e.target.checked))}
            />
            {'```'} multiline string
          </label>
        )}
        {mode === 'json' && (
          <button type="button" className="link-btn" onClick={prettify}>
            Prettify
          </button>
        )}
      </div>
      {mode === 'none' && <div className="pane-empty">No Body</div>}
      {(mode === 'json' || mode === 'xml' || mode === 'text' || mode === 'graphql' || mode === 'hurl') && (
        <div className="editor-box">
          <CodeEditor
            value={content}
            onChange={setContent}
            language={mode === 'json' ? 'json' : mode === 'xml' ? 'xml' : mode === 'hurl' ? 'hurl' : 'text'}
            placeholder={
              mode === 'hurl'
                ? 'Any Hurl body: `one line`, base64,...; hex,...; ```raw ... ```'
                : mode === 'graphql'
                  ? 'query { ... }'
                  : undefined
            }
          />
        </div>
      )}
      {mode === 'form' && (
        <EditableTable
          columns={KV}
          rows={entry.form}
          onChange={(rows) => update((e) => void (e.form = rows))}
          makeRow={newKv}
          addLabel="Add Param"
        />
      )}
      {mode === 'multipart' && (
        <>
          <EditableTable
            columns={KV}
            rows={entry.multipart}
            onChange={(rows) => update((e) => void (e.multipart = rows))}
            makeRow={newKv}
            addLabel="Add Param"
          />
          <p className="hint">
            File upload: value <code>file,path/to/file.png;</code> optionally followed by a content type. Paths are
            relative to the Hurl file.
          </p>
        </>
      )}
      {mode === 'file' && (
        <div className="form-row">
          <input
            aria-label="File path"
            spellCheck={false}
            placeholder="data.bin (relative to the Hurl file)"
            value={content}
            onChange={(e) => setContent(e.target.value)}
          />
          <button
            type="button"
            className="btn btn-outline"
            onClick={async () => {
              const target = await pickFile('Body file');
              if (!target) return;
              try {
                // Relative to the Hurl file, with the escapes of Hurl file names.
                const { path, outside } = await call<{ path: string; outside: boolean }>('body_file_path', {
                  file: tab.path,
                  target,
                });
                setContent(path);
                if (outside) {
                  toast(
                    'This file is outside of the folder of the Hurl file: hurl refuses it unless its file root allows it (Preferences > Hurl options).',
                    'error',
                  );
                }
              } catch (e) {
                toastError(e);
              }
            }}
          >
            Browse
          </button>
        </div>
      )}
    </div>
  );
}

function AuthPane({ entry, update }: { entry: Entry; update: Update }) {
  const bearerIndex = entry.headers.findIndex(
    (h) => h.name.toLowerCase() === 'authorization' && /^bearer(\s|$)/i.test(h.value),
  );
  const mode = entry.basicAuth ? 'basic' : bearerIndex >= 0 ? 'bearer' : 'none';
  const labels: Record<string, string> = { none: 'No Auth', basic: 'Basic Auth', bearer: 'Bearer Token' };
  const setMode = (next: string) =>
    update((e) => {
      e.headers = e.headers.filter(
        (h) => !(h.name.toLowerCase() === 'authorization' && /^bearer(\s|$)/i.test(h.value)),
      );
      e.basicAuth = next === 'basic' ? (e.basicAuth ?? { name: '', value: '' }) : null;
      if (next === 'bearer') e.headers.push({ name: 'Authorization', value: 'Bearer ' });
    });

  return (
    <div className="pane-content">
      <div className="pane-toolbar">
        <Dropdown
          className="select-btn"
          items={Object.entries(labels).map(([key, label]) => ({ label, onClick: () => setMode(key) }))}
        >
          {labels[mode]}
          <IconChevronDown size={13} stroke={1.5} />
        </Dropdown>
      </div>
      {mode === 'none' && <div className="pane-empty">No Auth</div>}
      {mode === 'basic' && entry.basicAuth && (
        <div className="form-grid">
          <label>Username</label>
          <input
            spellCheck={false}
            value={entry.basicAuth.name}
            onChange={(ev) => update((e) => void (e.basicAuth = { ...e.basicAuth!, name: ev.target.value }))}
          />
          <label>Password</label>
          <input
            spellCheck={false}
            value={entry.basicAuth.value}
            placeholder="{{password}}"
            onChange={(ev) => update((e) => void (e.basicAuth = { ...e.basicAuth!, value: ev.target.value }))}
          />
          <p className="hint">
            Written as a <code>[BasicAuth]</code> section. Prefer a secret variable of the environment for the
            password.
          </p>
        </div>
      )}
      {mode === 'bearer' && (
        <div className="form-grid">
          <label>Token</label>
          <input
            spellCheck={false}
            placeholder="{{token}}"
            value={entry.headers[bearerIndex].value.replace(/^bearer\s*/i, '')}
            onChange={(ev) =>
              update((e) => void (e.headers[bearerIndex].value = `Bearer ${ev.target.value}`))
            }
          />
          <p className="hint">
            Written as an <code>Authorization: Bearer</code> header.
          </p>
        </div>
      )}
    </div>
  );
}

const ASSERT_COLUMNS: Column<Assert>[] = [
  { key: 'query', label: 'Expr', placeholder: 'jsonpath "$.id"', list: 'hurl-queries', width: '42%' },
  { key: 'predicate', label: 'Operator', placeholder: '==', list: 'hurl-predicates', width: '22%' },
  { key: 'value', label: 'Value', placeholder: 'Value' },
];

function AssertPane({ entry, update }: { entry: Entry; update: Update }) {
  return (
    <div className="pane-content">
      <div className="form-inline">
        <label>
          Status
          <input
            className="short"
            spellCheck={false}
            placeholder="any"
            title="Expected status code: 200, or * for any"
            value={entry.status === '*' ? '' : entry.status}
            onChange={(ev) =>
              update((e) => {
                e.status = ev.target.value.trim();
                // The response part is written only when it holds something.
                e.hasResponse = false;
              })
            }
          />
        </label>
        <label>
          Version
          <select value={entry.version} onChange={(ev) => update((e) => void (e.version = ev.target.value))}>
            {['HTTP', 'HTTP/1.0', 'HTTP/1.1', 'HTTP/2', 'HTTP/3'].map((v) => (
              <option key={v} value={v}>
                {v === 'HTTP' ? 'any' : v}
              </option>
            ))}
          </select>
        </label>
      </div>
      <EditableTable
        columns={ASSERT_COLUMNS}
        rows={entry.asserts}
        onChange={(rows) => update((e) => void (e.asserts = rows))}
        makeRow={() => ({ query: '', predicate: '==', value: '' })}
        addLabel="Add Assertion"
      />
      <h4 className="section-title">Implicit asserts</h4>
      <p className="hint">Headers and body the response must match exactly.</p>
      <EditableTable
        columns={KV}
        rows={entry.responseHeaders}
        onChange={(rows) => update((e) => void (e.responseHeaders = rows))}
        makeRow={newKv}
        addLabel="Add Expected Header"
      />
      <div className="editor-box editor-small">
        <CodeEditor
          value={entry.responseBody}
          onChange={(v) => update((e) => void (e.responseBody = v))}
          language="hurl"
          placeholder={'Expected body, in Hurl syntax: {"id": 1}, `text`, ``` ... ```'}
        />
      </div>
    </div>
  );
}

function RequestPane({ tab, entry, update }: { tab: Tab; entry: Entry; update: Update }) {
  const patchTab = useStore((s) => s.patchTab);
  const named = (rows: { name: string }[]) => rows.filter((r) => r.name.trim()).length;
  const auth =
    entry.basicAuth !== null ||
    entry.headers.some((h) => h.name.toLowerCase() === 'authorization' && /^bearer(\s|$)/i.test(h.value));
  // The response line counts once, for its status or for its version alone
  // (`HTTP/2 *` expects something too). As in the core, no version is `HTTP`.
  const status = entry.status.trim();
  const version = entry.version.trim();
  const responseLine = (status !== '' && status !== '*') || (version !== '' && version !== 'HTTP');
  const asserts = entry.asserts.filter((a) => a.query.trim()).length + (responseLine ? 1 : 0);
  const tabs: PaneTab[] = [
    { key: 'params', label: 'Params', count: named(entry.query) },
    { key: 'body', label: 'Body', dot: entry.body.mode !== 'none' },
    { key: 'headers', label: 'Headers', count: named(entry.headers) + named(entry.cookies) },
    { key: 'auth', label: 'Auth', dot: auth },
    { key: 'options', label: 'Options', count: named(entry.options) },
    { key: 'assert', label: 'Assert', count: asserts },
    { key: 'captures', label: 'Captures', count: named(entry.captures) },
    { key: 'docs', label: 'Docs', dot: entry.docs.trim() !== '' },
  ];
  const pane = tabs.some((t) => t.key === tab.pane) ? tab.pane : 'params';

  return (
    <div className="request-pane">
      <PaneTabs tabs={tabs} active={pane} onSelect={(key) => patchTab(tab.id, { pane: key })} />
      {pane === 'params' && (
        <div className="pane-content">
          <h4 className="section-title">Query</h4>
          <EditableTable
            columns={KV}
            rows={entry.query}
            onChange={(rows) => update((e) => void (e.query = rows))}
            makeRow={newKv}
            addLabel="Add Param"
          />
        </div>
      )}
      {pane === 'body' && <BodyPane tab={tab} entry={entry} update={update} />}
      {pane === 'headers' && (
        <div className="pane-content">
          <EditableTable
            columns={[{ ...KV[0], list: 'http-headers' }, KV[1]]}
            rows={entry.headers}
            onChange={(rows) => update((e) => void (e.headers = rows))}
            makeRow={newKv}
            addLabel="Add Header"
          />
          <h4 className="section-title">Cookies</h4>
          <EditableTable
            columns={KV}
            rows={entry.cookies}
            onChange={(rows) => update((e) => void (e.cookies = rows))}
            makeRow={newKv}
            addLabel="Add Cookie"
          />
        </div>
      )}
      {pane === 'auth' && <AuthPane entry={entry} update={update} />}
      {pane === 'captures' && (
        <div className="pane-content">
          <p className="hint">
            Values extracted from the response, usable as <code>{'{{name}}'}</code> in the next requests of the file.
          </p>
          <EditableTable
            columns={[
              { key: 'name', label: 'Name', placeholder: 'token', width: '30%' },
              { key: 'value', label: 'Query', placeholder: 'jsonpath "$.token"', list: 'hurl-queries' },
            ]}
            rows={entry.captures}
            onChange={(rows) => update((e) => void (e.captures = rows))}
            makeRow={newKv}
            addLabel="Add Capture"
          />
        </div>
      )}
      {pane === 'assert' && <AssertPane entry={entry} update={update} />}
      {pane === 'options' && (
        <div className="pane-content">
          <p className="hint">
            Options of this request (<code>[Options]</code> section): they override the preferences.
          </p>
          <EditableTable
            columns={[
              { key: 'name', label: 'Option', placeholder: 'location', list: 'hurl-options', width: '35%' },
              {
                key: 'value',
                label: 'Value',
                render: (row, onChange) => <SectionValue name={row.name} value={row.value} onChange={onChange} />,
              },
            ]}
            rows={entry.options}
            onChange={(rows) => update((e) => void (e.options = rows))}
            onCellChange={sectionRow}
            makeRow={newKv}
            addLabel="Add Option"
          />
        </div>
      )}
      {pane === 'docs' && (
        <div className="pane-content pane-fill">
          <p className="hint">
            Written as comments above the request. The first line is the name shown in the sidebar.
          </p>
          <textarea
            className="docs"
            aria-label="Documentation"
            placeholder="Describe the request"
            value={entry.docs}
            onChange={(ev) => update((e) => void (e.docs = ev.target.value))}
          />
        </div>
      )}
    </div>
  );
}

export function EntryPanel({ tab }: { tab: Tab }) {
  const entry = useStore((s) => entryOfTab(s, tab));
  const syntaxError = useStore((s) => s.files[tab.path]?.syntaxError);
  const layout = useStore((s) => s.prefs.layout);
  const updateDraft = useStore((s) => s.updateDraft);
  if (!entry) return <div className="panel-empty">This request no longer exists in the file.</div>;
  const update: Update = (mutate) => updateDraft(tab.id, mutate);
  return (
    <div className="entry-panel">
      <QueryUrl tab={tab} entry={entry} update={update} />
      <DraftBanner tab={tab} />
      {syntaxError && <SyntaxBanner message={syntaxError} />}
      <SplitPane
        direction={layout}
        first={<RequestPane tab={tab} entry={entry} update={update} />}
        second={<ResponsePane tab={tab} />}
      />
    </div>
  );
}
