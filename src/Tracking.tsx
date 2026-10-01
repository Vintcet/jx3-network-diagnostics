import { useState } from 'react';
import { ArrowRight, Network, Info } from 'lucide-react';
import type { GameContext, GameTracking, Report } from './types';

const time = (n: number) => `${Math.floor(n / 60).toString().padStart(2, '0')}:${Math.floor(n % 60).toString().padStart(2, '0')}`;
export function eventTime(start: string, elapsed: number) { return new Date(new Date(start).getTime() + elapsed * 1000).toLocaleTimeString('zh-CN', { hour12: false }); }
const stateName: Record<string, string> = { active: '有收发', connected: '已连接', recent: '保留对照', inactive: '历史记录' };
export const changeName: Record<string, string> = { first_seen: '首次看到地址', resumed: '地址再次出现', left_table: '连接表不再显示', quiet: '近期无收发' };

export function FollowingPanel({ tracking, context }: { tracking?: GameTracking; context: GameContext | null }) {
  const [history, setHistory] = useState(false);
  const relays = tracking?.relays ?? context?.relays ?? [];
  const primary = tracking?.endpoints.find(e => e.id === tracking.primaryId);
  const endpoints = (tracking?.endpoints ?? []).filter(e => history || e.state !== 'inactive');
  return <section className="panel following-panel"><div className="panel-heading"><div><h2>自动跟随游戏连接</h2><p>区服选择用于参照；新发现的地址会自动加入记录与探测。</p></div><span className="tag">{relays.length ? '已识别本地中转' : '按可见连接跟随'}</span></div>
    {relays.length > 0 && <div className="connection-chain"><span>游戏进程</span><ArrowRight size={16} aria-hidden="true" /><span>{relays.map(p => p.name).join('、')}</span><ArrowRight size={16} aria-hidden="true" /><span>该进程的上游候选</span><small>最终游戏服可能位于加速节点之后，无法直接看到</small></div>}
    <p className="tracking-note"><Info size={16} aria-hidden="true" />{tracking?.message || (relays.length ? '已通过本地端口对应关系识别中转进程。开始测试后，游戏和中转上游将分别记录；上游候选不等同游戏服务器。' : context?.matches.length ? `连接与目录入口匹配：${context.matches.map(s => `${s.area} / ${s.name}`).join('、')}。` : '选中游戏进程后开始测试。无法直接匹配区服时，可手动指定参照区服。')}</p>
    {primary && <div className="primary-endpoint"><Network size={18} aria-hidden="true" /><div><span>{primary.source === 'relay' ? '重点观测：中转上游候选' : '重点观测：游戏可见连接'}</span><strong>{primary.ip}:{primary.port} <small>{primary.protocol}</small></strong></div><div className="endpoint-match">{primary.catalogNames.length ? `目录入口匹配：${primary.catalogNames.join('、')}` : primary.source === 'relay' ? '无法由此确认最终区服' : '目录外地址，具体业务未知'}</div></div>}
    {endpoints.length > 0 && <><div className="tracking-controls"><span>同 IP 不同端口分别记录；ICMP 按 IP 测量</span><label><input type="checkbox" checked={history} onChange={e => setHistory(e.target.checked)} />显示历史地址</label></div><div className="scroll-table"><table><thead><tr><th>连接来源</th><th>地址 / 协议</th><th>首次观测</th><th>最近收发</th><th>当前状态</th></tr></thead><tbody>{endpoints.slice(0, 40).map(e => <tr className={e.id === tracking?.primaryId ? 'focused-endpoint' : ''} key={e.id}><td>{e.source === 'relay' ? '中转上游候选' : '游戏进程'}<small>{e.processName}</small></td><td className="mono">{e.ip}:{e.port}<small>{e.protocol}{e.catalogNames.length ? ` · ${e.catalogNames.join('、')}` : ''}</small></td><td>{time(e.firstSeen)}</td><td>{e.lastActive == null ? '暂无事件' : time(e.lastActive)}</td><td>{stateName[e.state]}<small>{e.probing ? '正在按 IP 探测' : e.state === 'inactive' ? '原始日志已保留' : '等待轮测或不支持主动探测'}</small></td></tr>)}</tbody></table></div>{endpoints.length > 40 && <p className="tracking-note">显示前 40 条，完整地址记录保存在日志中。</p>}</>}
  </section>;
}
export function ReportReadout({ report }: { report: Report }) {
  if (!report.summary?.headline) return null;
  return <section className="panel report-readout"><span className="readout-label">先看结论</span><h2>{report.summary.headline}</h2><ul>{report.summary.facts.map((s, i) => <li key={i}>{s}</li>)}</ul><div className="readout-next"><strong>接下来怎么做</strong><p>{report.summary.nextStep}</p></div></section>;
}
export function ConnectionHistory({ report }: { report: Report }) {
  const changes = report.connectionChanges ?? [];
  if (!changes.length) return null;
  return <section className="panel"><div className="panel-heading"><div><h2>连接地址变化记录</h2><p>时间为首次观测到变化的时刻；同一 IP 换端口也会记录。</p></div><span className="count">{changes.length} 条</span></div><div className="scroll-table"><table><thead><tr><th>观测时间</th><th>变化</th><th>来源 / 地址</th><th>前后 10 秒卡顿标记</th></tr></thead><tbody>{changes.slice(-100).map((c, i) => <tr key={i}><td>{eventTime(report.startedAt, c.elapsed)}<small>第 {time(c.elapsed)}</small></td><td>{changeName[c.kind] ?? c.kind}</td><td>{c.endpoint.source === 'relay' ? '中转上游候选' : '游戏进程'}<small>{c.endpoint.protocol} {c.endpoint.ip}:{c.endpoint.port}</small></td><td>{report.events.filter(e => e.kind === 'marker' && Math.abs(e.elapsed - c.elapsed) <= 10).length} 次</td></tr>)}</tbody></table></div><p className="tracking-note">连接与卡顿在时间上重合，并不单独证明跨服导致卡顿。新地址不一定有切换前的测量样本。</p></section>;
}
