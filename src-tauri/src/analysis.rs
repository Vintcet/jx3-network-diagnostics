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
pub fn statistics(probes: &[Probe]) -> Vec<TargetStats> {
    let mut groups = BTreeMap::<(&str, &str), Vec<&Probe>>::new();
    for p in probes {
        groups.entry((&p.target, &p.method)).or_default().push(p);
    }
    groups
        .values()
        .map(|ps| {
            let first = ps[0];
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
            let threshold = if s.role == "gateway" {
                20.0f64.max(s.p50.unwrap_or(0.0) * 3.0 + 5.0)
            } else {
                100.0f64.max(s.p50.unwrap_or(0.0) * 2.0 + 20.0)
            };
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
                    format!(
                        "{}（{}）在此 10 秒窗口内至少两次超时或超过本次基线阈值。",
                        s.label, s.target
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
    let ms = |v: Option<f64>| v.map(|v| format!("{v:.1}")).unwrap_or_else(|| "—".into());
    let rows = report.stats.iter().map(|s|format!("<tr><td>{}<small>{}</small></td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",esc(&s.label),esc(&s.target),esc(&s.method),s.success,s.sent,s.timeouts,s.errors,ms(s.p50),ms(s.p95))).collect::<String>();
    let findings = report.findings.iter().map(|f|format!("<article><h2>{}</h2><p>{:.0}–{:.0} 秒 · 把握：{}</p><ul>{}</ul><p><b>建议：</b>{}</p></article>",esc(&f.title),f.start,f.end,esc(&f.confidence),f.evidence.iter().map(|e|format!("<li>{}</li>",esc(e))).collect::<String>(),esc(&f.suggestion))).collect::<String>();
    let events = report
        .events
        .iter()
        .map(|e| format!("<li>{:.1}s　{}</li>", e.elapsed, esc(&e.message)))
        .collect::<String>();
    format!("<!doctype html><html lang=zh-CN><meta charset=utf-8><meta name=viewport content='width=device-width, initial-scale=1'><title>网络诊断报告</title><style>body{{font:16px/1.7 'Segoe UI','Microsoft YaHei',sans-serif;color:#203047;max-width:1080px;margin:40px auto;padding:0 24px;background:#f2f5f8}}article,section{{background:white;padding:24px;margin:20px 0;border-radius:8px}}h1{{font-size:28px}}h2{{font-size:20px}}table{{width:100%;border-collapse:collapse}}th,td{{text-align:left;padding:10px;border-bottom:1px solid #dce2e9}}small{{display:block;color:#546376}}li{{margin:5px 0}}p{{overflow-wrap:anywhere}}@media print{{body{{background:white;margin:0}}article{{break-inside:avoid}}}}</style><h1>剑网三网络诊断报告</h1><p>{} / {}<br>开始：{}<br>结束：{}<br>持续 {:.1} 秒 / 计划 {} 秒　状态：{}</p><section><h2>测量结果</h2><p>超时次数属于对应探测方式，不能视作游戏丢包率。延迟单位 ms。</p><table><thead><tr><th>目标</th><th>方式</th><th>成功/发送</th><th>超时</th><th>其他错误</th><th>P50</th><th>P95</th></tr></thead><tbody>{rows}</tbody></table></section>{findings}<section><h2>事件时间线</h2><ul>{events}</ul></section><section><h2>观测范围</h2><p>{}</p><ul>{}</ul><p>原始日志：{}</p></section></html>",esc(&report.server.area),esc(&report.server.name),esc(&report.started_at),esc(&report.ended_at),report.duration_seconds,report.requested_seconds,esc(&report.status),esc(&report.traffic_status.message),report.limitations.iter().map(|s|format!("<li>{}</li>",esc(s))).collect::<String>(),esc(&report.log_dir))
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
