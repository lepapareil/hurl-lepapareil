// Types shared with the Rust core (`packages/hurlui_core`).

export interface KeyValue {
  name: string;
  value: string;
}

export interface Assert {
  query: string;
  predicate: string;
  value: string;
}

export type BodyMode =
  | 'none'
  | 'json'
  | 'xml'
  | 'text'
  | 'graphql'
  | 'form'
  | 'multipart'
  | 'file'
  | 'hurl';

export interface Body {
  mode: BodyMode;
  content: string;
  fenced: boolean;
}

export interface RawSection {
  name: string;
  content: string;
}

/** One request (and its optional expected response) of a Hurl file. */
export interface Entry {
  index: number;
  startLine: number;
  endLine: number;
  docs: string;
  method: string;
  url: string;
  headers: KeyValue[];
  query: KeyValue[];
  form: KeyValue[];
  multipart: KeyValue[];
  cookies: KeyValue[];
  basicAuth: KeyValue | null;
  options: KeyValue[];
  requestExtra: RawSection[];
  body: Body;
  hasResponse: boolean;
  version: string;
  status: string;
  responseHeaders: KeyValue[];
  captures: KeyValue[];
  asserts: Assert[];
  responseExtra: RawSection[];
  responseBody: string;
}

export interface HurlFile {
  path: string;
  /** Changes when the file does (modification time and size). */
  stamp: string;
  raw: string;
  entries: Entry[];
  syntaxError: string | null;
}

export interface EntrySummary {
  index: number;
  method: string;
  url: string;
  name: string;
}

/** Options of `hurl` of a folder or of a Hurl file, from its options file, by long name. */
export type PathOptions = Record<string, boolean | string | string[]>;

export type TreeNode =
  | { type: 'folder'; name: string; path: string; options: PathOptions; children: TreeNode[] }
  | { type: 'file'; name: string; path: string; stamp: string; options: PathOptions; entries: EntrySummary[] };

export interface Variable {
  name: string;
  value: string;
  secret: boolean;
}

export interface Environment {
  name: string;
  path: string;
  variables: Variable[];
}

export interface Collection {
  path: string;
  name: string;
  /** Options of its options file. */
  options: PathOptions;
  children: TreeNode[];
  environments: Environment[];
}

export interface AppInfo {
  hurlPath: string | null;
  hurlVersion: string | null;
  hurlfmtPath: string | null;
  os: string;
  /** Version of Hurl UI, which is the version of Hurl its options come from. */
  version: string;
}

// --- Options of hurl ---------------------------------------------------------
//
// Nothing about the options of `hurl` is written in the frontend: they come
// from the catalog of the core (`hurl_options` command), generated from the
// option files of Hurl (`docs/spec/options/hurl`).

/** An option of `hurl` (see `packages/hurlui_core/src/options`). */
export interface HurlOption {
  /** Name of the option file. */
  name: string;
  /** `--long` on the command line, `long: value` in an `[Options]` section. */
  long: string;
  short: string | null;
  kind: 'flag' | 'choice' | 'integer' | 'text';
  /** Placeholder of the value, as shown by `hurl --help`. */
  value: string | null;
  default: string | null;
  choices: string[];
  min: number | null;
  /** Can be given several times. */
  multi: boolean;
  /** Option of the command line. */
  cli: boolean;
  /** Option of an `[Options]` section. */
  section: boolean;
  /**
   * Kind of its value in an `[Options]` section, from the type Hurl parses it
   * into: `duration` is an integer and an optional unit, `variable` is
   * `name=value`. Whatever the kind, the value can be a `{{placeholder}}`.
   */
  section_kind: 'boolean' | 'integer' | 'duration' | 'choice' | 'variable' | 'text' | null;
  /** Minimum of an `integer` value. */
  section_min: number | null;
  help: string | null;
  heading: string | null;
  /** Long names of the options which cannot be used with this one. */
  conflicts: string[];
  deprecated: boolean;
  experimental: boolean;
  /** Documentation of the man page, in Markdown. */
  description: string;
  /** Set by the application itself: cannot be set in the preferences. */
  managed: boolean;
}

export interface HurlOptions {
  /** Headings of `hurl --help`, in their order. */
  headings: string[];
  /** Units of a `duration` value, in an `[Options]` section. */
  duration_units: string[];
  options: HurlOption[];
}

/** Value of an option in the preferences: a flag, a value, or several values. */
export type OptionValue = boolean | string | string[];

/** Whether this option can be set in the preferences. */
export const isSettable = (option: HurlOption): boolean =>
  option.cli && !option.managed && !option.deprecated && !option.experimental;

/** Whether this value sets its option. */
export const isSet = (value: OptionValue | undefined): boolean =>
  Array.isArray(value) ? value.some((v) => v.trim() !== '') : typeof value === 'string' ? value.trim() !== '' : value === true;

// --- Run results (shape of the Hurl JSON report, augmented by the core) ------

export interface ResponseBody {
  size: number;
  truncated: boolean;
  /** Content-Encoding that could not be decoded for display. */
  encoded: boolean;
  text?: string;
  /** Images only: other binary bodies are described by their size. */
  base64?: string;
  /** The content was released by the GUI to bound memory. */
  dropped?: boolean;
  /**
   * The content was not sent by the core (no `text`, no `base64`): the
   * responses of the run exceeded its budget (100 MB).
   */
  omitted?: boolean;
}

export interface Call {
  request: { method: string; url: string; headers: KeyValue[] };
  response: {
    http_version: string;
    status: number;
    headers: KeyValue[];
    body?: ResponseBody | null;
  };
  timings: Record<string, number | string>;
}

export interface AssertResult {
  success: boolean;
  line: number;
  message?: string;
  source?: string;
}

export interface EntryResult {
  index: number;
  line: number;
  time: number;
  curl_cmd?: string;
  calls: Call[];
  captures: { name: string; value: unknown }[];
  asserts: AssertResult[];
  /** Runtime errors of the entry which are not assert failures. */
  errors: string[];
  /** A failed assert or a runtime error. */
  failed: boolean;
}

export interface RunResult {
  runId: string;
  file: string;
  command: string;
  exitCode: number | null;
  cancelled: boolean;
  durationMs: number;
  success: boolean;
  entries: EntryResult[];
  errors: string;
  stderr: string;
}

export type RunMeta = Omit<RunResult, 'entries'>;

/**
 * Display name of a request: the first non-empty line of its documentation.
 * Same rule as `Entry::name` in the Rust core, which names the sidebar rows.
 */
export function entryName(docs: string): string {
  return (
    docs
      .split('\n')
      .map((line) => line.trim())
      .find((line) => line !== '') ?? ''
  );
}

export function emptyEntry(): Entry {
  return {
    index: 0,
    startLine: 0,
    endLine: 0,
    docs: '',
    method: 'GET',
    url: '',
    headers: [],
    query: [],
    form: [],
    multipart: [],
    cookies: [],
    basicAuth: null,
    options: [],
    requestExtra: [],
    body: { mode: 'none', content: '', fenced: false },
    hasResponse: false,
    version: 'HTTP',
    status: '',
    responseHeaders: [],
    captures: [],
    asserts: [],
    responseExtra: [],
    responseBody: '',
  };
}

/** A value written with a placeholder (`{{name}}`), which Hurl accepts for any option of an `[Options]` section. */
export const isTemplate = (value: string): boolean => value.includes('{{');

/**
 * What is wrong with the value of an option in an `[Options]` section, or
 * null. The kinds of values are the ones Hurl parses (see `section_kind`).
 */
export function sectionValueError(option: HurlOption, value: string, catalog: HurlOptions): string | null {
  const v = value.trim();
  if (isTemplate(v)) return /^\{\{[^{}]+\}\}$/.test(v) || option.section_kind === 'text' ? null : 'Expecting a value or a {{variable}}';
  switch (option.section_kind) {
    case 'boolean':
      return v === 'true' || v === 'false' ? null : 'Expecting true or false';
    case 'integer': {
      const min = option.section_min ?? 0;
      return /^-?\d+$/.test(v) && Number(v) >= min ? null : `Expecting an integer, ${min} or more`;
    }
    case 'duration': {
      const units = catalog.duration_units;
      const m = /^(\d+)([a-z]*)$/.exec(v);
      return m && (m[2] === '' || units.includes(m[2]))
        ? null
        : `Expecting an integer and an optional unit (${units.join(', ')})`;
    }
    case 'choice':
      return option.choices.includes(v) ? null : `Expecting one of ${option.choices.join(', ')}`;
    case 'variable':
      return /^[A-Za-z_][\w-]*\s*=/.test(v) ? null : 'Expecting name=value';
    default:
      return null;
  }
}

/**
 * Value of an option of an `[Options]` section when the option is chosen:
 * the value already there if it suits the option, `true` for a boolean (an
 * option is written to be set), nothing otherwise.
 */
export function sectionValueFor(option: HurlOption, value: string, catalog: HurlOptions): string {
  if (sectionValueError(option, value, catalog) === null) return value;
  return option.section_kind === 'boolean' ? 'true' : '';
}
