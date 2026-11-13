// Waterfall of HTTP calls, as drawn by the timeline of `hurl --report-html`
// (packages/hurl/src/report/html/timeline/waterfall.rs): one row per call,
// placed on a common time axis, with the same steps and the same colours. The
// calls are the ones of a request, of a file or of several files.

import type { Call } from '../types';

/** Steps of a call, in their order, with the colours of the HTML report of Hurl. */
const STEPS = [
  { key: 'dns', label: 'DNS lookup', color: '#1d9688' },
  { key: 'tcp', label: 'TCP handshake', color: '#fa7f03' },
  { key: 'ssl', label: 'SSL handshake', color: '#9933ff' },
  { key: 'wait', label: 'Wait', color: '#18c852' },
  { key: 'transfer', label: 'Data transfer', color: '#36a9f4' },
] as const;

type StepKey = (typeof STEPS)[number]['key'];

/** A step of a call: start and duration in microseconds, from the start of the call. */
interface Step {
  key: StepKey;
  start: number;
  duration: number;
}

/** Calls shown together, under a title when there are several groups (the requests of a file, for instance). */
export interface WaterfallGroup {
  title?: string;
  calls: Call[];
}

interface Row {
  call: Call;
  /** Title of the group, on its first call. */
  title?: string;
  /** Microseconds, from the start of the first call. */
  start: number;
  total: number;
  steps: Step[];
}

/** Microseconds since the epoch of a date such as `2026-01-31T10:20:30.123456Z` (`Date` stops at milliseconds). */
export function parseMicros(date: unknown): number | null {
  if (typeof date !== 'string') return null;
  const m = /^(.*?)(?:\.(\d+))?(Z|[+-]\d{2}:?\d{2})?$/.exec(date);
  if (!m) return null;
  const millis = Date.parse(`${m[1]}${m[3] ?? 'Z'}`);
  if (Number.isNaN(millis)) return null;
  const fraction = Number(`0.${m[2] ?? '0'}`);
  return millis * 1000 + Math.round(fraction * 1_000_000);
}

const micros = (value: unknown): number => (typeof value === 'number' && value > 0 ? value : 0);

/**
 * Steps of a call, from its timings (the ones of libcurl, in microseconds
 * since the start of the call). Same computation as the HTML report of Hurl:
 * a step which did not happen (no TLS, connection reused...) has no duration
 * and is not drawn.
 */
export function callSteps(timings: Call['timings']): Step[] {
  const t = (key: string) => micros(timings[key]);
  const between = (key: StepKey, from: number, to: number): Step => ({ key, start: from, duration: to - from });
  return [
    between('dns', 0, t('name_lookup')),
    between('tcp', t('name_lookup'), t('connect')),
    between('ssl', t('connect'), t('app_connect')),
    between('wait', t('pre_transfer'), t('start_transfer')),
    between('transfer', t('start_transfer'), t('total')),
  ].filter((step) => step.duration > 0);
}

/** Calls on a common time axis: `null` when their timings do not allow it. */
function rows(groups: WaterfallGroup[]): { rows: Row[]; duration: number } | null {
  let origin: number | null = null;
  let offset = 0;
  const result: Row[] = [];
  for (const group of groups) {
    group.calls.forEach((call, i) => {
      const total = micros(call.timings.total);
      const begin = parseMicros(call.timings.begin_call);
      origin ??= begin;
      // Without dates, the calls are put one after the other.
      const start = begin !== null && origin !== null ? Math.max(0, begin - origin) : offset;
      result.push({ call, title: i === 0 ? group.title : undefined, start, total, steps: callSteps(call.timings) });
      offset = start + total;
    });
  }
  const duration = Math.max(0, ...result.map((r) => r.start + r.total));
  return duration > 0 ? { rows: result, duration } : null;
}

/** A duration in microseconds, in the unit which reads best. */
export function formatMicros(value: number): string {
  if (value < 1000) return `${Math.round(value)} µs`;
  if (value < 1_000_000) return `${(value / 1000).toFixed(value < 10_000 ? 2 : 1)} ms`;
  return `${(value / 1_000_000).toFixed(2)} s`;
}

/**
 * Ticks of the time axis: "nice" values (1, 2 or 5 times a power of ten), as
 * the grid of the HTML report of Hurl, at most `count` of them.
 */
export function ticks(duration: number, count = 6): number[] {
  if (duration <= 0) return [];
  const raw = duration / count;
  const power = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * power).find((s) => s >= raw) ?? 10 * power;
  const values: number[] = [];
  for (let t = 0; t < duration; t += step) values.push(t);
  return values;
}

/** Label of a tick: all the ticks of an axis are in the same unit. */
function tickLabel(value: number, duration: number): string {
  if (value === 0) return '0';
  if (duration < 1000) return `${value} µs`;
  if (duration < 1_000_000) return `${+(value / 1000).toFixed(3)} ms`;
  return `${+(value / 1_000_000).toFixed(3)} s`;
}

function path(url: string): string {
  try {
    const u = new URL(url);
    return `${u.pathname}${u.search}` || '/';
  } catch {
    return url;
  }
}

export function Waterfall({ groups }: { groups: WaterfallGroup[] }) {
  const data = rows(groups);
  if (!data) return null;
  const { duration } = data;
  const percent = (value: number) => `${(value / duration) * 100}%`;
  const statusClass = (status: number) =>
    status >= 200 && status < 300 ? 'status-ok' : status < 400 ? 'status-redirect' : 'status-error';
  const used = new Set(data.rows.flatMap((r) => r.steps.map((s) => s.key)));
  const label = (key: StepKey) => STEPS.find((s) => s.key === key)!.label;
  const axis = ticks(duration);

  return (
    <div className="waterfall">
      <div className="waterfall-grid" role="img" aria-label={`Waterfall of ${data.rows.length} HTTP calls`}>
        <div className="waterfall-head" />
        <div className="waterfall-axis">
          {axis.map((t) => (
            <span key={t} style={{ left: percent(t) }}>
              {tickLabel(t, duration)}
            </span>
          ))}
        </div>
        <div className="waterfall-head" />
        {data.rows.map((row, i) => (
          <div key={i} className="waterfall-row">
            {row.title !== undefined && (
              <div className="waterfall-title" title={row.title}>
                {row.title}
              </div>
            )}
            <div className="waterfall-call" title={`${row.call.request.method} ${row.call.request.url}`}>
              <span className={`status ${statusClass(row.call.response.status)}`}>
                {row.call.response.status}
              </span>
              <span className="waterfall-method">{row.call.request.method}</span>
              <span className="waterfall-url">{path(row.call.request.url)}</span>
            </div>
            <div className="waterfall-track">
              {axis.map((t) => (
                <i key={t} className="waterfall-tick" style={{ left: percent(t) }} />
              ))}
              {row.steps.map((step) => (
                <span
                  key={step.key}
                  className="waterfall-step"
                  title={`${label(step.key)}: ${formatMicros(step.duration)}`}
                  style={{
                    left: percent(row.start + step.start),
                    width: percent(step.duration),
                    background: STEPS.find((s) => s.key === step.key)!.color,
                  }}
                />
              ))}
            </div>
            <div className="waterfall-total" title="Total">
              {formatMicros(row.total)}
            </div>
          </div>
        ))}
      </div>
      <ul className="waterfall-legend">
        {STEPS.filter((s) => used.has(s.key)).map((s) => (
          <li key={s.key}>
            <i style={{ background: s.color }} />
            {s.label}
          </li>
        ))}
      </ul>
    </div>
  );
}
