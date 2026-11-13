// CodeMirror 6 editor with JSON, XML and Hurl highlighting.

import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { json } from '@codemirror/lang-json';
import { xml } from '@codemirror/lang-xml';
import {
  bracketMatching,
  HighlightStyle,
  indentOnInput,
  StreamLanguage,
  syntaxHighlighting,
} from '@codemirror/language';
import { Compartment, EditorState, type Extension } from '@codemirror/state';
import { EditorView, keymap, lineNumbers, placeholder as placeholderExt } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { useEffect, useRef } from 'react';

export type EditorLanguage = 'json' | 'xml' | 'hurl' | 'text';

const PREDICATES =
  /^(not|contains|startsWith|endsWith|matches|exists|includes|isEmpty|is[A-Z]\w*)\b/;
const QUERIES =
  /^(jsonpath|xpath|header|status|body|bytes|rawbytes|sha256|md5|duration|regex|variable|cookie|url|certificate|version|ip|redirects|true|false|null)\b/;

/** Line oriented highlighter for Hurl files. */
const hurlLanguage = StreamLanguage.define<{ fence: boolean }>({
  name: 'hurl',
  startState: () => ({ fence: false }),
  token(stream, state) {
    if (state.fence) {
      if (stream.match(/^.*?```/)) state.fence = false;
      else stream.skipToEnd();
      return 'string';
    }
    if (stream.sol()) {
      if (stream.match(/^\s*#.*/)) return 'comment';
      if (stream.match(/^\s*\[[A-Z][A-Za-z]*\]/)) return 'heading';
      if (stream.match(/^\s*HTTP(\/[\d.*]+)?(?=\s)/)) return 'keyword';
      if (stream.match(/^\s*[A-Z]+(?=\s)/)) return 'keyword';
      if (stream.match(/^\s*[\w\-.[\]@$]+(?=\s*:)/)) return 'propertyName';
    }
    if (stream.match('```')) {
      state.fence = true;
      return 'string';
    }
    if (stream.match(/^\{\{[^}]*\}\}/)) return 'variableName';
    if (stream.match(/^"(?:[^"\\]|\\.)*"?/)) return 'string';
    if (stream.match(/^`[^`]*`?/)) return 'string';
    if (/\s/.test(stream.string.charAt(stream.pos - 1)) && stream.match(/^#.*/)) return 'comment';
    if (stream.match(/^-?\d+(\.\d+)?\b/)) return 'number';
    if (stream.match(/^(==|!=|>=|<=|>|<)/)) return 'operator';
    if (stream.match(PREDICATES)) return 'operator';
    if (stream.match(QUERIES)) return 'atom';
    stream.next();
    return null;
  },
});

// Colours come from CSS variables so that the editor follows the theme.
const highlight = HighlightStyle.define([
  { tag: [t.propertyName, t.attributeName], color: 'var(--cm-property)' },
  { tag: [t.string, t.special(t.string)], color: 'var(--cm-string)' },
  { tag: [t.number, t.bool, t.null], color: 'var(--cm-number)' },
  { tag: [t.keyword, t.tagName], color: 'var(--cm-keyword)', fontWeight: '600' },
  { tag: [t.comment], color: 'var(--cm-comment)', fontStyle: 'italic' },
  { tag: [t.variableName], color: 'var(--cm-variable)' },
  { tag: [t.operator], color: 'var(--cm-operator)' },
  { tag: [t.atom], color: 'var(--cm-atom)' },
  { tag: [t.heading], color: 'var(--cm-heading)', fontWeight: '600' },
]);

const theme = EditorView.theme({
  '&': { height: '100%', color: 'var(--text)', backgroundColor: 'transparent', fontSize: '12.5px' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.55' },
  '.cm-content': { caretColor: 'var(--text)' },
  '.cm-cursor': { borderLeftColor: 'var(--text)' },
  '.cm-gutters': { backgroundColor: 'transparent', color: 'var(--text-muted)', border: 'none' },
  '.cm-activeLineGutter': { backgroundColor: 'transparent' },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': {
    backgroundColor: 'var(--selection)',
  },
  '.cm-placeholder': { color: 'var(--text-muted)' },
});

function languageExtension(language: EditorLanguage): Extension {
  switch (language) {
    case 'json':
      return json();
    case 'xml':
      return xml();
    case 'hurl':
      return hurlLanguage;
    default:
      return [];
  }
}

export function CodeEditor(props: {
  value: string;
  onChange?: (value: string) => void;
  language?: EditorLanguage;
  readOnly?: boolean;
  placeholder?: string;
  /**
   * Right click in the text, with the position in the document under the
   * pointer. Returns whether it was handled: the menu of the system is shown
   * otherwise.
   */
  onContextMenu?: (event: MouseEvent, state: EditorState, pos: number) => boolean;
}) {
  const { value, language = 'text', readOnly = false } = props;
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const onChange = useRef(props.onChange);
  onChange.current = props.onChange;
  const onContextMenu = useRef(props.onContextMenu);
  onContextMenu.current = props.onContextMenu;
  // True while the document is replaced from the props: not a user edit.
  const syncing = useRef(false);
  const languageSlot = useRef(new Compartment());
  const readOnlySlot = useRef(new Compartment());

  useEffect(() => {
    const editor = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          indentOnInput(),
          bracketMatching(),
          syntaxHighlighting(highlight),
          // Ctrl/Cmd+Enter sends the request (window shortcut). The default
          // keymap binds it to "insert blank line", which would alter the
          // body right before it is saved and sent.
          keymap.of([
            ...defaultKeymap.filter((binding) => binding.key !== 'Mod-Enter'),
            ...historyKeymap,
            indentWithTab,
          ]),
          theme,
          EditorView.lineWrapping,
          props.placeholder ? placeholderExt(props.placeholder) : [],
          languageSlot.current.of(languageExtension(language)),
          readOnlySlot.current.of([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
          EditorView.domEventHandlers({
            contextmenu(event, view) {
              const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
              if (pos === null || !onContextMenu.current?.(event, view.state, pos)) return false;
              event.preventDefault();
              return true;
            },
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !syncing.current) onChange.current?.(update.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = editor;
    return () => editor.destroy();
    // The editor is created once; later changes go through the effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const editor = view.current;
    if (!editor || editor.state.doc.toString() === value) return;
    syncing.current = true;
    editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: value } });
    syncing.current = false;
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({ effects: languageSlot.current.reconfigure(languageExtension(language)) });
  }, [language]);

  useEffect(() => {
    view.current?.dispatch({
      effects: readOnlySlot.current.reconfigure([
        EditorState.readOnly.of(readOnly),
        EditorView.editable.of(!readOnly),
      ]),
    });
  }, [readOnly]);

  return <div className="code-editor" ref={host} />;
}
