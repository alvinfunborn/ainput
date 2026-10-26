import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface PermissionStatus {
  required: boolean;
  accessibility: boolean;
  inputMonitoring: boolean;
  keyboardHook: boolean;
  ready: boolean;
  appPath: string;
}

// Keep permission checks alive across settings tabs and refresh on returning
// from System Settings. A working keyboard tap is confirmed by the backend.
export function usePermissions() {
  const [status, setStatus] = useState<PermissionStatus | null>(null);
  const [error, setError] = useState('');
  const active = useRef(false);
  const pending = useRef(false);
  const refresh = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    try {
      const value = await invoke<PermissionStatus>('get_permission_status');
      if (active.current) { setStatus(value); setError(''); }
    } catch (reason) {
      if (active.current) setError(String(reason));
    } finally { pending.current = false; }
  }, []);
  useEffect(() => {
    active.current = true;
    void refresh();
    const timer = window.setInterval(() => { if (!document.hidden) void refresh(); }, 2000);
    window.addEventListener('focus', refresh);
    return () => {
      active.current = false;
      window.clearInterval(timer);
      window.removeEventListener('focus', refresh);
    };
  }, [refresh]);
  return { status, error, refresh };
}
