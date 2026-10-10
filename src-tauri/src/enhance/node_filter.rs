use clash_verge_logging::{Type, logging};
use regex::Regex;
use serde_yaml_ng::{Mapping, Value};
use std::collections::HashSet;

const BUILTIN_POLICIES: &[&str] = &["DIRECT", "REJECT", "REJECT-DROP", "PASS", "PASS-RULE"];

/// Restricts inline proxies and provider-backed groups to the user's node-name pattern.
pub fn apply(mut config: Mapping, enabled: bool, pattern: Option<&str>) -> Mapping {
    if !enabled {
        return config;
    }

    let Some(pattern) = pattern.map(str::trim).filter(|value| !value.is_empty()) else {
        return config;
    };

    let regex = match Regex::new(pattern) {
        Ok(regex) => regex,
        Err(error) => {
            logging!(warn, Type::Config, "忽略无效的节点筛选正则表达式: {error}");
            return config;
        }
    };

    let proxy_names = config
        .get("proxies")
        .and_then(Value::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_mapping()?.get("name")?.as_str())
                .map(str::to_owned)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let matching_proxy_names = proxy_names
        .iter()
        .filter(|name| regex.is_match(name))
        .cloned()
        .collect::<HashSet<_>>();

    let group_names = config
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_mapping()?.get("name")?.as_str())
                .map(str::to_owned)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let provider_names = config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .map(|providers| {
            providers
                .keys()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if matching_proxy_names.is_empty() && provider_names.is_empty() && !proxy_names.is_empty() {
        logging!(warn, Type::Config, "节点筛选没有匹配到任何节点，保留原订阅配置");
        return config;
    }

    if let Some(Value::Sequence(proxies)) = config.get_mut("proxies") {
        proxies.retain(|item| {
            item.as_mapping()
                .and_then(|proxy| proxy.get("name"))
                .and_then(Value::as_str)
                .is_none_or(|name| matching_proxy_names.contains(name))
        });
    }

    if let Some(Value::Mapping(providers)) = config.get_mut("proxy-providers") {
        for provider in providers.values_mut() {
            if let Some(provider) = provider.as_mapping_mut() {
                provider.insert("filter".into(), pattern.into());
            }
        }
    }

    let mut allowed_names = matching_proxy_names;
    allowed_names.extend(group_names);
    allowed_names.extend(provider_names);
    allowed_names.extend(BUILTIN_POLICIES.iter().map(|name| (*name).to_owned()));

    if let Some(Value::Sequence(groups)) = config.get_mut("proxy-groups") {
        for group in groups {
            let Some(group_map) = group.as_mapping_mut() else {
                continue;
            };

            let uses_provider = group_map.contains_key("use");
            let includes_all = group_map.get("include-all").and_then(Value::as_bool).unwrap_or(false);

            if includes_all {
                group_map.insert("filter".into(), pattern.into());
            }

            if let Some(Value::Sequence(proxies)) = group_map.get_mut("proxies") {
                proxies.retain(|proxy| proxy.as_str().is_none_or(|name| allowed_names.contains(name)));
                if proxies.is_empty() && !uses_provider && !includes_all {
                    proxies.push("DIRECT".into());
                }
            }
        }
    }

    config
}
