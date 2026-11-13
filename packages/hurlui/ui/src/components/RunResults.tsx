// Results of a run of one file or of several: the same panes as the response
// of a request (Response, Headers, Timeline, Tests), over all the requests
// which were run.

import {
  IconChevronRight,
  IconCircleCheck,
  IconCircleX,
  IconExternalLink,
  IconLoader2,
} from '@tabler/icons-react';
import { Fragment, useMemo, useState, type ReactNode } from 'react';
import { entryKey, filesUnder, useStore } from '../store';
import type { Call, EntryResult, EntrySummary, RunResult } from '../types';
import {
  BodyView,
  displayedAsserts,
  EntryTests,
  ErrorBlock,
  formatDuration,
  formatSize,
  HeadersTable,
  statusClass,
} from './ResponsePane';
import { displayUrl, methodClass } from './Sidebar';
import { PaneTabs, type PaneTab } from './ui';
import { Waterfall, type WaterfallGroup } from './Waterfall';

/** Result of a file, as kept by a run: without the bodies of its responses. */
export interface FileRunResult {
  path: string;
  result: RunResult;
}

/** A request which was run. */
interface Item {
  key: string;
  file: FileRunResult;
  entry: EntryResult;
  /** Last call: the response of the request (the others are redirections and retries). */
  call?: Call;
  method: string;
  name: string;
}

/** Panes which list the requests, each one unfolding its content. */
type ListPane = 'response' | 'headers' | 'tests';

/** Above this number of requests, only the failed ones are unfolded in the Tests pane. */
const UNFOLD_ALL = 100;

/** Errors of a file which belong to none of its requests (syntax error, hurl not started...). */
function fileErrors(result: RunResult): string {
  return !result.success && result.errors && !result.entries.some((e) => e.failed) ? result.errors : '';
}

/**
 * Body of a response. A run does not keep the bodies (it may be of thousands
 * of requests): they are taken from the latest result of each request, which
 * holds its own as long as it is one of the last ones.
 */
function EntryBody({ item }: { item: Item }) {
  const run = useStore((s) => s.entryRuns[entryKey(item.file.path, item.entry.index)]);
  const same = run?.result && run.meta?.runId === item.file.result.runId && run.result.time === item.entry.time;
  const calls = same ? run.result!.calls : item.entry.calls;
  const call = calls[calls.length - 1];
  if (!call) return <div className="results-none">No response.</div>;
  return (
    <div className="results-body">
      <BodyView call={call} />
    </div>
  );
}

function EntrySection(props: {
  collection: string;
  item: Item;
  open: boolean;
  onToggle: () => void;
  children: () => ReactNode;
}) {
  const { item, open } = props;
  const openEntry = useStore((s) => s.openEntry);
  return (
    <section className={`results-entry ${item.entry.failed ? 'failed' : ''}`}>
      <div className="results-entry-head">
        <button type="button" className="results-entry-toggle" aria-expanded={open} onClick={props.onToggle}>
          <IconChevronRight size={14} stroke={1.5} className={`chevron ${open ? 'open' : ''}`} />
          {item.call ? (
            <span className={`status ${statusClass(item.call.response.status)}`}>{item.call.response.status}</span>
          ) : (
            <span className="status status-error">ERR</span>
          )}
          <span className={methodClass(item.method)}>{item.method}</span>
          <span className="results-entry-name">{item.name}</span>
          <span className="muted">{formatDuration(item.entry.time)}</span>
          <span className="muted">{formatSize(item.call?.response.body?.size ?? 0)}</span>
        </button>
        <button
          type="button"
          className="icon-btn"
          title="Open the request"
          onClick={() => openEntry(props.collection, item.file.path, item.entry.index)}
        >
          <IconExternalLink size={14} stroke={1.5} />
        </button>
      </div>
      {open && <div className="results-entry-content">{props.children()}</div>}
    </section>
  );
}

export function RunResults(props: {
  collection: string;
  /** The files which were run, in their order. */
  files: FileRunResult[];
  pane: string;
  onPane: (pane: string) => void;
  running?: boolean;
  onCancel?: () => void;
  /** Shown as long as there is no result. */
  empty: string;
}) {
  const { files, collection } = props;
  const tree = useStore((s) => s.collections.find((c) => c.path === collection)?.children);
  // What the requests are called in the sidebar.
  const summaries = useMemo(() => {
    const map = new Map<string, EntrySummary[]>();
    for (const file of filesUnder(tree ?? [])) map.set(file.path, file.entries);
    return map;
  }, [tree]);

  const items = useMemo(
    () =>
      files.flatMap((file) =>
        file.result.entries.map((entry, i): Item => {
          const call = entry.calls[entry.calls.length - 1];
          const summary = summaries.get(file.path)?.find((s) => s.index === entry.index);
          return {
            // A retried request is listed once per attempt.
            key: `${file.path}\n${i}`,
            file,
            entry,
            call,
            method: call?.request.method ?? summary?.method ?? '',
            name: summary?.name || displayUrl(summary?.url ?? call?.request.url ?? `Request ${entry.index}`),
          };
        }),
      ),
    [files, summaries],
  );

  // Requests unfolded in each pane, once the user has chosen; until then, the
  // ones which matter: the failed requests, and all of them in the Tests pane
  // as long as that stays readable.
  const [chosen, setChosen] = useState<Partial<Record<ListPane, Set<string>>>>({});
  const unfolded = (pane: ListPane): Set<string> => {
    const set = chosen[pane];
    if (set) return set;
    const failed = items.filter((i) => i.entry.failed).map((i) => i.key);
    if (pane === 'tests' && items.length <= UNFOLD_ALL) return new Set(items.map((i) => i.key));
    return new Set(failed.length > 0 || items.length === 0 ? failed : [items[0].key]);
  };

  if (files.length === 0 && !props.running) {
    return (
      <div className="response-pane">
        <div className="pane-empty">{props.empty}</div>
      </div>
    );
  }

  const asserts = items.flatMap((i) => displayedAsserts(i.entry.asserts));
  const failedAsserts = asserts.filter((a) => !a.success).length;
  const failedRequests = items.filter((i) => i.entry.failed).length;
  const duration = files.reduce((sum, f) => sum + f.result.durationMs, 0);
  const size = items.reduce((sum, i) => sum + (i.call?.response.body?.size ?? 0), 0);
  const tabs: PaneTab[] = [
    { key: 'response', label: 'Response', count: items.length },
    { key: 'headers', label: 'Headers' },
    { key: 'timeline', label: 'Timeline' },
    {
      key: 'tests',
      label: 'Tests',
      count: failedAsserts || asserts.length,
      error: failedAsserts > 0,
      dot: files.some((f) => !f.result.success),
    },
  ];
  const pane = tabs.some((t) => t.key === props.pane) ? props.pane : 'tests';
  const relative = (path: string) => path.slice(collection.length + 1) || path;

  const list = (name: ListPane, content: (item: Item) => ReactNode) => {
    const open = unfolded(name);
    const toggle = (key: string) => {
      const next = new Set(open);
      if (!next.delete(key)) next.add(key);
      setChosen({ ...chosen, [name]: next });
    };
    const setAll = (all: boolean) => setChosen({ ...chosen, [name]: new Set(all ? items.map((i) => i.key) : []) });
    return (
      <div className="pane-content results">
        {items.length > 1 && (
          <div className="results-toolbar">
            <button type="button" className="link-btn" onClick={() => setAll(true)}>
              Expand all
            </button>
            <button type="button" className="link-btn" onClick={() => setAll(false)}>
              Collapse all
            </button>
          </div>
        )}
        {files.map((file) => {
          const errors = fileErrors(file.result);
          return (
            <div key={file.path} className="results-file">
              {files.length > 1 && (
                <h5 className="results-file-head" title={file.path}>
                  {file.result.success ? (
                    <IconCircleCheck size={15} stroke={1.5} className="passed" />
                  ) : (
                    <IconCircleX size={15} stroke={1.5} className="failed" />
                  )}
                  <span>{relative(file.path)}</span>
                  <span className="muted">
                    {file.result.entries.length} req · {formatDuration(file.result.durationMs)}
                  </span>
                </h5>
              )}
              {file.result.cancelled && <p className="hint">The run of this file was cancelled.</p>}
              {errors && <ErrorBlock text={errors} />}
              {items
                .filter((i) => i.file === file)
                .map((item) => (
                  <EntrySection
                    key={item.key}
                    collection={collection}
                    item={item}
                    open={open.has(item.key)}
                    onToggle={() => toggle(item.key)}
                  >
                    {() => content(item)}
                  </EntrySection>
                ))}
            </div>
          );
        })}
      </div>
    );
  };

  // One waterfall for the whole run, as the timeline of the HTML report of
  // Hurl: the calls of all the requests, file after file.
  const timeline = (): WaterfallGroup[] =>
    files.map((file) => ({
      title: files.length > 1 ? relative(file.path) : undefined,
      calls: file.result.entries.flatMap((e) => e.calls),
    }));

  return (
    <div className="response-pane">
      <PaneTabs
        tabs={tabs}
        active={pane}
        onSelect={props.onPane}
        right={
          items.length > 0 && (
            <div className="response-status">
              <span className={failedRequests > 0 ? 'failed' : 'passed'}>
                {failedRequests > 0
                  ? `${failedRequests} of ${items.length} failed`
                  : `${items.length} request${items.length === 1 ? '' : 's'} passed`}
              </span>
              <span>{formatDuration(duration)}</span>
              <span>{formatSize(size)}</span>
            </div>
          )
        }
      />
      <div className="response-content">
        {pane === 'response' &&
          list('response', (item) => (
            <>
              <EntryBody item={item} />
              {item.entry.errors.length > 0 && !item.call && <ErrorBlock text={item.entry.errors.join('\n\n')} />}
            </>
          ))}
        {pane === 'headers' &&
          list('headers', (item) =>
            item.call ? (
              <HeadersTable headers={item.call.response.headers} />
            ) : (
              <div className="results-none">No response.</div>
            ),
          )}
        {pane === 'timeline' && (
          <div className="pane-content timeline">
            <Waterfall groups={timeline()} />
            {files.map((file) =>
              file.result.command ? (
                <Fragment key={file.path}>
                  <h5>{files.length > 1 ? `Command · ${relative(file.path)}` : 'Command'}</h5>
                  <pre>{file.result.command}</pre>
                </Fragment>
              ) : null,
            )}
          </div>
        )}
        {pane === 'tests' &&
          list('tests', (item) => (
            <div className="tests">
              <EntryTests result={item.entry} />
            </div>
          ))}
      </div>
      {props.running && (
        <div className="response-overlay">
          <IconLoader2 size={28} stroke={1.5} className="spin" />
          <span>Running hurl...</span>
          {props.onCancel && (
            <button type="button" className="btn btn-outline" onClick={props.onCancel}>
              Cancel
            </button>
          )}
        </div>
      )}
    </div>
  );
}
