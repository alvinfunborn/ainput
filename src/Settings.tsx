import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ApiOutlined, CheckOutlined, EditOutlined, SafetyCertificateOutlined, ThunderboltOutlined, ArrowRightOutlined, LoadingOutlined } from '@ant-design/icons';
import MacPermissions from './MacPermissions';
import { usePermissions } from './usePermissions';
import brandIcon from '../src-tauri/icons/128x128.png';
import './Settings.css';

interface AiConfig {
  provider: 'API' | 'CMD'; api_url: string; api_key: string; api_model: string;
  api_proxy: string; cmd: string; prompt: string;
}
interface Config {
  system: { completion_enabled: boolean; [key: string]: unknown };
  ai_client: AiConfig;
  keybinding: Record<string, string[]>;
  privacy: { enable: boolean; rules: string[] };
  ui_automation: { ignore_apps: string[]; [key: string]: unknown };
  [key: string]: unknown;
}
type Page = 'connection' | 'input' | 'prompt' | 'privacy';
type Result = { kind: 'success'; first: number; total: number } | { kind: 'error'; title: string; help: string; detail: string };
const GEMINI_URL = 'https://generativelanguage.googleapis.com/v1beta/openai/chat/completions';
const GEMINI_MODEL = 'gemini-3.5-flash-lite';
const pages = [
  { id: 'connection' as const, label: '模型连接', icon: ApiOutlined, description: '选择补全模型，让每一次输入都更流畅。' },
  { id: 'input' as const, label: '输入与快捷键', icon: ThunderboltOutlined, description: '了解补全操作，并选择需要暂停补全的应用。' },
  { id: 'prompt' as const, label: '提示词', icon: EditOutlined, description: '告诉模型如何接续你的表达。' },
  { id: 'privacy' as const, label: '权限与隐私', icon: SafetyCertificateOutlined, description: '管理系统权限，了解发送给模型的上下文。' },
];

function explainError(raw: unknown): Extract<Result, { kind: 'error' }> {
  const detail = String(raw);
  const value = detail.toLowerCase();
  if (value.includes('location is not supported') || value.includes('country')) return { kind: 'error', title: '当前网络出口所在地区不受支持', help: 'Google 已收到请求，但拒绝了当前出口地区。切换系统代理节点后，重新测试连接。此错误不能用来判断 Key 是否有效。', detail };
  if (value.includes('api_key_invalid') || value.includes('api key not valid') || value.includes('401') || value.includes('expired') || value.includes('leaked')) return { kind: 'error', title: 'API Key 未通过验证', help: '检查 Key 是否完整、仍然有效，以及是否被服务商停用。更新后重新保存并测试。', detail };
  if (value.includes('429') || value.includes('quota') || value.includes('billing') || value.includes('402')) return { kind: 'error', title: '请求受到额度或计费限制', help: '检查该项目的可用额度、调用频率和计费状态。', detail };
  if (value.includes('404') || value.includes('model_not_found')) return { kind: 'error', title: '找不到模型或接口', help: '检查模型名称和完整的 Chat Completions 接口地址。', detail };
  if (value.includes('403') || value.includes('permission_denied')) return { kind: 'error', title: '服务拒绝访问', help: '检查 API Key 的项目权限、接口限制及服务是否启用。', detail };
  if (value.includes('timed out') || value.includes('timeout') || value.includes('error sending request') || value.includes('connect')) return { kind: 'error', title: '未能完成网络连接', help: '检查代理是否运行，以及当前节点能否访问模型服务。也可以在代理设置中指定地址。', detail };
  return { kind: 'error', title: '连接测试未完成', help: '根据下方服务返回的原因检查设置，然后重试。', detail };
}

function Field({ label, hint, children, id }: { label: string; hint?: string; children: ReactNode; id: string }) {
  return <div className="settings-field"><label htmlFor={id}>{label}</label>{children}{hint && <p className="field-hint">{hint}</p>}</div>;
}

export default function Settings() {
  const [config, setConfig] = useState<Config | null>(null);
  const [saved, setSaved] = useState('');
  const [page, setPage] = useState<Page>(() => new URLSearchParams(window.location.search).get('section') === 'privacy' ? 'privacy' : 'connection');
  const permissions = usePermissions();
  const refreshPermissions = permissions.refresh;
  const checkedPermissions = useRef(false);
  const [service, setService] = useState<'gemini' | 'custom'>('gemini');
  const [loadingError, setLoadingError] = useState('');
  const [saveError, setSaveError] = useState('');
  const [saving, setSaving] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const [used, setUsed] = useState<number | null>(null);
  const revision = useRef(0);
  const busy = useRef(false);
  const contentRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!permissions.status || checkedPermissions.current) return;
    checkedPermissions.current = true;
    if (permissions.status.required && !permissions.status.ready) setPage('privacy');
  }, [permissions.status]);

  useEffect(() => {
    let active = true;
    const listener = getCurrentWindow().listen('show-permissions', () => {
      if (active) { setPage('privacy'); void refreshPermissions(); }
    });
    return () => { active = false; void listener.then(unlisten => unlisten()); };
  }, [refreshPermissions]);

  useEffect(() => {
    let active = true;
    invoke<Config>('get_config_for_frontend').then(value => {
      if (!active) return;
      value.ai_client.api_proxy ??= '';
      value.system.completion_enabled ??= true;
      setConfig(value); setSaved(JSON.stringify(value));
      setService(value.ai_client.api_url === GEMINI_URL ? 'gemini' : 'custom');
    }).catch(error => { if (active) setLoadingError(String(error)); });
    return () => { active = false; };
  }, []);

  const storedKey = config?.ai_client.api_key;
  useEffect(() => {
    if (storedKey === undefined) return;
    let active = true;
    const refresh = () => invoke<number>('get_used_token_command', { apikey: storedKey })
      .then(value => { if (active) setUsed(Number(value)); }).catch(() => { if (active) setUsed(null); });
    void refresh();
    const timer = setInterval(refresh, 15000);
    return () => { active = false; clearInterval(timer); };
  }, [storedKey]);

  useEffect(() => { contentRef.current?.scrollTo({ top: 0 }); }, [page]);

  const edit = (next: Config) => {
    revision.current += 1; setConfig(next); setResult(null); setSaveError('');
  };
  const updateAi = (patch: Partial<AiConfig>) => {
    if (config) edit({ ...config, ai_client: { ...config.ai_client, ...patch } });
  };
  const persist = async (snapshot: Config) => {
    setSaving(true); setSaveError('');
    try {
      await invoke('save_config_for_frontend', { config: snapshot });
      setSaved(JSON.stringify(snapshot));
    } catch (error) {
      setSaveError(String(error)); throw error;
    } finally { setSaving(false); }
  };
  const save = async () => {
    if (!config || busy.current || toggling) return;
    busy.current = true;
    try { await persist(config); } catch { /* The footer shows the actionable error. */ }
    finally { busy.current = false; }
  };
  const toggleCompletion = async () => {
    if (!config || saving || toggling) return;
    setToggling(true); setSaveError('');
    try {
      const enabled = await invoke<boolean>('set_completion_enabled', { enabled: !config.system.completion_enabled });
      // The switch saves immediately without saving/discarding other edits.
      setConfig(current => current && ({ ...current, system: { ...current.system, completion_enabled: enabled } }));
      setSaved(current => {
        const snapshot = JSON.parse(current) as Config;
        return JSON.stringify({ ...snapshot, system: { ...snapshot.system, completion_enabled: enabled } });
      });
    } catch (error) { setSaveError(String(error)); }
    finally { setToggling(false); }
  };
  const test = async () => {
    if (!config || busy.current || toggling) return;
    busy.current = true; setTesting(true); setResult(null);
    const currentRevision = revision.current;
    try {
      await persist(config);
      const timing = await invoke<{ first_token_ms: number; total_ms: number }>('test_api_connection', { config: config.ai_client });
      if (revision.current === currentRevision) setResult({ kind: 'success', first: timing.first_token_ms, total: timing.total_ms });
    } catch (error) {
      if (revision.current === currentRevision) setResult(explainError(error));
    } finally { busy.current = false; setTesting(false); }
  };

  if (!config) return <div className="settings-loading" role="status">{loadingError ? <><h2>无法加载设置</h2><p>{loadingError}</p><button onClick={() => window.location.reload()}>重新加载</button></> : <><LoadingOutlined spin /> 正在加载设置…</>}</div>;
  const ai = config.ai_client;
  const dirty = JSON.stringify(config) !== saved;
  const selected = pages.find(item => item.id === page)!;
  const apiMode = ai.provider === 'API';
  const canTest = Boolean(ai.api_key.trim() && ai.api_model.trim() && ai.api_url.trim());
  const needsPermissions = permissions.status?.required && !permissions.status.ready;
  const shortcut = (key: string) => config.keybinding[key]?.length ? config.keybinding[key].join(' / ') : '未设置';

  return <div className="settings-shell">
    <aside className="settings-sidebar">
      <div className="settings-brand"><img className="brand-symbol" src={brandIcon} alt="" /><div><strong>AInput</strong><span>AI 输入助手</span></div></div>
      <div className="sidebar-label">偏好设置</div>
      <nav aria-label="设置分类">{pages.map(item => <button key={item.id} className={page === item.id ? 'nav-item active' : 'nav-item'} aria-current={page === item.id ? 'page' : undefined} onClick={() => setPage(item.id)}><item.icon /><span>{item.label}</span>{item.id === 'privacy' && needsPermissions && <span className="permission-dot" aria-label="需要授权" />}</button>)}</nav>
      <div className="sidebar-tip"><span className="tip-mark">✦</span><p>接住下一句灵感</p><span>输入后稍候，按 <kbd>{shortcut('accept_candidate')}</kbd> 接受补全。</span></div>
      <div className="sidebar-foot"><span className="local-dot" />本机设置</div>
    </aside>
    <div className="settings-workspace">
      <header className="settings-header"><div><div className="eyebrow">AINPUT / SETTINGS</div><h1>{selected.label}</h1><p>{selected.description}</p></div><span className="header-icon"><selected.icon /></span></header>
      <main ref={contentRef} className="settings-content" id="settings-content">
        <section className={`completion-control ${config.system.completion_enabled ? '' : 'paused'}`} aria-label="补全总开关">
          <div><h2>自动补全 <span>{config.system.completion_enabled ? '已开启' : '已暂停'}</span></h2><p>{config.system.completion_enabled ? '输入时自动提供候选。关闭即可暂时停用。' : '已停止自动补全，开启后恢复。你的设置会保留。'}</p></div>
          <button type="button" role="switch" aria-label="自动补全" aria-checked={config.system.completion_enabled} className={`settings-toggle ${config.system.completion_enabled ? 'on' : ''}`} disabled={saving || toggling} onClick={() => void toggleCompletion()}><span /></button>
        </section>
        {page !== 'privacy' && needsPermissions && <div className="permission-banner" role="status"><div><strong>完成授权后，才能使用输入补全</strong><p>{permissions.status?.accessibility ? '辅助功能已允许，键盘连接尚未就绪。' : '请先允许 AInput 使用 macOS 辅助功能。'}</p></div><button className="settings-button secondary" onClick={() => setPage('privacy')}>前往授权<ArrowRightOutlined /></button></div>}
        {page !== 'privacy' && permissions.error && <p className="permission-error" role="alert">无法检查系统权限：{permissions.error}</p>}
        {page === 'privacy' && <MacPermissions {...permissions} />}
        <fieldset className="settings-editor" disabled={saving || testing || toggling}>
          {page === 'connection' && <>
            <div className="mode-switch" aria-label="连接方式"><button aria-pressed={apiMode} className={apiMode ? 'selected' : ''} onClick={() => updateAi({ provider: 'API' })}><ApiOutlined /> API 连接<span>使用服务商 Key</span></button><button aria-pressed={!apiMode} className={!apiMode ? 'selected' : ''} onClick={() => updateAi({ provider: 'CMD' })}><ThunderboltOutlined /> 本机命令<span>调用已安装的 CLI</span></button></div>
            {apiMode ? <>
              <section className="settings-card">
                <div className="card-heading"><div><h2>模型服务</h2><p>Key 仅保存在本机，连接时发送给所选服务。</p></div><span className="small-tag">STREAMING</span></div>
                <div className="field-grid">
                  <Field id="service" label="服务商"><select id="service" value={service} onChange={e => { const value = e.target.value as 'gemini' | 'custom'; setService(value); if (value === 'gemini') updateAi({ api_url: GEMINI_URL, api_model: GEMINI_MODEL }); else updateAi({ api_url: '', api_model: '' }); }}><option value="gemini">Google Gemini</option><option value="custom">自定义兼容接口</option></select></Field>
                  <Field id="model" label="模型名称"><input id="model" value={ai.api_model} spellCheck={false} onChange={e => updateAi({ api_model: e.target.value })} placeholder={GEMINI_MODEL} /></Field>
                </div>
                <Field id="api-key" label="API Key" hint={service === 'gemini' ? '使用 Google AI Studio 创建的 Gemini API Key。' : '填写此接口对应的 API Key。'}><input id="api-key" type="password" autoComplete="off" spellCheck={false} value={ai.api_key} onChange={e => updateAi({ api_key: e.target.value.trim() })} placeholder="粘贴你的 API Key" /></Field>
                <details className="advanced-settings"><summary>接口地址 <span>{service === 'gemini' ? 'Google 官方兼容接口' : '自定义'}</span></summary><Field id="api-url" label="完整接口 URL"><input id="api-url" type="url" value={ai.api_url} spellCheck={false} onChange={e => updateAi({ api_url: e.target.value.trim() })} placeholder="https://…/v1/chat/completions" /></Field></details>
              </section>
              <section className="settings-card network-card"><div className="card-heading"><div><h2>网络与代理</h2><p>从桌面启动，也会读取系统 HTTP(S) 代理。</p></div><span className="neutral-badge">{ai.api_proxy.trim() ? '手动代理' : '跟随系统'}</span></div><Field id="api-proxy" label="代理地址（可选）" hint="留空使用环境变量或系统代理；手动地址支持 http:// 和 https://。"><input id="api-proxy" value={ai.api_proxy} spellCheck={false} onChange={e => updateAi({ api_proxy: e.target.value.trim() })} placeholder="自动 · 使用当前系统代理" /></Field></section>
            </> : <section className="settings-card"><div className="card-heading"><div><h2>本机命令</h2><p>使用已安装并登录的 Codex、Gemini 等命令行工具。</p></div></div><Field id="command" label="执行命令" hint="没有占位符时，提示词通过标准输入传入。CLI 使用自己的网络和登录配置。"><textarea id="command" className="code-input" rows={5} value={ai.cmd} onChange={e => updateAi({ cmd: e.target.value })} placeholder="gemini -o text" /></Field><div className="inline-note">可用占位符 <code>{'{{prompt_file}}'}</code> <code>{'{{prompt}}'}</code></div></section>}
          </>}
          {page === 'input' && <>
            <section className="settings-card"><div className="card-heading"><div><h2>顺着思路，继续写</h2><p>输入文字后稍候，补全会出现在光标附近。</p></div></div><div className="completion-preview"><span>hello wo</span><span className="preview-completion">rld</span><kbd>{shortcut('accept_candidate')}</kbd></div><div className="shortcut-row"><span>接受整段补全</span><kbd>{shortcut('accept_candidate')}</kbd></div><div className="shortcut-row"><span>收起当前补全</span><kbd>{shortcut('exit_overlay')}</kbd></div><div className="shortcut-row"><span>仅接受第一个字</span><kbd>{shortcut('select_candidate_char_1')}</kbd></div><p className="field-hint">按住 Command、Control、Option 或 Shift 时，常规快捷键正常传递。</p></section>
            <section className="settings-card"><div className="card-heading"><div><h2>暂停补全的应用</h2><p>每行一个进程名称，例如 TextEdit、Safari 或 Code。</p></div></div><label className="sr-only" htmlFor="ignored-apps">忽略的应用</label><textarea id="ignored-apps" rows={5} spellCheck={false} value={config.ui_automation.ignore_apps.join('\n')} onChange={e => edit({ ...config, ui_automation: { ...config.ui_automation, ignore_apps: e.target.value.split('\n') } })} placeholder={'例如：\nSafari\nCode'} /><p className="field-hint">保存后重启 AInput 生效。</p></section>
          </>}
          {page === 'prompt' && <section className="settings-card"><div className="card-heading"><div><h2>补全指令</h2><p>建议要求模型只返回续写内容，避免解释或重复原文。</p></div></div><label htmlFor="prompt" className="sr-only">补全提示词</label><textarea id="prompt" className="code-input prompt-editor" spellCheck={false} value={ai.prompt} onChange={e => updateAi({ prompt: e.target.value })} /><div className="variable-list"><span>上下文变量</span>{['input_content', 'app_name', 'window_title', 'input_history', 'clipboard_contents'].map(variable => <code key={variable}>{`{{${variable}}}`}</code>)}</div><p className="field-hint">包含历史输入或剪贴板变量时，对应内容会成为发送给模型的上下文。</p></section>}
          {page === 'privacy' && <>
            <section className="settings-card"><div className="card-heading"><div><h2>敏感内容过滤</h2><p>发送给模型前，按本机规则替换匹配的敏感内容。</p></div><button className={`settings-toggle ${config.privacy.enable ? 'on' : ''}`} role="switch" aria-checked={config.privacy.enable} aria-label="敏感内容过滤" onClick={() => edit({ ...config, privacy: { ...config.privacy, enable: !config.privacy.enable } })}><span /></button></div><p className="inline-note">规则无法识别所有敏感信息。包含剪贴板和历史输入的提示词，会将这些上下文一并发送给所选服务。</p><details className="advanced-settings"><summary>查看过滤规则<span>{config.privacy.rules.length} 条</span></summary><div className="privacy-rules">{config.privacy.rules.map((rule, index) => <code key={index}>{rule}</code>)}</div></details></section>
            <section className="settings-card"><div className="card-heading"><div><h2>本机使用记录</h2><p>按当前 API Key 统计的补全链路字符量，包含输入与输出；并非服务商 Token 账单。</p></div></div><div className="usage-number">{used === null ? '—' : used.toLocaleString()}<span>字符</span></div></section>
          </>}
        </fieldset>
        {page === 'connection' && apiMode && <section className="connection-test">
          <div className="test-action"><div><h2>测试连接</h2><p>仅发送一条示例文本，不读取剪贴板或输入历史。</p></div><button className="settings-button primary" disabled={saving || testing || toggling || !canTest} onClick={() => void test()}>{testing ? <><LoadingOutlined spin />正在测试…</> : <>保存并测试<ArrowRightOutlined /></>}</button></div>
          {!canTest && <p className="field-hint">请先填写 API Key、模型名称和接口地址。</p>}
          {testing && <div className="testing-progress" role="status"><span />正在等待模型返回首段文字…</div>}
          {result?.kind === 'success' && <div className="test-result success" role="status"><div className="result-title"><CheckOutlined />连接成功，已收到模型回复</div><div className="timing-grid"><div><span>首字耗时</span><strong>{(result.first / 1000).toFixed(2)}<small> s</small></strong></div><div><span>总耗时</span><strong>{(result.total / 1000).toFixed(2)}<small> s</small></strong></div></div></div>}
          {result?.kind === 'error' && <div className="test-result error" role="alert"><div className="result-title">{result.title}</div><p>{result.help}</p><details><summary>查看原始错误</summary><pre>{result.detail}</pre></details></div>}
        </section>}
      </main>
      <footer className="settings-footer"><div role="status" className={saveError ? 'save-state failed' : 'save-state'}>{saveError || (saving ? '正在保存…' : dirty ? '有未保存的更改' : <><CheckOutlined />已保存到本机</>)}</div><button className="settings-button secondary" disabled={!dirty || saving || testing || toggling} onClick={() => void save()}>{saving ? '保存中…' : '保存更改'}</button></footer>
    </div>
  </div>;
}
