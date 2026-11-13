// Path of what is under a position of a JSON or XML document shown in an
// editor: a JSONPath or an XPath, as Hurl evaluates them (`jsonpath` and
// `xpath` queries), like "Copy XPath" of the developer tools of a browser.
//
// The paths are read from the syntax tree of the editor (the one of its
// highlighting), so they follow the text as it is shown, pretty-printed or
// not.

import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import type { EditorState } from '@codemirror/state';

type Tree = ReturnType<typeof syntaxTree>;
type SyntaxNode = Tree['topNode'];

/** Time given to the parser to reach the end of a large document, in milliseconds. */
const PARSE_TIMEOUT = 500;

/**
 * Position of the text of a line nearest to `pos`. A click in the margin of
 * a line (its indentation, or after its end) is about what the line holds,
 * not about what encloses it.
 */
function snap(state: EditorState, pos: number): number {
  const line = state.doc.lineAt(pos);
  const start = line.from + (line.text.length - line.text.trimStart().length);
  const end = line.from + line.text.trimEnd().length;
  if (start >= end) return pos;
  if (pos < start) return start;
  if (pos < end) return pos;
  // After the end of the line: its last character, the separator of JSON items aside.
  const last = end - 1;
  return last > start && state.sliceDoc(last, end) === ',' ? last - 1 : last;
}

// --- JSON ---------------------------------------------------------------------------

const JSON_VALUES = new Set(['True', 'False', 'Null', 'Number', 'String', 'Object', 'Array']);

/** A member name which can be written `.name`; the others are written `['name']`. */
const SHORTHAND = /^[A-Za-z_][A-Za-z0-9_]*$/;

function jsonSegment(key: string): string {
  if (SHORTHAND.test(key)) return `.${key}`;
  return `['${key.replace(/\\/g, '\\\\').replace(/'/g, "\\'")}']`;
}

/**
 * JSONPath of the value at `pos` (or of the member, on its name): `$` for
 * the document itself, null when there is no JSON there.
 */
export function jsonPathAt(state: EditorState, at: number): string | null {
  const pos = snap(state, at);
  // What precedes the position is enough: the index of an array item only
  // depends on the items before it.
  const tree = ensureSyntaxTree(state, Math.min(pos + 1, state.doc.length), PARSE_TIMEOUT) ?? syntaxTree(state);
  const segments: string[] = [];
  let found = false;
  for (let node: SyntaxNode | null = tree.resolveInner(pos, 1); node; node = node.parent) {
    if (node.name === 'JsonText') found = true;
    if (node.name === 'Property') {
      const name = node.getChild('PropertyName');
      if (!name) return null;
      try {
        segments.unshift(jsonSegment(JSON.parse(state.sliceDoc(name.from, name.to)) as string));
      } catch {
        return null;
      }
    } else if (node.parent?.name === 'Array' && JSON_VALUES.has(node.name)) {
      let index = 0;
      for (let prev = node.prevSibling; prev; prev = prev.prevSibling) if (JSON_VALUES.has(prev.name)) index++;
      segments.unshift(`[${index}]`);
    }
  }
  return found ? `$${segments.join('')}` : null;
}

// --- XML ----------------------------------------------------------------------------

const text = (state: EditorState, node: SyntaxNode | null): string => (node ? state.sliceDoc(node.from, node.to) : '');

/** Opening tag of an element, which holds its name and its attributes. */
const tagOf = (element: SyntaxNode): SyntaxNode | null => element.getChild('OpenTag') ?? element.getChild('SelfClosingTag');

const nameOf = (state: EditorState, element: SyntaxNode): string => text(state, tagOf(element)?.getChild('TagName') ?? null);

/** Value of an attribute of an element, undefined when it has no such attribute. */
function attribute(state: EditorState, element: SyntaxNode, name: string): string | undefined {
  for (const attr of tagOf(element)?.getChildren('Attribute') ?? []) {
    if (text(state, attr.getChild('AttributeName')) === name) {
      return text(state, attr.getChild('AttributeValue')).replace(/^["']|["']$/g, '');
    }
  }
  return undefined;
}

/** Default namespace an element is in (`xmlns="..."` on it or on the nearest ancestor), '' for none. */
function defaultNamespace(state: EditorState, element: SyntaxNode): string {
  for (let node: SyntaxNode | null = element; node; node = node.parent) {
    if (node.name !== 'Element') continue;
    const ns = attribute(state, node, 'xmlns');
    if (ns !== undefined) return ns;
  }
  return '';
}

/** First default namespace declared in the document, in document order: the one Hurl registers as `_`. */
function firstDefaultNamespace(state: EditorState, tree: Tree): string | undefined {
  let found: string | undefined;
  tree.iterate({
    enter(node) {
      if (found !== undefined) return false;
      if (node.name === 'AttributeName' && state.sliceDoc(node.from, node.to) === 'xmlns') {
        const value = node.node.parent?.getChild('AttributeValue') ?? null;
        found = text(state, value).replace(/^["']|["']$/g, '');
      }
      return undefined;
    },
  });
  return found;
}

/**
 * Name test of an element in an XPath evaluated by Hurl. A name without
 * prefix only matches an element which is in no namespace. Hurl registers
 * the prefixes of the document, and its first default namespace as `_`: an
 * element of another default namespace can only be matched by its local name.
 */
function nameTest(state: EditorState, element: SyntaxNode, first: () => string | undefined): string {
  const name = nameOf(state, element);
  if (name.includes(':')) return name;
  const ns = defaultNamespace(state, element);
  if (ns === '') return name;
  return ns === first() ? `_:${name}` : `*[local-name()='${name}']`;
}

/**
 * XPath of the element at `pos`, or of the attribute when `pos` is on one:
 * an absolute path from the root, with a position (`item[2]`) where several
 * siblings have the same name. Null when there is no element there.
 */
export function xpathAt(state: EditorState, at: number): string | null {
  const pos = snap(state, at);
  // Whether an element is the only one of its name depends on the siblings
  // which follow it: the whole document is needed to leave positions out.
  const whole = ensureSyntaxTree(state, state.doc.length, PARSE_TIMEOUT);
  const tree = whole ?? ensureSyntaxTree(state, Math.min(pos + 1, state.doc.length), PARSE_TIMEOUT) ?? syntaxTree(state);
  let first: { value: string | undefined } | undefined;
  const firstNamespace = () => (first ??= { value: firstDefaultNamespace(state, tree) }).value;

  let attr: string | null = null;
  const steps: string[] = [];
  for (let node: SyntaxNode | null = tree.resolveInner(pos, 1); node; node = node.parent) {
    if (node.name === 'Attribute' && steps.length === 0) {
      const name = text(state, node.getChild('AttributeName'));
      // A namespace declaration is not an attribute for XPath.
      if (name !== 'xmlns' && !name.startsWith('xmlns:')) attr = name;
    }
    if (node.name !== 'Element') continue;
    const name = nameOf(state, node);
    if (name === '') return null;
    const siblings = (node.parent?.getChildren('Element') ?? []).filter((e) => nameOf(state, e) === name);
    const position = siblings.findIndex((e) => e.from === node.from) + 1;
    const step = nameTest(state, node, firstNamespace);
    // Without the whole document, the position is always written: it is right either way.
    steps.unshift(siblings.length > 1 || !whole ? `${step}[${position}]` : step);
  }
  if (steps.length === 0) return null;
  return `/${steps.join('/')}${attr ? `/@${attr}` : ''}`;
}

// --- Hurl queries -------------------------------------------------------------------

/** A string of a Hurl file. */
const quoted = (value: string): string => `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;

/** Hurl query evaluating a JSONPath, for an assert or a capture. */
export const jsonPathQuery = (path: string): string => `jsonpath ${quoted(path)}`;

/**
 * Hurl query evaluating an XPath, for an assert or a capture: the text of
 * the element or the value of the attribute (the path alone is a set of
 * nodes, which Hurl can only count).
 */
export const xpathQuery = (path: string): string => `xpath ${quoted(`string(${path})`)}`;
