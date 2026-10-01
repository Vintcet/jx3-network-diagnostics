use crate::{analysis, model::*};
use std::fmt::Write;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

pub const RULES:&str="基线为本次测试成功回应延迟的中位数（P50）。网关阈值=max(20 ms, 基线×3+5 ms)；公网、游戏及中转候选阈值=max(100 ms, 基线×2+20 ms)。目标至少累计10个成功样本；固定10秒窗口内至少3次探测，其中至少2次超时或实际延迟严格大于阈值，才标记波动。ICMP等待上限800 ms；超时不当作测得的800 ms延迟。TCP建连等待上限900 ms，单独统计，不使用上述波动阈值。";
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}
fn number(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.1}"))
        .unwrap_or_else(|| "未测得".into())
}
fn local(s: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S %:z")
                .to_string()
        })
        .unwrap_or_else(|_| s.into())
}
fn at(report: &Report, elapsed: f64) -> String {
    chrono::DateTime::parse_from_rfc3339(&report.started_at)
        .map(|t| {
            (t + chrono::Duration::milliseconds((elapsed * 1000.0) as i64))
                .with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| format!("{elapsed:.1}s"))
}

pub fn markdown(report: &Report) -> String {
    let mut text = String::new();
    let summary = analysis::summarize(report);
    let _=writeln!(text,"# 剑网三网络诊断报告\n\n这是一份可以直接阅读、上传给支持文本附件的 AI，或复制到聊天窗口的诊断资料。所有结论来自单台电脑的观测；请结合证据分析，不把探测超时当游戏丢包、不把地址变化直接当成跨服原因。\n\n## 测试背景\n\n- 参照区服：{} / {}\n- 开始时间：{}\n- 结束时间：{}\n- 实际持续：{:.1} 秒；计划：{} 秒\n- 状态：{}\n- 记录编号：{}\n",cell(&report.server.area),cell(&report.server.name),local(&report.started_at),local(&report.ended_at),report.duration_seconds,report.requested_seconds,match report.status.as_str(){"completed"=>"到时完成","stopped"=>"手动提前结束",_=>&report.status},cell(&report.id));
    if let Some(p) = &report.game_process {
        let _ = writeln!(
            text,
            "- 游戏进程：{}，PID {}\n- EXE 路径：{}",
            cell(&p.name),
            p.pid,
            cell(p.path.as_deref().unwrap_or("不可读取"))
        );
    }
    if let Some(env) = &report.environment {
        for a in env.adapters.iter().filter(|a| a.status == "已连接").take(8) {
            let _ = writeln!(
                text,
                "- 网卡：{} / {}；地址 {}；网关 {}；DNS {}{}",
                cell(&a.name),
                cell(&a.kind),
                a.addresses.join(", "),
                a.gateways.join(", "),
                a.dns.join(", "),
                if Some(a.index) == env.selected_interface {
                    "（开始时的探测出接口）"
                } else {
                    ""
                }
            );
        }
        for line in env.wifi.iter().chain(env.notes.iter()).take(12) {
            let _ = writeln!(text, "- {}", cell(line));
        }
    }
    let _ = writeln!(text, "\n## 先看结论\n\n**{}**\n", summary.headline);
    for fact in &summary.facts {
        let _ = writeln!(text, "- {}", cell(fact));
    }
    let _=writeln!(text,"\n下一步：{}\n\n## 测量与阈值口径\n\n{}\n\nICMP 通常约每2秒一次，TCP建连约每6秒一次。中位数、P95和最大值只统计成功回应；P95表示成功回应中约95%不超过该值。加速器存在时，区服入口直连探测不一定走游戏加速线路。\n",summary.next_step,RULES);
    let _=writeln!(text,"## 目标统计（毫秒）\n\n|目标 / 方式|成功/尝试|超时|其他错误|基线 P50|P95|最大成功延迟|实际异常阈值|\n|---|---:|---:|---:|---:|---:|---:|---:|");
    for s in report.stats.iter().take(100) {
        let threshold = if s.method != "ICMP" {
            "不适用".into()
        } else if !s.assessable {
            "样本不足，未启用".into()
        } else {
            number(s.p50.map(|v| analysis::delay_threshold(&s.role, v)))
        };
        let _ = writeln!(
            text,
            "|{} / {} / {}|{}/{}|{}|{}|{}|{}|{}|{}|",
            cell(&s.label),
            cell(&s.target),
            s.method,
            s.success,
            s.sent,
            s.timeouts,
            s.errors,
            number(s.p50),
            number(s.p95),
            number(s.max),
            threshold
        );
    }
    if report.stats.len() > 100 {
        let _ = writeln!(
            text,
            "\n目标统计共 {} 项，此处显示前100项，完整数据见 report.json。",
            report.stats.len()
        );
    }
    let _ = writeln!(text, "\n## 异常与证据\n");
    let mut findings = report.findings.iter().collect::<Vec<_>>();
    findings.sort_by_key(|f| f.level != "warning");
    for f in findings.iter().take(20) {
        let _ = writeln!(
            text,
            "### {}\n\n- 时间：{}–{}（开始后 {:.1}–{:.1} 秒）\n- 判断把握：{}",
            cell(&f.title),
            at(report, f.start),
            at(report, f.end),
            f.start,
            f.end,
            cell(&f.confidence)
        );
        for e in &f.evidence {
            let _ = writeln!(text, "- {}", cell(e));
        }
        let _ = writeln!(text, "- 建议：{}\n", cell(&f.suggestion));
    }
    if findings.len() > 20 {
        let _ = writeln!(
            text,
            "共 {} 条发现，本文件优先显示前20条，完整证据见 report.json。",
            findings.len()
        );
    }
    let _=writeln!(text,"\n## 地址变化与卡顿标记\n\n地址变化是观测时间，受采样和事件交付延迟影响。中转上游可能包含控制连接，无法确认每条都承载游戏。\n\n|观测时间|变化|来源|协议 / 地址|前后10秒卡顿标记|\n|---|---|---|---|---:|");
    for c in report.connection_changes.iter().rev().take(60).rev() {
        let _ = writeln!(
            text,
            "|{}|{}|{}|{} {}:{}|{}|",
            at(report, c.elapsed),
            match c.kind.as_str() {
                "first_seen" => "首次看到",
                "resumed" => "再次出现",
                "left_table" => "连接表不再显示",
                _ => "近期无收发",
            },
            if c.endpoint.source == "relay" {
                "中转上游候选"
            } else {
                "游戏连接"
            },
            c.endpoint.protocol,
            cell(&c.endpoint.ip),
            c.endpoint.port,
            report
                .events
                .iter()
                .filter(|e| e.kind == "marker" && (e.elapsed - c.elapsed).abs() <= 10.0)
                .count()
        );
    }
    if report.connection_changes.is_empty() {
        let _ = writeln!(
            text,
            "\n没有可提供的地址变化记录；旧版未采集时不能推断为没有切服。"
        );
    } else {
        let _ = writeln!(
            text,
            "\n共 {} 条变化，此处显示最近最多60条，完整列表见 report.json。",
            report.connection_changes.len()
        );
    }
    for e in report
        .events
        .iter()
        .filter(|e| e.kind == "marker" || e.kind == "gap")
        .take(40)
    {
        let _ = writeln!(text, "- {}：{}", at(report, e.elapsed), cell(&e.message));
    }
    let _=writeln!(text,"\n## 数据覆盖与限制\n\n- {}\n- 解析网络事件：{}；系统丢事件计数：{}；未解析事件：{}；端点解析错误：{}。\n- 原始日志格式：{}；文件大小：{:.2} MiB。\n",cell(&report.traffic_status.message),report.traffic_status.events,report.traffic_status.lost_events,report.traffic_status.unparsed_events,report.traffic_status.endpoint_errors,if report.log_format.is_empty(){"旧版完整快照"}else{&report.log_format},report.raw_log_bytes as f64/1048576.0);
    for s in &report.limitations {
        let _ = writeln!(text, "- {}", cell(s));
    }
    let _=writeln!(text,"\n## 给分析者的问题\n\n请基于以上数据说明：哪些异常能确认，哪些只是可能；卡顿是否与地址变化在时间上重合；更值得检查本机、局域网、外网线路、加速器候选还是游戏端；还缺少什么证据；下一步如何做对照测试。不要仅凭一次重合或某一跳不回应下确定结论。\n\n本文件是便于交流的摘要。更完整的结构化结果在 report.json；压缩原始日志用于深入复核，通常不需要整份上传给聊天助手。\n");
    text
}
pub fn refresh_legacy_evidence(report: &mut Report, dir: &Path) -> Result<(), String> {
    analysis::fill_thresholds(report);
    if report.schema_version >= 3 {
        return Ok(());
    }
    let file = match File::open(dir.join("samples.jsonl")) {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    report.raw_log_bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut probes = vec![];
    let mut activities = vec![];
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if !(line.contains("\"type\":\"probe\"") || line.contains("\"type\":\"activities\"")) {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if value["type"] == "probe" {
            probes.push(
                serde_json::from_value::<Probe>(value["data"].clone())
                    .map_err(|e| e.to_string())?,
            );
        } else if value["type"] == "activities" {
            activities = serde_json::from_value::<Vec<analysis::Activity>>(value["data"].clone())
                .map_err(|e| e.to_string())?;
        }
    }
    if !probes.is_empty() {
        report.findings = analysis::findings(
            &probes,
            &activities,
            &report.events,
            report.duration_seconds,
        );
        report
            .findings
            .extend(analysis::transition_findings(report, &probes));
    }
    Ok(())
}
