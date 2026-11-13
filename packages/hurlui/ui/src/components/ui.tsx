// Small UI building blocks: dialog, context menu, toasts, tabs, tables, windowed list, split pane.

import { IconPlus, IconTrash } from '@tabler/icons-react';
import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type Key,
  type ReactNode,
} from 'react';
import { flushSync } from 'react-dom';
import { closeMenu, showMenu, useUi, type MenuItem } from '../ui';

// --- Dialog -------------------------------------------------------------------

export function DialogHost() {
  const dialog = useUi((s) => s.dialog);
  const [values, setValues] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!dialog) return;
    setValues(Object.fromEntries((dialog.fields ?? []).map((f) => [f.name, f.initial ?? ''])));
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && dialog.resolve(null);
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [dialog]);

  if (!dialog) return null;
  const fields = dialog.fields ?? [];
  const valid = fields.every((f) => f.optional || (values[f.name] ?? '').trim() !== '');
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (valid) dialog.resolve(values);
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && dialog.resolve(null)}>
      <form className="modal" onSubmit={submit} role="dialog" aria-label={dialog.title}>
        <div className="modal-title">{dialog.title}</div>
        <div className="modal-body">
          {dialog.message && <p className="modal-message">{dialog.message}</p>}
          {fields.map((f, i) => (
            <label key={f.name} className="field">
              <span>{f.label}</span>
              {f.options ? (
                <select
                  value={values[f.name] ?? ''}
                  onChange={(e) => setValues({ ...values, [f.name]: e.target.value })}
                >
                  {f.options.map((o) => (
                    <option key={o}>{o}</option>
                  ))}
                </select>
              ) : (
                <input
                  autoFocus={i === 0}
                  spellCheck={false}
                  placeholder={f.placeholder}
                  value={values[f.name] ?? ''}
                  onChange={(e) => setValues({ ...values, [f.name]: e.target.value })}
                />
              )}
            </label>
          ))}
        </div>
        <div className="modal-footer">
          <button type="button" className="btn btn-ghost" onClick={() => dialog.resolve(null)}>
            Cancel
          </button>
          <button
            type="submit"
            className={`btn ${dialog.danger ? 'btn-danger' : 'btn-primary'}`}
            disabled={!valid}
            autoFocus={fields.length === 0}
          >
            {dialog.confirmLabel ?? 'OK'}
          </button>
        </div>
      </form>
    </div>
  );
}

// --- Context menu / dropdown ----------------------------------------------------

export function MenuHost() {
  const menu = useUi((s) => s.menu);
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<CSSProperties>({});

  // Keeps the menu inside the window.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const { width, height } = ref.current.getBoundingClientRect();
    setPosition({
      left: Math.max(4, Math.min(menu.x, window.innerWidth - width - 4)),
      top: Math.max(4, Math.min(menu.y, window.innerHeight - height - 4)),
    });
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && closeMenu();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [menu]);

  if (!menu) return null;
  return (
    <div
      className="menu-backdrop"
      onMouseDown={closeMenu}
      onContextMenu={(e) => {
        e.preventDefault();
        closeMenu();
      }}
    >
      <div className="menu" ref={ref} style={position} role="menu" onMouseDown={(e) => e.stopPropagation()}>
        {menu.items.map((item, i) =>
          item.separator ? (
            <div key={i} className="menu-separator" />
          ) : (
            <button
              key={i}
              role="menuitem"
              className={`menu-item ${item.danger ? 'danger' : ''}`}
              disabled={item.disabled}
              onClick={() => {
                closeMenu();
                item.onClick?.();
              }}
            >
              {item.icon && <span className="menu-icon">{item.icon}</span>}
              {item.label}
            </button>
          ),
        )}
      </div>
    </div>
  );
}

/** A button opening a menu below itself. */
export function Dropdown(props: {
  items: MenuItem[];
  children: ReactNode;
  className?: string;
  title?: string;
  align?: 'left' | 'right';
}) {
  const ref = useRef<HTMLButtonElement>(null);
  const open = () => {
    const r = ref.current!.getBoundingClientRect();
    // Right aligned menus are clamped to the window by the host.
    showMenu({ clientX: props.align === 'right' ? r.right - 180 : r.left, clientY: r.bottom + 4 }, props.items);
  };
  return (
    <button type="button" ref={ref} className={props.className} title={props.title} onClick={open}>
      {props.children}
    </button>
  );
}

// --- Toasts ---------------------------------------------------------------------

export function ToastHost() {
  const toasts = useUi((s) => s.toasts);
  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast-${t.kind}`} role="status">
          {t.message}
        </div>
      ))}
    </div>
  );
}

// --- Pane tabs ------------------------------------------------------------------

export interface PaneTab {
  key: string;
  label: string;
  count?: number;
  dot?: boolean;
  error?: boolean;
}

export function PaneTabs(props: {
  tabs: PaneTab[];
  active: string;
  onSelect: (key: string) => void;
  right?: ReactNode;
}) {
  return (
    <div className="pane-tabs" role="tablist">
      {props.tabs.map((t) => (
        <button
          key={t.key}
          role="tab"
          aria-selected={t.key === props.active}
          className={`pane-tab ${t.key === props.active ? 'active' : ''}`}
          onClick={() => props.onSelect(t.key)}
        >
          {t.label}
          {t.count ? <sup className={t.error ? 'error' : ''}>{t.count}</sup> : null}
          {t.dot && !t.count ? <span className="pane-tab-dot" /> : null}
        </button>
      ))}
      {props.right && <div className="pane-tabs-right">{props.right}</div>}
    </div>
  );
}

// --- Editable table ---------------------------------------------------------------

export interface Column<T> {
  key: keyof T & string;
  label: string;
  placeholder?: string;
  type?: 'text' | 'checkbox';
  /** Id of a <datalist> providing suggestions. */
  list?: string;
  width?: string;
  masked?: (row: T) => boolean;
  /** Control of the cell, when it is not a plain input. */
  render?: (row: T, onChange: (value: string) => void) => ReactNode;
}

export function EditableTable<T extends object>(props: {
  columns: Column<T>[];
  rows: T[];
  onChange: (rows: T[]) => void;
  makeRow: () => T;
  addLabel: string;
  /** Adjusts a row after an edit of one of its cells: `previous` is the row before the edit. */
  onCellChange?: (row: T, key: string, previous: T) => T;
}) {
  const { columns, rows, onChange } = props;
  const setCell = (index: number, key: string, value: string | boolean) =>
    onChange(
      rows.map((r, i) => {
        if (i !== index) return r;
        const row = { ...r, [key]: value };
        return props.onCellChange ? props.onCellChange(row, key, r) : row;
      }),
    );

  return (
    <div className="table-editor">
      <table>
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} style={{ width: c.width }}>
                {c.label}
              </th>
            ))}
            <th style={{ width: '34px' }} />
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i}>
              {columns.map((c) => {
                const value = (row as Record<string, unknown>)[c.key];
                return (
                  <td key={c.key} className={c.type === 'checkbox' ? 'cell-check' : ''}>
                    {c.render ? (
                      c.render(row, (v) => setCell(i, c.key, v))
                    ) : c.type === 'checkbox' ? (
                      <input
                        type="checkbox"
                        aria-label={c.label}
                        checked={Boolean(value)}
                        onChange={(e) => setCell(i, c.key, e.target.checked)}
                      />
                    ) : (
                      <input
                        type={c.masked?.(row) ? 'password' : 'text'}
                        aria-label={c.label}
                        spellCheck={false}
                        autoComplete="off"
                        list={c.list}
                        placeholder={c.placeholder}
                        value={String(value ?? '')}
                        onChange={(e) => setCell(i, c.key, e.target.value)}
                      />
                    )}
                  </td>
                );
              })}
              <td className="cell-action">
                <button
                  type="button"
                  className="icon-btn"
                  title="Delete"
                  onClick={() => onChange(rows.filter((_, j) => j !== i))}
                >
                  <IconTrash size={15} stroke={1.5} />
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <button type="button" className="link-btn" onClick={() => onChange([...rows, props.makeRow()])}>
        <IconPlus size={14} stroke={1.5} /> {props.addLabel}
      </button>
    </div>
  );
}

// --- Windowed list ----------------------------------------------------------------

/** Rows mounted above and below the visible ones: scrolling does not show blanks. */
const OVERSCAN = 8;

/**
 * Scrollable list of rows of the same height which only mounts the rows in
 * view (plus a few around), whatever the number of items. The rows must be
 * exactly `rowHeight` pixels high: the value is given to the styles as
 * `--row-height`, on the scroll container. The container must not have a top
 * padding (the offsets are counted from its top edge).
 */
export function VirtualList(props: {
  count: number;
  /** Pixels. */
  rowHeight: number;
  renderRow: (index: number) => ReactNode;
  /** Stable identity of a row: a row scrolled or moved keeps its component. */
  rowKey: (index: number) => Key;
  className?: string;
  role?: string;
}) {
  const { count, rowHeight } = props;
  const ref = useRef<HTMLDivElement>(null);
  // In rows rather than pixels: scrolling inside a row renders nothing.
  const [view, setView] = useState({ start: 0, size: 0 });
  // Last window measured, to tell without React whether it moved.
  const measured = useRef(view);

  // `now`: the rows are rendered before this returns. Only from a browser
  // callback: React cannot flush while it is rendering or committing.
  const measure = useCallback(
    (now = false) => {
      const el = ref.current;
      if (!el) return;
      const start = Math.max(0, Math.floor(el.scrollTop / rowHeight));
      // One more than what fits: the first and the last rows are usually cut.
      const size = Math.ceil(el.clientHeight / rowHeight) + 1;
      if (measured.current.start === start && measured.current.size === size) return;
      const next = { start, size };
      measured.current = next;
      if (now) flushSync(() => setView(next));
      else setView(next);
    },
    [rowHeight],
  );

  // The browser dispatches `scroll` and the resize observations right before
  // it paints, outside of any React render. An update left to the scheduler
  // there is committed after that paint: a jump past the overscan showed one
  // frame of blank rows. Rendering at once puts the rows in the same frame.
  const measureNow = useCallback(() => measure(true), [measure]);

  useLayoutEffect(() => {
    measure();
    const observer = new ResizeObserver(measureNow);
    observer.observe(ref.current!);
    return () => observer.disconnect();
  }, [measure, measureNow]);

  // A shorter list makes the browser pull the scroll offset back: read it
  // again before painting.
  useLayoutEffect(() => measure(), [measure, count]);

  // Until then, same clamping as the browser: never a window past the end.
  const start = Math.min(view.start, Math.max(0, count - view.size));
  const first = Math.max(0, start - OVERSCAN);
  const last = Math.min(count, start + view.size + OVERSCAN);
  const rows: ReactNode[] = [];
  for (let i = first; i < last; i++) rows.push(<Fragment key={props.rowKey(i)}>{props.renderRow(i)}</Fragment>);

  return (
    <div
      ref={ref}
      className={`virtual-list ${props.className ?? ''}`}
      role={props.role}
      style={{ '--row-height': `${rowHeight}px` } as CSSProperties}
      onScroll={measureNow}
    >
      {/* As high as the whole list; the padding stands for the rows above the window. */}
      <div
        className="virtual-list-rows"
        role="presentation"
        style={{ boxSizing: 'border-box', height: count * rowHeight, paddingTop: first * rowHeight }}
      >
        {rows}
      </div>
    </div>
  );
}

// --- Split pane -------------------------------------------------------------------

export function SplitPane(props: {
  direction: 'horizontal' | 'vertical';
  first: ReactNode;
  second: ReactNode;
  initial?: number;
}) {
  const initial = props.initial ?? 0.5;
  const [fraction, setFraction] = useState(initial);
  const ref = useRef<HTMLDivElement>(null);
  const horizontal = props.direction === 'horizontal';

  const startDrag = (e: React.PointerEvent) => {
    e.preventDefault();
    const box = ref.current!.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      const f = horizontal ? (ev.clientX - box.left) / box.width : (ev.clientY - box.top) / box.height;
      setFraction(Math.min(0.85, Math.max(0.15, f)));
    };
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
    <div className={`split split-${props.direction}`} ref={ref}>
      <div className="split-first" style={{ flexBasis: `${fraction * 100}%` }}>
        {props.first}
      </div>
      <div
        className="split-handle"
        onPointerDown={startDrag}
        onDoubleClick={() => setFraction(initial)}
        role="separator"
        aria-orientation={horizontal ? 'vertical' : 'horizontal'}
      />
      <div className="split-second">{props.second}</div>
    </div>
  );
}
