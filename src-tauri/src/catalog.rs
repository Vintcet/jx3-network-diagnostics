use crate::model::{now, Catalog, Endpoint, Server};
use std::{collections::BTreeMap, net::Ipv4Addr, path::Path, time::Duration};

const URL: &str = "https://jx3comm.xoyocdn.com/jx3hd/zhcn_hd/serverlist/serverlist.ini";
pub fn parse(bytes: &[u8]) -> Result<Vec<Server>, String> {
    let (text, _, bad) = encoding_rs::GBK.decode(bytes);
    if bad {
        return Err("服务器目录编码无效".into());
    }
    let mut groups = BTreeMap::<String, Server>::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<_> = line.trim_end_matches('\r').split('\t').collect();
        if f.len() < 12 {
            return Err("服务器目录列数不足".into());
        }
        let ip = f[3].parse::<Ipv4Addr>().map_err(|_| "服务器 IP 无效")?;
        if !public_v4(ip) {
            return Err("服务器目录含非公网地址".into());
        }
        let port = f[4]
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or("服务器端口无效")?;
        if f[10].is_empty() || f[11].is_empty() {
            return Err("区服名称缺失".into());
        }
        let id = format!("{}:{}:{}", f[9], f[11], f[10]);
        let entry = groups.entry(id.clone()).or_insert_with(|| Server {
            id,
            area: f[11].into(),
            name: f[10].into(),
            aliases: vec![],
            endpoints: vec![],
        });
        if !entry.aliases.iter().any(|a| a == f[1]) {
            entry.aliases.push(f[1].into());
        }
        if !entry
            .endpoints
            .iter()
            .any(|e| e.ip == f[3] && e.port == port)
        {
            entry.endpoints.push(Endpoint {
                ip: ip.to_string(),
                port,
            });
        }
    }
    if groups.is_empty() {
        return Err("服务器目录为空".into());
    }
    Ok(groups.into_values().collect())
}
pub fn public_v4(ip: Ipv4Addr) -> bool {
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && !ip.is_documentation()
        && ip.octets()[0] < 240
        && ip.octets()[0] != 0
        && !(ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
}
pub fn load(root: &Path, refresh: bool) -> Catalog {
    let cache = root.join("catalog.json");
    let mut warning = None;
    if refresh {
        let result = (|| -> Result<Catalog, String> {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| e.to_string())?;
            let response = client
                .get(URL)
                .send()
                .and_then(|r| r.error_for_status())
                .map_err(|e| e.to_string())?;
            if response.content_length().unwrap_or(0) > 2_000_000 {
                return Err("目录过大".into());
            }
            use std::io::Read;
            let mut bytes = vec![];
            response
                .take(2_000_001)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 2_000_000 {
                return Err("目录过大".into());
            }
            let data = Catalog {
                servers: parse(&bytes)?,
                source: "官方在线目录".into(),
                fetched_at: now(),
                warning: None,
            };
            std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
            std::fs::write(&cache, serde_json::to_vec_pretty(&data).unwrap())
                .map_err(|e| e.to_string())?;
            Ok(data)
        })();
        match result {
            Ok(c) => return c,
            Err(e) => warning = Some(format!("目录更新失败，已使用本地目录：{e}")),
        }
    }
    if let Ok(bytes) = std::fs::read(cache) {
        if let Ok(mut c) = serde_json::from_slice::<Catalog>(&bytes) {
            if !c.servers.is_empty()
                && c.servers.iter().all(|s| {
                    !s.endpoints.is_empty()
                        && s.endpoints
                            .iter()
                            .all(|e| e.ip.parse().is_ok_and(public_v4) && e.port > 0)
                })
            {
                c.source = "本地缓存".into();
                c.warning = warning;
                return c;
            }
        }
    }
    Catalog {
        servers: parse(include_bytes!("../data/serverlist.ini")).expect("内置目录应有效"),
        source: "内置目录".into(),
        fetched_at: "2026-10-01".into(),
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_catalog_groups_aliases() {
        let s = parse(include_bytes!("../data/serverlist.ini")).unwrap();
        let target = s.iter().find(|s| s.name == "绝代天骄").unwrap();
        assert!(target.aliases.contains(&"风骨霸刀".into()));
        assert_eq!(target.endpoints[0].ip, "109.244.61.178");
    }
    #[test]
    fn keeps_multiple_endpoints() {
        let text = "a\told\t0\t8.8.8.8\t3724\ta\told\t0\t0\tz\tnew\tarea\na\tother\t0\t1.1.1.1\t3724\ta\tother\t0\t0\tz\tnew\tarea";
        let s = parse(text.as_bytes()).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].endpoints.len(), 2);
    }
    #[test]
    fn rejects_invalid_catalog() {
        assert!(parse(b"bad").is_err());
        assert!(parse(b"a\tb\t0\t127.0.0.1\t1\ta\tb\t0\t0\tz\tx\ty").is_err());
    }
}
