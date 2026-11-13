// Transient UI state: dialogs, context menus and toasts.

import type { ReactNode } from 'react';
import { create } from 'zustand';

export interface Field {
  name: string;
  label: string;
  initial?: string;
  placeholder?: string;
  options?: string[];
  optional?: boolean;
}

export interface Dialog {
  title: string;
  message?: string;
  fields?: Field[];
  confirmLabel?: string;
  danger?: boolean;
  resolve: (values: Record<string, string> | null) => void;
}

export interface MenuItem {
  label?: string;
  icon?: ReactNode;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
}

export interface Toast {
  id: number;
  message: string;
  kind: 'info' | 'success' | 'error';
}

interface UiState {
  dialog: Dialog | null;
  menu: { x: number; y: number; items: MenuItem[] } | null;
  toasts: Toast[];
}

export const useUi = create<UiState>(() => ({ dialog: null, menu: null, toasts: [] }));

/** Opens a form dialog; resolves with the field values, or null when cancelled. */
export function askForm(options: Omit<Dialog, 'resolve'>): Promise<Record<string, string> | null> {
  return new Promise((resolve) => {
    useUi.setState({
      dialog: {
        ...options,
        resolve: (values) => {
          useUi.setState({ dialog: null });
          resolve(values);
        },
      },
    });
  });
}

export async function askText(
  title: string,
  label: string,
  initial = '',
  confirmLabel = 'OK',
): Promise<string | null> {
  const values = await askForm({ title, confirmLabel, fields: [{ name: 'value', label, initial }] });
  return values ? values.value.trim() : null;
}

export async function askConfirm(
  title: string,
  message: string,
  confirmLabel = 'OK',
  danger = false,
): Promise<boolean> {
  return (await askForm({ title, message, confirmLabel, danger })) !== null;
}

export function showMenu(at: { clientX: number; clientY: number }, items: MenuItem[]): void {
  useUi.setState({ menu: { x: at.clientX, y: at.clientY, items } });
}

export function closeMenu(): void {
  useUi.setState({ menu: null });
}

let toastId = 0;

export function toast(message: string, kind: Toast['kind'] = 'info'): void {
  // Several commands failing for the same reason (bridge without its token,
  // hurl not found...) say it once: the message is already on screen.
  if (useUi.getState().toasts.some((t) => t.message === message && t.kind === kind)) return;
  const id = ++toastId;
  useUi.setState((s) => ({ toasts: [...s.toasts, { id, message, kind }] }));
  setTimeout(
    () => useUi.setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
    kind === 'error' ? 7000 : 3000,
  );
}

export function toastError(e: unknown): void {
  toast(e instanceof Error ? e.message : String(e), 'error');
}
