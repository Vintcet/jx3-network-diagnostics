use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct Activity {
    pub elapsed: f64,
    pub name: String,
    pub pid: u32,
    pub send_bps: f64,
    pub receive_bps: f64,
}
pub fn summarize(report: &Report) -> ReportSummary {
    let entry = report
        .stats
        .iter()
        .find(|s| s.method == "ICMP" && report.server.endpoints.iter().any(|e| e.ip == s.target));
    let tcp = report
        .stats
        .iter()
        .find(|s| s.method == "TCP" && s.role == "server");
    let has_relay = !report.relay_processes.is_empty();
    let has_relay_probe = report.stats.iter().any(|s| s.role == "relay");
    let has_game_probe = report.stats.iter().any(|s| s.role == "game");
    let mut facts = vec![];
    if let Some(s) = entry {
        if let (Some(p50), Some(p95)) = (s.p50, s.p95) {
            facts.push(format!(
                "所选区服入口：通常约 {p50:.0} ms；成功回应中约 95% 不超过 {p95:.0} ms。"
            ));
        }
        facts.push(format!(
            "向区服入口发送 {} 次 Ping，其中 {} 次未回应；这不是游戏丢包率。",
            s.sent, s.timeouts
        ));
    }
    if let Some(s) = tcp {
        facts.push(format!(
            "区服入口的 TCP 建连：{} 次成功 / {} 次尝试，{} 次超时，{} 次其他错误。",
            s.success, s.sent, s.timeouts, s.errors
        ));
    }
    if has_relay {
        facts.push(format!("发现游戏连接的本地中转进程：{}。它的上游可能包括加速节点与控制连接，无法据此确定最终游戏服务器。",report.relay_processes.iter().map(|p|p.name.as_str()).collect::<Vec<_>>().join("、")));
    }
    if !has_game_probe {
        facts.push("这份报告没有直接探测到游戏进程的公网远端；所选区服入口只能作为参照。".into());
    }
    let markers = report.events.iter().filter(|e| e.kind == "marker").count();
    if report.schema_version < 2 {
        facts.push(format!(
            "本次有 {markers} 次玩家卡顿标记。旧版未记录完整地址切换，无法由这份报告确认跨服行为。"
        ));
    } else {
        facts.push(format!("本次记录了 {markers} 次玩家卡顿标记、{} 次重点连接调整。连接变化与卡顿同时出现，也只能证明时间关联。",report.transitions.iter().filter(|t|t.from.is_some()&&t.to.is_some()).count()));
    }
    let headline = if report.stats.iter().all(|s| s.success == 0) {
        "有效测量不足，暂时无法判断网络问题"
    } else if has_relay && !has_game_probe {
        if has_relay_probe {
            "已观测加速器上游候选，最终游戏服仍不可见"
        } else {
            "已识别本地加速/代理，这轮未覆盖加速后的真实游戏线路"
        }
    } else if entry.is_some_and(|s| s.timeouts > 0) && tcp.is_some_and(|s| s.success == s.sent) {
        "出现探测未回应，但还不能认定游戏连接故障"
    } else if report.findings.iter().any(|f| f.level == "warning") {
        "发现网络波动，原因需要结合下面的证据排查"
    } else {
        "本次未发现证据充分的持续异常，偶发卡顿仍需对照"
    };
    ReportSummary{headline:headline.into(),facts,next_step:if has_relay{"保持日常加速设置，边玩边测，并在切图或卡顿时作标记；重点对照游戏、中转上游候选与公网参照。区服入口的直连探测不能代表加速器完整线路。"}else{"在真实卡顿或切图时点击“刚刚卡了”，比较事件前后网关、公网和游戏目标是否同步变化；只有一次时间重合时不要直接认定跨服导致卡顿。"}.into()}
}
pub fn transition_findings(report: &Report, probes: &[Probe]) -> Vec<Finding> {
    report.transitions.iter().filter(|t|t.elapsed>=10.0&&t.from.is_some()&&t.to.is_some()).take(100).map(|t|{
        let endpoint=|label:&Option<String>|report.game_endpoints.iter().find(|e|Some(format!("{} {}:{}",e.protocol,e.ip,e.port))==*label);
        let before=endpoint(&t.from);let after=endpoint(&t.to);
        let describe=|which:&str,ip:Option<&str>,start:f64,end:f64|{
            let ps=probes.iter().filter(|p|p.method=="ICMP"&&Some(p.target.as_str())==ip&&p.elapsed>=start&&p.elapsed<end).collect::<Vec<_>>();
            let mut values=ps.iter().filter(|p|p.status=="ok").filter_map(|p|p.ms).collect::<Vec<_>>();values.sort_by(f64::total_cmp);
            if values.len()<3{format!("{which}：有效样本不足，不能判断延迟是否变差。")}else{format!("{which}：通常约 {:.0} ms，{} 次探测中 {} 次未回应。",percentile(&values,0.5).unwrap(),ps.len(),ps.iter().filter(|p|p.status=="timeout").count())}
        };
        let markers=report.events.iter().filter(|e|e.kind=="marker"&&(e.elapsed-t.elapsed).abs()<=10.0).count();
        Finding{title:if markers>0{"连接调整附近有卡顿标记"}else{"已记录连接调整，可对照前后表现"}.into(),level:"info".into(),confidence:"仅时间关联".into(),start:(t.elapsed-20.0).max(0.0),end:(t.elapsed+20.0).min(report.duration_seconds),evidence:vec![format!("{:.1} 秒：{} → {}。",t.elapsed,t.from.as_deref().unwrap_or("—"),t.to.as_deref().unwrap_or("—")),describe("调整前 20 秒，旧地址",before.map(|e|e.ip.as_str()),(t.elapsed-20.0).max(0.0),t.elapsed),describe("调整后 20 秒，新地址",after.map(|e|e.ip.as_str()),t.elapsed,t.elapsed+20.0),format!("调整前后 10 秒内有 {markers} 次卡顿标记；加速节点变化不等于游戏物理服务器变化。")],suggestion:"重复同一切图/跨服操作，观察连接变化和卡顿是否反复同时出现，并检查公网与网关是否一起波动。".into()}
    }).collect()
}
fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(
            values[((values.len() as f64 * p).ceil() as usize)
                .saturating_sub(1)
                .min(values.len() - 1)],
        )
    }
}
pub fn delay_threshold(role: &str, baseline: f64) -> f64 {
    if role == "gateway" {
        20.0f64.max(baseline * 3.0 + 5.0)
    } else {
        100.0f64.max(baseline * 2.0 + 20.0)
    }
}
pub fn fill_thresholds(report: &mut Report) {
    for s in &mut report.stats {
        s.threshold_ms = if s.method == "ICMP" && s.assessable {
            s.p50.map(|v| delay_threshold(&s.role, v))
        } else {
            None
        };
    }
}
pub fn statistics(probes: &[Probe]) -> Vec<TargetStats> {
    let mut groups = BTreeMap::<(&str, &str), Vec<&Probe>>::new();
    for p in probes {
        groups.entry((&p.target, &p.method)).or_default().push(p);
    }
    groups
        .values()
        .map(|ps| {
            let first = ps
                .iter()
                .find(|p| p.role == "game")
                .or_else(|| ps.iter().find(|p| p.role == "relay"))
                .copied()
                .unwrap_or(ps[0]);
            let mut values: Vec<_> = ps
                .iter()
                .filter(|p| p.status == "ok")
                .filter_map(|p| p.ms)
                .collect();
            let mut jitter_sum = 0.0;
            let mut jitter_n = 0;
            let mut prev: Option<f64> = None;
            let (mut run, mut longest) = (0, 0);
            for p in ps {
                if p.status == "timeout" {
                    run += 1;
                    longest = longest.max(run);
                } else {
                    run = 0;
                }
                if p.status == "ok" {
                    if let (Some(a), Some(b)) = (prev, p.ms) {
                        jitter_sum += (a - b).abs();
                        jitter_n += 1;
                    }
                    prev = p.ms;
                } else {
                    prev = None;
                }
            }
            values.sort_by(f64::total_cmp);
            let timeouts = ps.iter().filter(|p| p.status == "timeout").count();
            TargetStats {
                target: first.target.clone(),
                label: first.label.clone(),
                role: first.role.clone(),
                method: first.method.clone(),
                sent: ps.len(),
                success: values.len(),
                timeouts,
                errors: ps.len() - values.len() - timeouts,
                p50: percentile(&values, 0.5),
                p95: percentile(&values, 0.95),
                max: values.last().copied(),
                jitter: if jitter_n > 0 {
                    Some(jitter_sum / jitter_n as f64)
                } else {
                    None
                },
                longest_timeout_run: longest,
                assessable: values.len() >= 10,
                threshold_ms: if first.method == "ICMP" && values.len() >= 10 {
                    percentile(&values, 0.5).map(|v| delay_threshold(&first.role, v))
                } else {
                    None
                },
            }
        })
        .collect()
}
pub fn findings(
    probes: &[Probe],
    activities: &[Activity],
    events: &[Event],
    duration: f64,
) -> Vec<Finding> {
    let stats = statistics(probes);
    let mut result = vec![];
    for s in &stats {
        if s.success == 0 && s.sent > 0 {
            result.push(Finding {
                title: format!("{} 的 {} 测量无法评估", s.label, s.method),
                level: "info".into(),
                confidence: "证据不足".into(),
                start: 0.0,
                end: duration,
                evidence: vec![format!(
                    "{} 次测量，{} 次超时，{} 次其他错误；没有成功样本。",
                    s.sent, s.timeouts, s.errors
                )],
                suggestion: if s.method == "ICMP" {
                    "目标可能限制 ICMP 回应。结合 TCP 与游戏实际连接判断，不将其报告为游戏丢包。"
                } else {
                    "检查是否拒绝连接、目标维护或线路不可达；当前不能单凭建连失败确定原因。"
                }
                .into(),
            });
        }
    }
    let mut windows = BTreeMap::<usize, Vec<&Probe>>::new();
    for p in probes {
        windows
            .entry((p.elapsed / 10.0) as usize)
            .or_default()
            .push(p);
    }
    for window in 0..=(duration / 10.0) as usize {
        let window_probes = windows.get(&window).map(Vec::as_slice).unwrap_or(&[]);
        let start = window as f64 * 10.0;
        let end = (start + 10.0).min(duration);
        let mut bad: Vec<&TargetStats> = vec![];
        let mut good: Vec<&TargetStats> = vec![];
        for s in stats.iter().filter(|s| s.method == "ICMP" && s.assessable) {
            let ps: Vec<_> = window_probes
                .iter()
                .filter(|p| {
                    p.target == s.target
                        && p.method == "ICMP"
                        && p.elapsed >= start
                        && p.elapsed < start + 10.0
                })
                .collect();
            if ps.len() < 3 {
                continue;
            }
            let threshold = delay_threshold(&s.role, s.p50.unwrap_or(0.0));
            let abnormal = ps
                .iter()
                .filter(|p| {
                    p.status == "timeout"
                        || (p.status == "ok" && p.ms.is_some_and(|v| v > threshold))
                })
                .count();
            if abnormal >= 2 {
                bad.push(s);
            } else if ps.iter().all(|p| p.status == "ok") {
                good.push(s);
            }
        }
        let bad_gateway = bad.iter().any(|s| s.role == "gateway");
        let bad_refs = bad.iter().filter(|s| s.role == "reference").count();
        let good_refs = good.iter().filter(|s| s.role == "reference").count();
        let bad_game = bad.iter().any(|s| s.role == "server" || s.role == "game");
        let bad_relay = bad.iter().any(|s| s.role == "relay");
        let good_gateway = good.iter().any(|s| s.role == "gateway");
        let decision = if bad_gateway && bad_refs > 0 && bad_game {
            Some(("本地连接或共同本地因素值得优先检查", "中", "建议使用网线对照复测，并检查无线连接、路由器负载和本机调度。网关自身也可能限制探测回应。"))
        } else if good_gateway && bad_refs >= 2 && bad_game {
            Some(("网关之后的共同路径出现同步异常", "中", "记录此时间段，检查路由器出口与宽带接入，并在不同时段复测；当前不能直接判定运营商责任。"))
        } else if bad_game && good_refs >= 2 {
            Some((
                "游戏相关目标的探测出现独立波动",
                "中",
                "结合 TCP 建连、实际游戏连接和体感标记确认；排查游戏相关线路、接入端及加速路径。",
            ))
        } else if bad_relay && good_refs >= 2 {
            Some(("加速/代理候选地址的探测出现波动","低","此地址属于中转程序的上游候选，可能是控制连接或加速节点；结合实际流量和游戏体感核对，不能直接认定游戏服故障。"))
        } else if !bad.is_empty() {
            Some((
                "部分目标出现探测波动",
                "低",
                "参照证据不足，暂不能定位环节。延长测试并覆盖实际卡顿时段。",
            ))
        } else {
            None
        };
        if let Some((title, confidence, suggestion)) = decision {
            let mut evidence: Vec<String> = bad
                .iter()
                .map(|s| {
                    let baseline = s.p50.unwrap_or(0.0);
                    let threshold = delay_threshold(&s.role, baseline);
                    let values = window_probes
                        .iter()
                        .filter(|p| {
                            p.target == s.target
                                && p.method == "ICMP"
                                && (p.status == "timeout"
                                    || (p.status == "ok" && p.ms.is_some_and(|v| v > threshold)))
                        })
                        .map(|p| {
                            let at = chrono::DateTime::parse_from_rfc3339(&p.at)
                                .map(|v| {
                                    v.with_timezone(&chrono::Local)
                                        .format("%H:%M:%S")
                                        .to_string()
                                })
                                .unwrap_or_else(|_| format!("第 {:.1} 秒", p.elapsed));
                            if p.status == "timeout" {
                                format!("{at} 等待 800 ms 后未收到回应")
                            } else {
                                format!("{at} 实测 {:.1} ms", p.ms.unwrap_or(0.0))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("；");
                    format!(
                        "{}（{}）：基线 {:.1} ms，异常阈值 {:.1} ms。本窗口异常样本：{}。",
                        s.label, s.target, baseline, threshold, values
                    )
                })
                .collect();
            if !good.is_empty() {
                evidence.push(format!(
                    "同窗口保持回应：{}。",
                    good.iter()
                        .map(|s| s.label.as_str())
                        .collect::<Vec<_>>()
                        .join("、")
                ));
            }
            for s in stats
                .iter()
                .filter(|s| s.role == "server" && s.method == "TCP")
            {
                let ps: Vec<_> = window_probes
                    .iter()
                    .filter(|p| {
                        p.target == s.target
                            && p.method == "TCP"
                            && p.elapsed >= start
                            && p.elapsed < start + 10.0
                    })
                    .collect();
                if !ps.is_empty() && ps.iter().all(|p| p.status == "ok") {
                    evidence
                        .push("同期区服 TCP 建连成功；ICMP 波动不能直接等同游戏连接故障。".into());
                }
            }
            if let Some(a) = activities
                .iter()
                .filter(|a| {
                    a.elapsed >= start
                        && a.elapsed < start + 10.0
                        && (a.send_bps > 524_288.0 || a.receive_bps > 2_097_152.0)
                })
                .max_by(|a, b| {
                    (a.send_bps + a.receive_bps).total_cmp(&(b.send_bps + b.receive_bps))
                })
            {
                evidence.push(format!("同期后台进程 {}（PID {}）上传 {:.2} MB/s、下载 {:.2} MB/s；仅为时间关联，可暂停后对照复测。",a.name,a.pid,a.send_bps/1_000_000.0,a.receive_bps/1_000_000.0));
            }
            if let Some(last) = result
                .last_mut()
                .filter(|f| f.title == title && (f.end - start).abs() < 0.01)
            {
                last.end = end;
                for e in evidence {
                    if last.evidence.len() < 12 && !last.evidence.contains(&e) {
                        last.evidence.push(e);
                    }
                }
            } else {
                result.push(Finding {
                    title: title.into(),
                    level: "warning".into(),
                    confidence: confidence.into(),
                    start,
                    end,
                    evidence,
                    suggestion: suggestion.into(),
                });
            }
        }
    }
    if result.iter().all(|f| f.level != "warning") {
        result.push(Finding {title: if probes.is_empty() { "尚无可分析的探测样本" } else { "未发现证据充分的同步网络异常" }.into(),level:"info".into(),confidence: if stats.iter().filter(|s|s.assessable).count()<3 { "证据不足" } else { "有限观测" }.into(),start:0.0,end:duration,evidence:vec![format!("有效观测时长 {:.0} 秒，{} 个目标／方式具有至少 10 个成功样本。",duration,stats.iter().filter(|s|s.assessable).count()),format!("玩家记录了 {} 次卡顿标记。",events.iter().filter(|e|e.kind=="marker").count())],suggestion:"此结果只覆盖测量时段和可回应目标。若游戏仍卡顿，请检查实际连接、加速路径、本机性能或延长测试。".into()});
    }
    result
}
pub fn html(report: &Report) -> String {
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }
    let local_stamp = |text: &str| {
        chrono::DateTime::parse_from_rfc3339(text)
            .map(|at| {
                at.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|_| text.into())
    };
    let local_at = |seconds: f64| {
        chrono::DateTime::parse_from_rfc3339(&report.started_at)
            .map(|at| {
                (at + chrono::Duration::milliseconds((seconds * 1000.0) as i64))
                    .with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|_| format!("{seconds:.1}s"))
    };
    let summary = summarize(report);
    let readout = format!(
        "<section><h2>先看结论：{}</h2><ul>{}</ul><p><b>接下来怎么做：</b>{}</p></section>",
        esc(&summary.headline),
        summary
            .facts
            .iter()
            .map(|s| format!("<li>{}</li>", esc(s)))
            .collect::<String>(),
        esc(&summary.next_step)
    );
    let changes = if report.connection_changes.is_empty() {
        String::new()
    } else {
        format!("<section><h2>连接地址变化记录</h2><p>下列时间为观测时间。中转上游候选不等于最终游戏服务器；时间重合不能单独证明跨服导致卡顿。</p><ul>{}</ul></section>",report.connection_changes.iter().map(|c|format!("<li>第 {:.1} 秒：{} {} {}:{}（{}）</li>",c.elapsed,match c.kind.as_str(){"first_seen"=>"首次看到地址","left_table"=>"连接表不再显示","resumed"=>"地址再次出现",_=>"近期无收发"},esc(&c.endpoint.protocol),esc(&c.endpoint.ip),c.endpoint.port,if c.endpoint.source=="relay"{"中转上游候选"}else{"游戏进程"})).collect::<String>())
    };
    let ms = |v: Option<f64>| v.map(|v| format!("{v:.1}")).unwrap_or_else(|| "—".into());
    let rules = crate::report_text::RULES;
    let rows = report.stats.iter().map(|s|format!("<tr><td>{}<small>{}</small></td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",esc(&s.label),esc(&s.target),esc(&s.method),s.success,s.sent,s.timeouts,s.errors,ms(s.p50),if s.method=="ICMP"{ms(s.threshold_ms)}else{"不适用".into()},ms(s.p95))).collect::<String>();
    let findings = report.findings.iter().map(|f|format!("<article><h2>{}</h2><p>{}–{}（第 {:.0} 秒起） · 把握：{}</p><ul>{}</ul><p><b>建议：</b>{}</p></article>",esc(&f.title),local_at(f.start),local_at(f.end),f.start,esc(&f.confidence),f.evidence.iter().map(|e|format!("<li>{}</li>",esc(e))).collect::<String>(),esc(&f.suggestion))).collect::<String>();
    let events = report
        .events
        .iter()
        .map(|e| {
            format!(
                "<li>{}（第 {:.1} 秒）　{}</li>",
                local_at(e.elapsed),
                e.elapsed,
                esc(&e.message)
            )
        })
        .collect::<String>();
    format!("<!doctype html><html lang=zh-CN><meta charset=utf-8><meta name=viewport content='width=device-width, initial-scale=1'><title>网络诊断报告</title><style>body{{font:16px/1.7 'Segoe UI','Microsoft YaHei',sans-serif;color:#203047;max-width:1080px;margin:40px auto;padding:0 24px;background:#f2f5f8}}article,section{{background:white;padding:24px;margin:20px 0;border-radius:8px}}h1{{font-size:28px}}h2{{font-size:20px}}table{{width:100%;border-collapse:collapse}}th,td{{text-align:left;padding:10px;border-bottom:1px solid #dce2e9}}small{{display:block;color:#546376}}li{{margin:5px 0}}p{{overflow-wrap:anywhere}}@media print{{body{{background:white;margin:0}}article{{break-inside:avoid}}}}</style><h1>剑网三网络诊断报告</h1>{readout}<p>{} / {}<br>开始：{}<br>结束：{}<br>持续 {:.1} 秒 / 计划 {} 秒　状态：{}</p><section><h2>测量结果</h2><p>超时次数属于对应探测方式，不能视作游戏丢包率。延迟单位 ms。</p><table><thead><tr><th>目标</th><th>方式</th><th>成功/发送</th><th>超时</th><th>其他错误</th><th>基线 P50</th><th>异常阈值</th><th>95% 上限</th></tr></thead><tbody>{rows}</tbody></table></section><section><h2>判定规则</h2><p>{rules}</p></section>{findings}{changes}<section><h2>事件时间线</h2><ul>{events}</ul></section><section><h2>观测范围</h2><p>{}</p><ul>{}</ul><p>原始日志：{}</p></section></html>",esc(&report.server.area),esc(&report.server.name),esc(&local_stamp(&report.started_at)),esc(&local_stamp(&report.ended_at)),report.duration_seconds,report.requested_seconds,esc(&report.status),esc(&report.traffic_status.message),report.limitations.iter().map(|s|format!("<li>{}</li>",esc(s))).collect::<String>(),esc(&report.log_dir))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample(role: &str, t: f64, status: &str, ms: Option<f64>) -> Probe {
        Probe {
            at: String::new(),
            elapsed: t,
            target: role.into(),
            label: role.into(),
            role: role.into(),
            method: "ICMP".into(),
            status: status.into(),
            ms,
            detail: None,
        }
    }
    #[test]
    fn threshold_and_evidence_include_actual_values() {
        assert_eq!(delay_threshold("server", 43.0), 106.0);
        assert_eq!(delay_threshold("gateway", 2.0), 20.0);
        let ps = (0..30)
            .map(|i| {
                sample(
                    "server",
                    i as f64 * 2.0,
                    "ok",
                    Some(if i >= 20 { 150.0 } else { 40.0 }),
                )
            })
            .collect::<Vec<_>>();
        let stats = statistics(&ps);
        assert_eq!(stats[0].threshold_ms, Some(100.0));
        let output = findings(&ps, &[], &[], 60.0);
        let evidence = output
            .iter()
            .flat_map(|f| &f.evidence)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(evidence.contains("基线 40.0 ms"));
        assert!(evidence.contains("阈值 100.0 ms"));
        assert!(evidence.contains("实测 150.0 ms"));
    }
    #[test]
    fn nonresponsive_is_not_packet_loss_diagnosis() {
        let ps = (0..20)
            .map(|i| sample("server", i as f64 * 2.0, "timeout", None))
            .collect::<Vec<_>>();
        assert!(findings(&ps, &[], &[], 40.0)
            .iter()
            .all(|f| f.level != "warning"));
        assert!(!statistics(&ps)[0].assessable);
    }
    #[test]
    fn refused_is_an_error_not_a_timeout() {
        let ps = vec![sample("server", 0.0, "refused", None)];
        let s = statistics(&ps);
        assert_eq!(s[0].timeouts, 0);
        assert_eq!(s[0].errors, 1);
    }
    #[test]
    fn correlates_shared_path_only_with_references() {
        let mut ps = vec![];
        for i in 0..30 {
            for (target, role) in [
                ("g", "gateway"),
                ("r1", "reference"),
                ("r2", "reference"),
                ("s", "server"),
            ] {
                let mut p = sample(
                    role,
                    i as f64 * 2.0,
                    "ok",
                    Some(if i >= 20 && role != "gateway" {
                        300.0
                    } else {
                        10.0
                    }),
                );
                p.target = target.into();
                ps.push(p);
            }
        }
        let f = findings(&ps, &[], &[], 60.0);
        assert!(f.iter().any(|f| f.title.contains("网关之后")));
    }
    #[test]
    fn jitter_does_not_bridge_timeouts() {
        let ps = vec![
            sample("s", 0.0, "ok", Some(10.0)),
            sample("s", 2.0, "timeout", None),
            sample("s", 4.0, "ok", Some(100.0)),
        ];
        assert_eq!(statistics(&ps)[0].jitter, None);
    }
}
