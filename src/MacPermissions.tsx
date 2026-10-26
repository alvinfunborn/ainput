import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { PermissionStatus } from './usePermissions';

export default function MacPermissions({ status, error, refresh }: { status: PermissionStatus | null; error: string; refresh: () => Promise<void> }) {
  const [actionError, setActionError] = useState('');
  const [pending, setPending] = useState(false);
  const open = async (permission: string) => {
    setActionError(''); setPending(true);
    try { await invoke('open_permission_settings', { permission }); await refresh(); }
    catch (reason) { setActionError(String(reason)); }
    finally { setPending(false); }
  };
  if (status && !status.required) return null;
  return <section className={`settings-card permission-card ${status && !status.ready ? 'needs-permission' : ''}`}>
    <div className="card-heading"><div><h2>macOS 权限</h2><p>允许 AInput 读取当前输入，并插入你接受的补全。</p></div><span className={`permission-state ${status?.ready ? 'ready' : ''}`}>{status ? status.ready ? '已就绪' : '需要设置' : '正在检查'}</span></div>
    {status && !status.ready && <div className="permission-intro" role="status"><strong>{status.accessibility ? '辅助功能已允许，正在等待键盘连接' : '先开启辅助功能，开始使用补全'}</strong><p>{status.accessibility ? '若连接迟迟未就绪，请重启 AInput。仍有问题时，展开下方说明检查输入监控。' : '目前无法读取输入或接受补全。打开系统设置，开启 AInput 旁的开关，再返回这里；授权状态会自动更新。'}</p></div>}
    <div className="permission-row"><div><strong>辅助功能</strong><p>系统设置 → 隐私与安全性 → 辅助功能</p></div><span className={`permission-state ${status?.accessibility ? 'ready' : ''}`}>{status ? status.accessibility ? '已允许' : '未授权' : '正在检查'}</span></div>
    <div className="permission-row"><div><strong>键盘连接</strong><p>用于接受和收起候选内容。</p></div><span className={`permission-state ${status?.keyboardHook ? 'ready' : ''}`}>{status ? status.keyboardHook ? '已连接' : '等待连接' : '正在检查'}</span></div>
    {status && !status.ready && <>
      <div className="permission-actions">{!status.accessibility ? <button className="settings-button primary" disabled={pending} onClick={() => void open('accessibility')}>打开辅助功能设置<small>↗</small></button> : <button className="settings-button primary" onClick={() => { void invoke('restart_application').catch(reason => setActionError(String(reason))); }}>重启 AInput</button>}<button className="settings-button secondary" onClick={() => void refresh()}>重新检测</button></div>
      {!status.keyboardHook && <details className="advanced-settings"><summary>授权后仍无法连接？</summary><p className="inline-note">更新应用后，可在辅助功能中移除旧条目，重新添加当前的 AInput 并重启。如果键盘仍未连接，再检查输入监控权限。</p><div className="permission-actions"><button className="settings-button secondary" disabled={pending} onClick={() => void open('inputMonitoring')}>打开输入监控设置</button><button className="settings-button secondary" onClick={() => { void invoke('restart_application').catch(reason => setActionError(String(reason))); }}>重启 AInput</button></div><p className="field-hint app-path">当前应用：{status.appPath}</p></details>}
    </>}
    {(error || actionError) && <p className="permission-error" role="alert">{error || actionError}</p>}
  </section>;
}
