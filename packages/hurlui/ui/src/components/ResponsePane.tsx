// Response pane of a request: Response, Headers, Timeline, Tests.

import type { EditorState } from '@codemirror/state';
import { IconCheck, IconCode, IconCopy, IconLoader2, IconRoute, IconSend, IconX } from '@tabler/icons-react';
import { useState } from 'react';
import { entryKey, useStore, type Tab } from '../store';
import type { AssertResult, Call, EntryResult, KeyValue, ResponseBody, RunMeta } from '../types';
import { jsonPathAt, jsonPathQuery, xpathAt, xpathQuery } from '../paths';
import { showMenu, toast, type MenuItem } from '../ui';
import { CodeEditor, type EditorLanguage } from './CodeEditor';
import { PaneTabs, type PaneTab } from './ui';
import { Waterfall } from './Waterfall';

function headerValue(headers: KeyValue[], name: string): string | undefined {
  return headers.find((h) => h.name.toLowerCase() === name)?.value;
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

export function formatDuration(ms: number): string {
  return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`;
}

export function statusClass(status: number): string {
  if (status >= 200 && status < 300) return 'status-ok';
  if (status >= 300 && status < 400) return 'status-redirect';
  return 'status-error';
}

function Placeholder() {
  // The core knows the platform; the user agent is a guess until it has answered.
  const os = useStore((s) => s.appInfo?.os);
  const mod = (os !== undefined ? os === 'macos' : /mac/i.test(navigator.userAgent)) ? 'Cmd' : 'Ctrl';
  return (
    <div className="response-placeholder">
      <IconSend size={64} stroke={1} />
      <dl>
        <dt>Send Request</dt>
        <dd>{mod} + Enter</dd>
        <dt>Save</dt>
        <dd>{mod} + S</dd>
        <dt>Close Tab</dt>
        <dd>{mod} + W</dd>
        <dt>Preferences</dt>
        <dd>{mod} + ,</dd>
      </dl>
    </div>
  );
}

export function ErrorBlock({ text }: { text: string }) {
  return <pre className="error-block">{text}</pre>;
}

/**
 * Pretty-printed text of the JSON bodies (null: not JSON). Kept out of the
 * component, which is unmounted at every tab switch: parsing and printing
 * megabytes of JSON again each time would freeze the window. Keyed by the
 * body object of the result, so an entry goes away with its result.
 */
const prettyCache = new WeakMap<ResponseBody, string | null>();

function looksJson(text: string, contentType: string): boolean {
  return contentType.includes('json') || /^\s*[[{]/.test(text);
}

/** Pretty-prints a body once; null when it is not JSON. */
function prettyOf(body: ResponseBody, contentType: string): string | null {
  const cached = prettyCache.get(body);
  if (cached !== undefined) return cached;
  let pretty: string | null = null;
  if (body.text !== undefined && looksJson(body.text, contentType)) {
    try {
      pretty = JSON.stringify(JSON.parse(body.text), null, 2);
    } catch {
      // Not valid JSON: shown as it is.
    }
  }
  prettyCache.set(body, pretty);
  return pretty;
}

export function BodyView({ call }: { call: Call }) {
  // Choice of the user, for as long as this view is mounted.
  const [raw, setRaw] = useState(false);
  const body = call.response.body;
  const contentType = (headerValue(call.response.headers, 'content-type') ?? '').toLowerCase();
  const text = body?.text;

  if (!body || body.size === 0) return <div className="pane-empty">No Body</div>;
  // Before `dropped`: the core never sent this content (budget of the run),
  // so running the same thing again would not bring it.
  if (body.omitted) {
    return (
      <div className="pane-empty">
        The body of this response ({formatSize(body.size)}) was not loaded: this run returned more than 100 MB of
        responses. Send this request alone to see it.
      </div>
    );
  }
  if (body.dropped) {
    return (
      <div className="pane-empty">
        The body of this response ({formatSize(body.size)}) is no longer kept in memory. Send the request again to
        see it.
      </div>
    );
  }
  if (text === undefined) {
    const image = contentType.startsWith('image/') && body.base64;
    return image ? (
      <div className="body-image">
        <img alt="Response" src={`data:${contentType.split(';')[0]};base64,${body.base64}`} />
      </div>
    ) : (
      <div className="pane-empty">
        Binary response ({formatSize(body.size)})
        {body.encoded && ', content encoding not decoded'}
      </div>
    );
  }
  // Pretty by default whatever the size (the core caps a body at 20 MB):
  // parsing and printing megabytes of JSON takes tens of milliseconds, once
  // (cached), whereas laying out the raw text, often one unbroken line of
  // several megabytes, takes seconds.
  const pretty = prettyOf(body, contentType);
  const showRaw = raw || pretty === null;
  const language: EditorLanguage =
    pretty !== null ? 'json' : /xml|html/.test(contentType) || /^\s*</.test(text) ? 'xml' : 'text';
  const shown = pretty !== null && !showRaw ? pretty : text;
  // Paths are offered for JSON and XML. HTML is left out: its text is not
  // read as a browser or Hurl reads it (unclosed elements, implied ones), so
  // the paths would be wrong.
  const html = contentType.includes('html') || /^\s*<(!doctype\s+html|html[\s>])/i.test(text);
  const paths = language === 'json' ? 'json' : language === 'xml' && !html ? 'xml' : null;
  const copy = (value: string) => navigator.clipboard.writeText(value).then(() => toast(`Copied ${value}`, 'success'));
  /** Right click on the body: the path of what is under the pointer, as "Copy XPath" of a browser. */
  const pathMenu = (event: MouseEvent, state: EditorState, pos: number): boolean => {
    const path = paths === 'json' ? jsonPathAt(state, pos) : paths === 'xml' ? xpathAt(state, pos) : null;
    if (path === null) return false;
    const query = paths === 'json' ? jsonPathQuery(path) : xpathQuery(path);
    const selection = state.sliceDoc(state.selection.main.from, state.selection.main.to);
    const items: MenuItem[] = [
      {
        label: paths === 'json' ? 'Copy JSONPath' : 'Copy XPath',
        icon: <IconRoute size={14} stroke={1.5} />,
        onClick: () => copy(path),
      },
      { label: 'Copy as Hurl Query', icon: <IconCode size={14} stroke={1.5} />, onClick: () => copy(query) },
    ];
    if (selection) {
      items.push({
        label: 'Copy Selection',
        icon: <IconCopy size={14} stroke={1.5} />,
        onClick: () => navigator.clipboard.writeText(selection),
      });
    }
    showMenu(event, items);
    return true;
  };
  return (
    <div className="body-view">
      <div className="pane-toolbar">
        {pretty !== null && (
          <div className="segmented">
            <button type="button" className={showRaw ? '' : 'on'} onClick={() => setRaw(false)}>
              Pretty
            </button>
            <button type="button" className={showRaw ? 'on' : ''} onClick={() => setRaw(true)}>
              Raw
            </button>
          </div>
        )}
        {body.truncated && <span className="muted">Truncated ({formatSize(body.size)} received)</span>}
        <div className="statusbar-spacer" />
        <button
          type="button"
          className="icon-btn"
          title="Copy"
          onClick={() => navigator.clipboard.writeText(shown).then(() => toast('Response copied', 'success'))}
        >
          <IconCopy size={15} stroke={1.5} />
        </button>
      </div>
      <div className="editor-box">
        <CodeEditor value={shown} language={language} readOnly onContextMenu={paths ? pathMenu : undefined} />
      </div>
    </div>
  );
}

export function HeadersTable({ headers }: { headers: KeyValue[] }) {
  if (headers.length === 0) return <div className="pane-empty">No Headers</div>;
  return (
    <table className="kv-table">
      <thead>
        <tr>
          <th>Name</th>
          <th>Value</th>
        </tr>
      </thead>
      <tbody>
        {headers.map((h, i) => (
          <tr key={i}>
            <td>{h.name}</td>
            <td>{h.value}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

const TIMINGS: [string, string][] = [
  ['name_lookup', 'DNS lookup'],
  ['connect', 'TCP connection'],
  ['app_connect', 'TLS handshake'],
  ['pre_transfer', 'Pre-transfer'],
  ['start_transfer', 'First byte'],
  ['total', 'Total'],
];

function Timeline({ result, meta }: { result?: EntryResult; meta?: RunMeta }) {
  return (
    <div className="timeline">
      {result && result.calls.length > 0 && <Waterfall groups={[{ calls: result.calls }]} />}
      {result?.calls.map((c, i) => (
        <details key={i} className="timeline-call" open={i === result.calls.length - 1}>
          <summary>
            <span className={`status ${statusClass(c.response.status)}`}>{c.response.status}</span>
            <span className="timeline-method">{c.request.method}</span>
            <span className="timeline-url">{c.request.url}</span>
          </summary>
          <h5>Request</h5>
          <pre>
            {c.request.method} {c.request.url}
            {'\n'}
            {c.request.headers.map((h) => `${h.name}: ${h.value}`).join('\n')}
          </pre>
          <h5>Response</h5>
          <pre>
            {c.response.http_version} {c.response.status}
            {'\n'}
            {c.response.headers.map((h) => `${h.name}: ${h.value}`).join('\n')}
          </pre>
          <h5>Timings</h5>
          <table className="kv-table timings">
            <tbody>
              {TIMINGS.filter(([key]) => typeof c.timings[key] === 'number').map(([key, label]) => (
                <tr key={key}>
                  <td>{label}</td>
                  <td>{((c.timings[key] as number) / 1000).toFixed(1)} ms</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      ))}
      {result?.curl_cmd && (
        <>
          <h5>curl</h5>
          <pre>{result.curl_cmd}</pre>
        </>
      )}
      {meta?.command && (
        <>
          <h5>Command</h5>
          <pre>{meta.command}</pre>
        </>
      )}
      {meta?.stderr && (
        <>
          <h5>Hurl output</h5>
          <pre>{meta.stderr}</pre>
        </>
      )}
    </div>
  );
}

/**
 * Asserts as they are listed. Hurl reports several asserts for one source
 * line (`HTTP 201`: the version, then the status code): consecutive asserts of
 * the same line and source are shown once when they all passed, and only the
 * failed ones otherwise (never hidden, each with its own message). An assert
 * without source is left alone: nothing tells that it is the same one.
 * The counters of the Tests tab are computed from this list too.
 */
export function displayedAsserts(asserts: AssertResult[]): AssertResult[] {
  const shown: AssertResult[] = [];
  for (let i = 0; i < asserts.length; ) {
    const first = asserts[i];
    let end = i + 1;
    if (first.source !== undefined) {
      while (end < asserts.length && asserts[end].line === first.line && asserts[end].source === first.source) end++;
    }
    const group = asserts.slice(i, end);
    const failed = group.filter((a) => !a.success);
    if (failed.length > 0) shown.push(...failed);
    else shown.push(first);
    i = end;
  }
  return shown;
}

export function AssertList({ result }: { result: EntryResult }) {
  if (result.asserts.length === 0) return <div className="muted">No assert.</div>;
  return (
    <ul className="assert-list">
      {displayedAsserts(result.asserts).map((a, i) => (
        <li key={i} className={a.success ? 'passed' : 'failed'}>
          {a.success ? <IconCheck size={15} stroke={2} /> : <IconX size={15} stroke={2} />}
          <div>
            <code>{a.source ?? `line ${a.line}`}</code>
            {!a.success && a.message && <pre>{a.message}</pre>}
          </div>
        </li>
      ))}
    </ul>
  );
}

/** Asserts, captures and errors of a request. */
export function EntryTests({ result }: { result: EntryResult }) {
  return (
    <>
      <h5>Asserts</h5>
      <AssertList result={result} />
      {result.captures.length > 0 && (
        <>
          <h5>Captures</h5>
          <table className="kv-table">
            <tbody>
              {result.captures.map((c, i) => (
                <tr key={i}>
                  <td>{c.name}</td>
                  <td>{typeof c.value === 'string' ? c.value : JSON.stringify(c.value)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
      {/* Failed asserts carry their own message: only the other errors are listed. */}
      {result.errors.length > 0 && (
        <>
          <h5>Errors</h5>
          <ErrorBlock text={result.errors.join('\n\n')} />
        </>
      )}
    </>
  );
}

function Tests({ result, meta }: { result?: EntryResult; meta?: RunMeta }) {
  return (
    <div className="tests">
      {result && <EntryTests result={result} />}
      {!result && meta && !meta.success && meta.errors && (
        <>
          <h5>Errors</h5>
          <ErrorBlock text={meta.errors} />
        </>
      )}
    </div>
  );
}

export function ResponsePane({ tab }: { tab: Tab }) {
  const run = useStore((s) => s.entryRuns[entryKey(tab.path, tab.index)]);
  const { patchTab, cancelRun } = useStore.getState();
  if (!run || (!run.result && !run.meta && !run.running)) return <Placeholder />;

  const { result, meta } = run;
  const call = result?.calls[result.calls.length - 1];
  // Same list as the rows of the Tests pane: the badge counts what is shown.
  const asserts = result ? displayedAsserts(result.asserts) : [];
  const failed = asserts.filter((a) => !a.success).length;
  const total = asserts.length;
  const tabs: PaneTab[] = [
    { key: 'response', label: 'Response' },
    { key: 'headers', label: 'Headers', count: call?.response.headers.length },
    { key: 'timeline', label: 'Timeline' },
    {
      key: 'tests',
      label: 'Tests',
      count: failed || total,
      error: failed > 0,
      dot: result ? result.errors.length > 0 : !!meta && !meta.success,
    },
  ];
  const pane = tabs.some((t) => t.key === tab.responsePane) ? tab.responsePane : 'response';

  return (
    <div className="response-pane">
      <PaneTabs
        tabs={tabs}
        active={pane}
        onSelect={(key) => patchTab(tab.id, { responsePane: key })}
        right={
          call && (
            <div className="response-status">
              <span className={`status ${statusClass(call.response.status)}`}>{call.response.status}</span>
              <span>{formatDuration(result!.time)}</span>
              <span>{formatSize(call.response.body?.size ?? 0)}</span>
            </div>
          )
        }
      />
      <div className="response-content">
        {pane === 'response' &&
          (call ? (
            <BodyView call={call} />
          ) : meta ? (
            <div className="pane-content">
              <p className="hint">
                {meta.cancelled ? 'The request was cancelled.' : 'Hurl could not get a response.'}
              </p>
              {(result?.errors.length ? result.errors.join('\n\n') : meta.errors) && (
                <ErrorBlock text={result?.errors.length ? result.errors.join('\n\n') : meta.errors} />
              )}
            </div>
          ) : null)}
        {pane === 'headers' && (
          <div className="pane-content">{call ? <HeadersTable headers={call.response.headers} /> : null}</div>
        )}
        {pane === 'timeline' && (
          <div className="pane-content">
            <Timeline result={result} meta={meta} />
          </div>
        )}
        {pane === 'tests' && (
          <div className="pane-content">
            <Tests result={result} meta={meta} />
          </div>
        )}
      </div>
      {run.running && (
        <div className="response-overlay">
          <IconLoader2 size={28} stroke={1.5} className="spin" />
          <span>Running hurl...</span>
          <button type="button" className="btn btn-outline" onClick={() => cancelRun(run.runId)}>
            Cancel Request
          </button>
        </div>
      )}
    </div>
  );
}
