// Options of `hurl` in the preferences. Nothing here knows any option: the
// controls are built from the catalog of options shared with Hurl (see
// `packages/hurlui_core/src/options`), so an option added to Hurl shows up
// here without any change.

import { useMemo, useState } from 'react';
import { useStore } from '../store';
import {
  isSet,
  isSettable,
  isTemplate,
  sectionValueError,
  sectionValueFor,
  type HurlOption,
  type KeyValue,
  type OptionValue,
} from '../types';

/** What `hurl --help` shows for the value of an option. */
function placeholder(option: HurlOption): string {
  const value = option.value ?? '';
  if (option.multi) return `${value}, one per line`;
  return option.default === null ? value : `${value} (default: ${option.default})`;
}

function matches(option: HurlOption, filter: string): boolean {
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  const text = `--${option.long} ${option.help ?? ''} ${option.value ?? ''}`.toLowerCase();
  return words.every((w) => text.includes(w));
}

/** Help of an option, with the options it cannot be used with. */
function title(option: HurlOption): string {
  const conflicts = option.conflicts.map((long) => `--${long}`).join(', ');
  return conflicts ? `${option.description}\n\nCannot be used with ${conflicts}.` : option.description;
}

function OptionName({ option }: { option: HurlOption }) {
  return (
    <code>
      {option.short ? `-${option.short}, ` : ''}--{option.long}
    </code>
  );
}

/** Sets an option, or unsets it. */
type SetOption = (long: string, value: OptionValue | undefined) => void;

/** Checkbox of a flag of `hurl`. */
function FlagCheck({ option, value, setOption }: { option: HurlOption; value?: OptionValue; setOption: SetOption }) {
  return (
    <label className="check" title={title(option)}>
      <input type="checkbox" checked={value === true} onChange={(e) => setOption(option.long, e.target.checked)} />
      {option.help ?? option.long} <OptionName option={option} />
    </label>
  );
}

/**
 * Checkbox of a flag of `hurl` in the preferences, by long name. Renders
 * nothing if this version of Hurl has no such flag.
 */
export function OptionCheck({ long }: { long: string }) {
  const option = useStore((s) => s.hurlOptions?.options.find((o) => o.long === long));
  const value = useStore((s) => s.prefs.options[long]);
  const setOption = useStore((s) => s.setOption);
  if (!option || option.kind !== 'flag' || !isSettable(option)) return null;
  return <FlagCheck option={option} value={value} setOption={setOption} />;
}

/** Control of an option which takes a value. */
function OptionInput({ option, value, setOption }: { option: HurlOption; value?: OptionValue; setOption: SetOption }) {
  const id = `hurl-option-${option.long}`;
  const label = (
    <label htmlFor={id} title={title(option)}>
      {option.help ?? option.long}
      <br />
      <OptionName option={option} />
    </label>
  );
  if (option.multi) {
    const lines = Array.isArray(value) ? value : typeof value === 'string' ? [value] : [];
    return (
      <>
        {label}
        <textarea
          id={id}
          spellCheck={false}
          rows={Math.max(2, lines.length)}
          placeholder={placeholder(option)}
          value={lines.join('\n')}
          onChange={(e) => setOption(option.long, e.target.value.split('\n'))}
        />
      </>
    );
  }
  const text = typeof value === 'string' ? value : '';
  if (option.kind === 'choice') {
    return (
      <>
        {label}
        <select id={id} value={text} onChange={(e) => setOption(option.long, e.target.value)}>
          <option value="">{option.default === null ? 'Not set' : `Default (${option.default})`}</option>
          {option.choices.map((choice) => (
            <option key={choice} value={choice}>
              {choice}
            </option>
          ))}
        </select>
      </>
    );
  }
  return (
    <>
      {label}
      <input
        id={id}
        spellCheck={false}
        type={option.kind === 'integer' ? 'number' : 'text'}
        min={option.kind === 'integer' && option.min !== null ? option.min : undefined}
        placeholder={placeholder(option)}
        value={text}
        onChange={(e) => setOption(option.long, e.target.value)}
      />
    </>
  );
}

/**
 * All the options of `hurl` which can be set for a run, under the headings
 * of `hurl --help`: the ones of the preferences, of a folder or of a file.
 */
export function HurlOptionsForm({ values, setOption }: { values: Record<string, OptionValue>; setOption: SetOption }) {
  const catalog = useStore((s) => s.hurlOptions);
  const [filter, setFilter] = useState('');
  const [onlySet, setOnlySet] = useState(false);
  const settable = useMemo(() => catalog?.options.filter(isSettable) ?? [], [catalog]);

  if (!catalog) return <p className="hint failed">The options of hurl could not be loaded.</p>;

  const count = settable.filter((o) => isSet(values[o.long])).length;
  const shown = settable.filter((o) => matches(o, filter) && (!onlySet || isSet(values[o.long])));
  return (
    <div className="hurl-options">
      <div className="form-row">
        <input
          aria-label="Filter the options of hurl"
          spellCheck={false}
          placeholder={`Filter ${settable.length} options`}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <label className="check">
          <input type="checkbox" checked={onlySet} onChange={(e) => setOnlySet(e.target.checked)} />
          Only the {count} set
        </label>
      </div>
      {[...catalog.headings, null].map((heading) => {
        // Last, the options which have no heading in `hurl --help`.
        const options = shown.filter((o) =>
          heading === null ? o.heading === null || !catalog.headings.includes(o.heading) : o.heading === heading,
        );
        if (options.length === 0) return null;
        const flags = options.filter((o) => o.kind === 'flag');
        const inputs = options.filter((o) => o.kind !== 'flag');
        return (
          <section key={heading ?? ''}>
            <h5 className="hurl-options-heading">{heading ?? 'Options'}</h5>
            {flags.length > 0 && (
              <div className="form-stack">
                {flags.map((o) => (
                  <FlagCheck key={o.long} option={o} value={values[o.long]} setOption={setOption} />
                ))}
              </div>
            )}
            {inputs.length > 0 && (
              <div className="form-grid">
                {inputs.map((o) => (
                  <OptionInput key={o.long} option={o} value={values[o.long]} setOption={setOption} />
                ))}
              </div>
            )}
          </section>
        );
      })}
      {shown.length === 0 && <p className="hint">No option matches.</p>}
    </div>
  );
}

// --- Options of a request (`[Options]` section) -------------------------------------

/** The option of an `[Options]` section with this name, in the catalog. */
function sectionOption(name: string): HurlOption | undefined {
  const long = name.trim();
  return useStore.getState().hurlOptions?.options.find((o) => o.section && o.long === long);
}

/** Row of the options of a request after an edit: choosing an option gives a value which suits it. */
export function sectionRow(row: KeyValue, key: string, previous: KeyValue): KeyValue {
  const catalog = useStore.getState().hurlOptions;
  if (key !== 'name' || !catalog) return row;
  const option = sectionOption(row.name);
  // The `true` given to a boolean does not follow the name to another kind of
  // option (`netrc`, on the way to `netrc-file`).
  const given = sectionOption(previous.name)?.section_kind === 'boolean' && row.value === 'true';
  const value = given && option?.section_kind !== 'boolean' ? '' : row.value;
  return { ...row, value: option ? sectionValueFor(option, value, catalog) : value };
}

/** Entry of the lists of values which turns the control into a free field, for a `{{variable}}`. */
const TEMPLATE = '{{variable}}';

/**
 * Value of an option of a request, as the option takes it: a list for a
 * boolean or a choice, digits for an integer, a checked field for the other
 * kinds. Hurl accepts a `{{variable}}` for any option: such a value is edited
 * as text. An option which is not in the catalog takes any text.
 */
export function SectionValue(props: { name: string; value: string; onChange: (value: string) => void }) {
  const { value, onChange } = props;
  const catalog = useStore((s) => s.hurlOptions);
  const option = catalog?.options.find((o) => o.section && o.long === props.name.trim());
  const error = option && catalog ? sectionValueError(option, value, catalog) : null;
  const kind = option?.section_kind ?? 'text';
  const common = { 'aria-label': 'Value', 'aria-invalid': error !== null, title: error ?? option?.help ?? undefined };

  const values = kind === 'boolean' ? ['true', 'false'] : kind === 'choice' ? (option?.choices ?? []) : null;
  if (values && !isTemplate(value)) {
    return (
      <select {...common} value={value} onChange={(e) => onChange(e.target.value === TEMPLATE ? '{{}}' : e.target.value)}>
        {/* A value which is not one of the list is shown, not replaced. */}
        {!values.includes(value) && <option value={value}>{value}</option>}
        {values.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
        <option value={TEMPLATE}>{TEMPLATE}</option>
      </select>
    );
  }
  const placeholder =
    kind === 'integer'
      ? `${option?.value ?? 'Integer'} (integer)`
      : kind === 'duration'
        ? `${option?.value ?? 'Duration'}, or with a unit: ${catalog?.duration_units.join(', ')}`
        : (option?.value ?? 'Value');
  return (
    <input
      {...common}
      type="text"
      inputMode={kind === 'integer' ? 'numeric' : undefined}
      spellCheck={false}
      autoComplete="off"
      placeholder={placeholder}
      value={value}
      onChange={(e) => {
        const next = e.target.value;
        // An integer is made of digits: anything else is not typed, but a `{{variable}}`.
        if (kind === 'integer' && !/^-?\d*$/.test(next) && !next.startsWith('{')) return;
        onChange(next);
      }}
    />
  );
}

/** Options as they are written on a command line, for display. */
export function commandLine(values: Record<string, OptionValue>): string {
  return Object.entries(values)
    .flatMap(([long, value]) =>
      value === true ? [`--${long}`] : (Array.isArray(value) ? value : [String(value)]).map((v) => `--${long} ${v}`),
    )
    .join('  ');
}
