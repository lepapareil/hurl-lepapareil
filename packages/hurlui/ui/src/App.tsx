import { memo, useEffect } from 'react';
import { Console, StatusBar, TabsBar, TitleBar } from './components/Chrome';
import { EntryPanel } from './components/EntryPanel';
import { EnvironmentsPanel, FilePanel, PreferencesPanel, RunnerPanel, Welcome } from './components/Panels';
import { Sidebar } from './components/Sidebar';
import { DialogHost, MenuHost, ToastHost } from './components/ui';
import { useStore } from './store';

// Suggestions offered by the request tables (free text stays allowed: the
// installed hurl decides what is valid).
const QUERIES = [
  'status', 'version', 'url', 'body', 'bytes', 'sha256', 'md5', 'duration', 'ip', 'redirects',
  'jsonpath "$."', 'xpath "//"', 'header "Content-Type"', 'cookie "name"', 'regex "(.*)"',
  'variable "name"', 'certificate "Subject"',
];
const HEADERS = [
  'Accept', 'Accept-Encoding', 'Accept-Language', 'Authorization', 'Cache-Control', 'Content-Type', 'Cookie',
  'If-Match', 'If-None-Match', 'Origin', 'Referer', 'User-Agent', 'X-Api-Key', 'X-Requested-With',
];

// Memoized, like the title bar, the sidebar and the status bar (where they are
// defined): none of them takes props, so a render of the window skips them.
const Datalists = memo(function Datalists() {
  // Options of an `[Options]` section: from the catalog of options shared
  // with Hurl, not from a list kept here.
  const catalog = useStore((s) => s.hurlOptions);
  // Operators of an assert: all the predicates of Hurl (the core has them
  // from `hurl_core`), then each of them negated.
  const predicates = useStore((s) => s.hurlPredicates);
  const lists: [string, string[]][] = [
    ['hurl-predicates', [...predicates, ...predicates.map((p) => `not ${p}`)]],
    ['hurl-queries', QUERIES],
    ['http-headers', HEADERS],
  ];
  return (
    <>
      <datalist id="hurl-options">
        {catalog?.options
          .filter((o) => o.section)
          .map((o) => <option key={o.long} value={o.long} label={o.help ?? undefined} />)}
      </datalist>
      {lists.map(([id, values]) => (
        <datalist key={id} id={id}>
          {values.map((v) => (
            <option key={v} value={v} />
          ))}
        </datalist>
      ))}
    </>
  );
});

/** Subscribes to its own tab, so that typing in it does not re-render the rest of the window. */
function TabPanel({ id }: { id: string }) {
  const tab = useStore((s) => s.tabs.find((t) => t.id === id));
  if (!tab) return null;
  switch (tab.kind) {
    case 'entry':
      return <EntryPanel tab={tab} />;
    case 'file':
      return <FilePanel tab={tab} />;
    case 'runner':
      return <RunnerPanel tab={tab} />;
    case 'environments':
      return <EnvironmentsPanel tab={tab} />;
    default:
      return <PreferencesPanel />;
  }
}

function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const s = useStore.getState();
      const tab = s.tabs.find((t) => t.id === s.activeTabId);
      const handlers: Record<string, () => void> = {
        Enter: () => {
          if (tab?.kind === 'entry') void s.sendEntry(tab.id);
          else if (tab?.kind === 'file') void s.runFile(tab.collection, tab.path);
          else if (tab?.kind === 'runner') void s.startRunner(tab.id);
        },
        s: () => tab && void s.saveTab(tab.id),
        w: () => tab && void s.closeTab(tab.id),
        '\\': () => s.setLayout({ sidebarCollapsed: !s.sidebarCollapsed }),
        j: () => s.setPrefs({ layout: s.prefs.layout === 'horizontal' ? 'vertical' : 'horizontal' }),
        ',': () => s.openPreferences(),
      };
      const handler = handlers[e.key.length === 1 ? e.key.toLowerCase() : e.key];
      if (handler && !e.shiftKey) {
        e.preventDefault();
        handler();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

export function App() {
  const ready = useStore((s) => s.ready);
  const sidebarCollapsed = useStore((s) => s.sidebarCollapsed);
  const consoleOpen = useStore((s) => s.consoleOpen);
  // Only the identity of the active tab: its content changes at every keystroke.
  const activeTabId = useStore((s) => (s.tabs.some((t) => t.id === s.activeTabId) ? s.activeTabId : null));
  useShortcuts();

  return (
    <div className="app">
      <TitleBar />
      <div className="main">
        {!sidebarCollapsed && <Sidebar />}
        <section className="workspace">
          {activeTabId && <TabsBar />}
          <div className="tab-panel">
            {/* Keyed by tab: each tab owns its editors and their undo history. */}
            {activeTabId ? <TabPanel key={activeTabId} id={activeTabId} /> : ready ? <Welcome /> : null}
          </div>
        </section>
      </div>
      {consoleOpen && <Console />}
      <StatusBar />
      <Datalists />
      <DialogHost />
      <MenuHost />
      <ToastHost />
    </div>
  );
}
