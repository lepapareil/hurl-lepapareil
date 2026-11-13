// Window chrome: title bar, request tabs, environment selector, status bar, console.

import {
  IconFolder,
  IconCheck,
  IconDatabase,
  IconFileText,
  IconLayoutBottombar,
  IconLayoutColumns,
  IconLayoutRows,
  IconLayoutSidebar,
  IconMoon,
  IconPlayerPlay,
  IconSettings,
  IconSun,
  IconTerminal2,
  IconTrash,
  IconX,
} from '@tabler/icons-react';
import { memo, type MouseEvent } from 'react';
import hurlLogo from '../../../../../art/logo-icon.svg';
import { baseName, entryOfTab, isDirty, useStore, type Tab } from '../store';
import { entryName } from '../types';
import { showMenu, type MenuItem } from '../ui';
import { displayUrl, methodClass } from './Sidebar';
import { Dropdown } from './ui';

/** The logo of Hurl UI is the logo of Hurl: the file of the repository (`art/`), not a copy. */
export function Logo({ size = 18 }: { size?: number }) {
  return <img src={hurlLogo} width={size} height={size} alt="" aria-hidden="true" />;
}

// The title bar and the status bar read the store through their own
// selectors: memoized, they do not follow the renders of the window.
export const TitleBar = memo(function TitleBar() {
  const prefs = useStore((s) => s.prefs);
  const setPrefs = useStore((s) => s.setPrefs);
  const setLayout = useStore((s) => s.setLayout);
  const sidebarCollapsed = useStore((s) => s.sidebarCollapsed);
  const consoleOpen = useStore((s) => s.consoleOpen);
  const horizontal = prefs.layout === 'horizontal';
  return (
    <header className="titlebar">
      <div className="titlebar-brand">
        <Logo />
        <span>Hurl UI</span>
      </div>
      <div className="titlebar-actions">
        <button
          type="button"
          className={`icon-btn ${sidebarCollapsed ? '' : 'on'}`}
          title="Toggle sidebar (Ctrl+\)"
          onClick={() => setLayout({ sidebarCollapsed: !sidebarCollapsed })}
        >
          <IconLayoutSidebar size={16} stroke={1.5} />
        </button>
        <button
          type="button"
          className={`icon-btn ${consoleOpen ? 'on' : ''}`}
          title="Toggle console"
          onClick={() => setLayout({ consoleOpen: !consoleOpen })}
        >
          <IconLayoutBottombar size={16} stroke={1.5} />
        </button>
        <button
          type="button"
          className="icon-btn"
          title="Change the request/response orientation (Ctrl+J)"
          onClick={() => setPrefs({ layout: horizontal ? 'vertical' : 'horizontal' })}
        >
          {horizontal ? <IconLayoutColumns size={16} stroke={1.5} /> : <IconLayoutRows size={16} stroke={1.5} />}
        </button>
      </div>
    </header>
  );
});

function EnvironmentSelector({ collection }: { collection: string }) {
  const environments = useStore((s) => s.collections.find((c) => c.path === collection)?.environments);
  const active = useStore((s) => s.activeEnv[collection]);
  const setActiveEnv = useStore((s) => s.setActiveEnv);
  const openEnvironments = useStore((s) => s.openEnvironments);
  const check = <IconCheck size={14} stroke={1.5} />;
  const blank = <span style={{ width: 14, display: 'inline-block' }} />;
  const items: MenuItem[] = [
    { label: 'No Environment', icon: active ? blank : check, onClick: () => setActiveEnv(collection, null) },
    ...(environments ?? []).map((e) => ({
      label: e.name,
      icon: e.name === active ? check : blank,
      onClick: () => setActiveEnv(collection, e.name),
    })),
    { separator: true },
    { label: 'Configure', icon: <IconSettings size={14} stroke={1.5} />, onClick: () => openEnvironments(collection) },
  ];
  return (
    <Dropdown className={`env-selector ${active ? 'selected' : ''}`} items={items} align="right" title="Environment">
      <IconDatabase size={14} stroke={1.5} />
      <span>{active ?? 'No Environment'}</span>
    </Dropdown>
  );
}

function TabLabel({ tab }: { tab: Tab }) {
  const entry = useStore((s) => (tab.kind === 'entry' ? entryOfTab(s, tab) : undefined));
  switch (tab.kind) {
    case 'entry':
      return (
        <>
          <span className={methodClass(entry?.method ?? 'GET')}>{entry?.method ?? '?'}</span>
          <span className="tab-name">
            {entry ? entryName(entry.docs) || displayUrl(entry.url) || 'Untitled' : baseName(tab.path)}
          </span>
        </>
      );
    case 'file':
      return (
        <>
          <IconFileText size={14} stroke={1.5} />
          <span className="tab-name">{baseName(tab.path)}</span>
        </>
      );
    case 'runner':
      return (
        <>
          <IconPlayerPlay size={14} stroke={1.5} />
          <span className="tab-name">Runner · {baseName(tab.path).replace(/\.hurl$/i, '')}</span>
        </>
      );
    case 'environments':
      return (
        <>
          <IconDatabase size={14} stroke={1.5} />
          <span className="tab-name">Environments</span>
        </>
      );
    default:
      return (
        <>
          <IconSettings size={14} stroke={1.5} />
          <span className="tab-name">Preferences</span>
        </>
      );
  }
}

export function TabsBar() {
  const tabs = useStore((s) => s.tabs);
  const activeTabId = useStore((s) => s.activeTabId);
  const collections = useStore((s) => s.collections);
  const envDrafts = useStore((s) => s.envDrafts);
  const { setActiveTab, closeTab, closeTabs, revertTab, openRunner } = useStore.getState();
  const dirty = (tab: Tab) => isDirty(tab, envDrafts);
  const active = tabs.find((t) => t.id === activeTabId);
  const collection = collections.find((c) => c.path === active?.collection);

  const menu = (e: MouseEvent, tab: Tab) => {
    e.preventDefault();
    const position = tabs.indexOf(tab);
    showMenu(e, [
      { label: 'Close', onClick: () => closeTab(tab.id) },
      { label: 'Close Others', onClick: () => closeTabs((t) => t.id === tab.id) },
      { label: 'Close to the Right', onClick: () => closeTabs((t) => tabs.indexOf(t) <= position) },
      { label: 'Close Saved', onClick: () => closeTabs(dirty) },
      { label: 'Close All', onClick: () => closeTabs(() => false) },
      { separator: true },
      { label: 'Revert Changes', disabled: !dirty(tab), onClick: () => revertTab(tab.id) },
    ]);
  };

  return (
    <div className="tabs-area">
      <div className="collection-header">
        <div className="collection-title">
          <IconFolder size={16} stroke={1.5} />
          <span>{collection?.name ?? 'Hurl UI'}</span>
        </div>
        {collection && (
          <div className="collection-actions">
            <button
              type="button"
              className="icon-btn"
              title="Runner"
              onClick={() => openRunner(collection.path, collection.path)}
            >
              <IconPlayerPlay size={16} stroke={1.5} />
            </button>
            <EnvironmentSelector collection={collection.path} />
          </div>
        )}
      </div>
      <div className="request-tabs" role="tablist">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            role="tab"
            aria-selected={tab.id === activeTabId}
            className={`request-tab ${tab.id === activeTabId ? 'active' : ''} ${dirty(tab) ? 'dirty' : ''}`}
            onClick={() => setActiveTab(tab.id)}
            onAuxClick={(e) => e.button === 1 && closeTab(tab.id)}
            onContextMenu={(e) => menu(e, tab)}
            title={tab.path || undefined}
          >
            <TabLabel tab={tab} />
            <button
              type="button"
              className="tab-close"
              aria-label="Close tab"
              onClick={(e) => {
                e.stopPropagation();
                void closeTab(tab.id);
              }}
            >
              <IconX size={13} stroke={1.5} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

export const StatusBar = memo(function StatusBar() {
  const appInfo = useStore((s) => s.appInfo);
  const prefs = useStore((s) => s.prefs);
  const consoleOpen = useStore((s) => s.consoleOpen);
  const failures = useStore((s) => s.logs.filter((l) => !l.success).length);
  const { setPrefs, setLayout, openPreferences } = useStore.getState();
  const dark = document.documentElement.dataset.theme === 'dark';
  return (
    <footer className="statusbar">
      <button type="button" className="status-btn" title="Preferences (Ctrl+,)" onClick={openPreferences}>
        <IconSettings size={15} stroke={1.5} />
      </button>
      <button
        type="button"
        className="status-btn"
        title={`Theme: ${prefs.theme}`}
        onClick={() => setPrefs({ theme: dark ? 'light' : 'dark' })}
      >
        {dark ? <IconSun size={15} stroke={1.5} /> : <IconMoon size={15} stroke={1.5} />}
      </button>
      <div className="statusbar-spacer" />
      <button
        type="button"
        className={`status-btn ${consoleOpen ? 'on' : ''}`}
        onClick={() => setLayout({ consoleOpen: !consoleOpen })}
      >
        <IconTerminal2 size={15} stroke={1.5} />
        Console
        {failures > 0 && <span className="status-badge">{failures}</span>}
      </button>
      {appInfo &&
        (appInfo.hurlVersion ? (
          <button type="button" className="status-btn" title={appInfo.hurlPath ?? ''} onClick={openPreferences}>
            {appInfo.hurlVersion.split(' ').slice(0, 2).join(' ')}
          </button>
        ) : (
          <button type="button" className="status-btn status-error" onClick={openPreferences}>
            hurl not found
          </button>
        ))}
    </footer>
  );
});

export function Console() {
  const logs = useStore((s) => s.logs);
  const { clearLogs, setLayout } = useStore.getState();
  return (
    <div className="console">
      <div className="console-header">
        <span className="console-title">Console</span>
        <span className="muted">hurl executions</span>
        <div className="statusbar-spacer" />
        <button type="button" className="icon-btn" title="Clear" onClick={clearLogs}>
          <IconTrash size={15} stroke={1.5} />
        </button>
        <button type="button" className="icon-btn" title="Close" onClick={() => setLayout({ consoleOpen: false })}>
          <IconX size={15} stroke={1.5} />
        </button>
      </div>
      <div className="console-body">
        {logs.length === 0 && <div className="muted console-empty">No execution yet.</div>}
        {[...logs].reverse().map((log) => (
          <details key={log.id} className="console-item">
            <summary>
              <span className={`run-dot ${log.success ? 'passed' : 'failed'}`} />
              <span className="muted">{new Date(log.time).toLocaleTimeString()}</span>
              <code>{log.command}</code>
              <span className="muted">
                {log.cancelled ? 'cancelled' : `exit ${log.exitCode ?? '?'}`} · {log.durationMs} ms
              </span>
            </summary>
            <pre>{log.output || '(no output)'}</pre>
          </details>
        ))}
      </div>
    </div>
  );
}
