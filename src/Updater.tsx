import { useEffect, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { Download, RefreshCw, X } from 'lucide-react';
import appInfo from '../package.json';
import './updater.css';

type UpdateInfo = { version: string; notes: string };
type Progress = { phase: 'downloading' | 'installing'; downloaded: number; total: number | null };
type Status = 'idle' | 'checking' | 'current' | 'available' | 'downloading' | 'installing' | 'error';

export function useUpdater(desktop: boolean) {
  const [status, setStatus] = useState<Status>('idle');
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [error, setError] = useState('');
  const [visible, setVisible] = useState(false);
  const inFlight = useRef(false);

  async function check(manual = true) {
    if (!desktop || inFlight.current) return;
    inFlight.current = true;
    setStatus('checking'); setError(''); setUpdate(null);
    if (manual) setVisible(true);
    try {
      const result = await invoke<UpdateInfo | null>('check_update');
      setUpdate(result); setStatus(result ? 'available' : 'current');
      if (result) setVisible(true);
    } catch (e) {
      setError(String(e)); setStatus('error');
    } finally { inFlight.current = false; }
  }

  useEffect(() => {
    if (!desktop) return;
    const timer = setTimeout(() => void check(false), 2000);
    return () => clearTimeout(timer);
  }, [desktop]);

  async function install() {
    if (!update || inFlight.current) return;
    inFlight.current = true;
    setStatus('downloading'); setProgress(null); setError(''); setVisible(true);
    const onProgress = new Channel<Progress>();
    onProgress.onmessage = value => { setProgress(value); setStatus(value.phase); };
    try {
      await invoke('install_update', { onProgress });
      // Windows closes this process after starting the verified installer.
      setStatus('installing');
    } catch (e) {
      setError(String(e)); setStatus('error'); inFlight.current = false;
    }
  }

  return { desktop, status, update, progress, error, visible, setVisible, check, install,
    installing: status === 'downloading' || status === 'installing' };
}

type Updater = ReturnType<typeof useUpdater>;

export function VersionFooter({ updater }: { updater: Updater }) {
  const label = updater.status === 'checking' ? '正在检查更新…'
    : updater.installing ? '正在更新…'
    : updater.update ? `发现新版 ${updater.update.version}`
    : updater.status === 'error' ? '检查失败，重试'
    : updater.status === 'current' ? '已是最新版 · 检查'
    : '检查更新';
  return <div className="app-meta" aria-label={`版本 ${appInfo.version}，更新日期 ${appInfo.releaseDate}，作者 ${appInfo.author}`}>
    <dl><dt>版本</dt><dd>v{appInfo.version}</dd><dt>更新日期</dt><dd><time dateTime={appInfo.releaseDate}>{appInfo.releaseDate}</time></dd><dt>作者</dt><dd>{appInfo.author}</dd></dl>
    <button className="update-check" aria-label={label} title={updater.desktop ? label : '请在桌面应用中检查更新'} disabled={!updater.desktop || updater.status === 'checking' || updater.installing} onClick={() => updater.update ? updater.setVisible(true) : void updater.check()}>
      <RefreshCw size={15} aria-hidden="true" className={updater.status === 'checking' ? 'spin' : ''} /><span>{label}</span>
    </button>
  </div>;
}

export function UpdateNotice({ updater, running }: { updater: Updater; running: boolean }) {
  if (!updater.visible) return null;
  const { status, update, progress, error } = updater;
  const percent = progress?.total ? Math.min(100, Math.floor(progress.downloaded / progress.total * 100)) : null;
  return <section className="update-notice" aria-label="软件更新">
    <div className="update-heading"><strong>{update ? `新版本 v${update.version}` : status === 'current' ? '已是最新版本' : status === 'error' ? '检查更新失败' : '正在检查更新…'}</strong>
      {!updater.installing && <button className="icon-button" aria-label="收起更新提示" onClick={() => updater.setVisible(false)}><X size={16} aria-hidden="true" /></button>}
    </div>
    {update && <>
      {update.notes && <details><summary>查看更新内容</summary><p className="release-notes">{update.notes}</p></details>}
      <p>更新将关闭工具并安装新版，已有测试记录保留。单文件版更新后，请从开始菜单启动。</p>
      {running && <p>测试进行中，请结束测试并等待报告保存后再安装。</p>}
    </>}
    {status === 'current' && <p>当前版本 v{appInfo.version}，启动时会自动检查 GitHub 上的新版本。</p>}
    {error && <p className="update-error" role="alert">{error}</p>}
    {updater.installing && <div role="status"><p>{status === 'installing' ? '签名校验通过，正在启动安装程序…' : `正在下载${percent == null ? '…' : ` ${percent}%`}${progress ? `（${(progress.downloaded / 1048576).toFixed(1)} MB）` : ''}`}</p><progress aria-label="更新下载进度" max={100} value={status === 'installing' ? 100 : percent ?? undefined} /></div>}
    {!updater.installing && <div className="update-actions">
      {update && <button className="button primary" disabled={running} onClick={() => void updater.install()}><Download size={16} aria-hidden="true" />{error ? '重试下载安装' : '下载并安装'}</button>}
      {status === 'error' && !update && <button className="button" onClick={() => void updater.check()}>重试检查</button>}
    </div>}
  </section>;
}
