import React, { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';

const labelStyle: React.CSSProperties = {
  fontSize: 13,
  color: '#e0e0e0',
  marginBottom: 2,
  fontWeight: 500,
};
const inputStyle: React.CSSProperties = {
  fontSize: 13,
  padding: '4px 8px',
  border: 'none',
  borderRadius: 6,
  background: '#23232b',
  color: '#fff',
  outline: 'none',
  minWidth: 0,
  width: '100%',
  boxSizing: 'border-box',
  marginBottom: 10,
};
const textareaStyle: React.CSSProperties = {
  ...inputStyle,
  height: 144,
  resize: 'vertical',
};
const sectionStyle: React.CSSProperties = {
  margin: '0 auto',
  marginTop: 32,
  maxWidth: 400,
  width: '100%',
  display: 'flex',
  flexDirection: 'column',
  paddingTop: 8,
};
const containerStyle: React.CSSProperties = {
  width: '100vw',
  height: '100vh',
  minWidth: 0,
  minHeight: 0,
  background: '#181820',
  boxSizing: 'border-box',
  display: 'flex',
  flexDirection: 'column',
  justifyContent: 'flex-start',
  alignItems: 'center',
  padding: '0 16px',
};
const streamedCharsStyle: React.CSSProperties = {
  fontSize: 12,
  color: '#888',
  textAlign: 'right',
  width: '100%',
  marginTop: 8,
  marginBottom: 0,
  userSelect: 'all',
};

const Settings: React.FC = () => {
  const [config, setConfig] = useState<any>({ ai_client: {} });
  const saveTimer = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    invoke('get_config_for_frontend').then((cfg: any) => {
      setConfig(cfg);
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    const fetchToken = () => {
      invoke('get_used_token_command', { apikey: config.ai_client?.api_key || '' }).then((used: any) => {
        if (!cancelled) setConfig((prev: any) => ({
          ...prev,
          ai_client: { ...prev.ai_client, usedToken: String(used) }
        }));
      });
    };
    fetchToken();
    const timer = setInterval(fetchToken, 1000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [config.ai_client?.api_key]);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) => {
    const { name, value } = e.target;
    setConfig((prev: any) => ({
      ...prev,
      ai_client: { ...prev.ai_client, [name]: value }
    }));
  };

  const handleBlur = () => {
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      const { usedToken, ...ai_client } = config.ai_client || {};
      invoke('save_config_for_frontend', { config: { ...config, ai_client } })
        .then(() => console.info('[Settings] config saved'));
    }, 300);
  };

  const aiClient = config.ai_client || {};

  return (
    <div style={containerStyle}>
      <section style={sectionStyle}>
        <label style={labelStyle} htmlFor="provider">Provider</label>
        <select id="provider" name="provider" value={aiClient.provider || ''} onChange={handleChange} onBlur={handleBlur} style={inputStyle}>
          <option value="API">API</option>
          <option value="CMD">CMD</option>
        </select>
        <label style={labelStyle} htmlFor="api_url">API URL</label>
        <input id="api_url" name="api_url" value={aiClient.api_url || ''} onChange={handleChange} onBlur={handleBlur} style={inputStyle} placeholder="API URL" />
        <label style={labelStyle} htmlFor="api_key">API Key</label>
        <input id="api_key" name="api_key" value={aiClient.api_key || ''} onChange={handleChange} onBlur={handleBlur} style={inputStyle} placeholder="API Key" />
        <label style={labelStyle} htmlFor="api_model">Model</label>
        <input id="api_model" name="api_model" value={aiClient.api_model || ''} onChange={handleChange} onBlur={handleBlur} style={inputStyle} placeholder="Model" />
        <label style={labelStyle} htmlFor="cmd">CMD</label>
        <textarea
          id="cmd"
          name="cmd"
          value={aiClient.cmd || ''}
          onChange={handleChange}
          onBlur={handleBlur}
          style={{ ...textareaStyle, height: 60 }}
          placeholder="CMD"
          rows={3}
        />
        <label style={labelStyle} htmlFor="prompt">Prompt</label>
        <textarea id="prompt" name="prompt" value={aiClient.prompt || ''} onChange={handleChange} onBlur={handleBlur} style={textareaStyle} placeholder="Prompt" />
        <div style={streamedCharsStyle}>Streamed Chars {aiClient.usedToken || ''}</div>
      </section>
    </div>
  );
};

export default Settings; 