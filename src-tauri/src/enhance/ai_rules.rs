use serde_yaml_ng::{Mapping, Value};

// Category names must match the bundled MetaCubeX/meta-rules-dat GeoSite database.
const SERVICES: &[(&str, &str)] = &[
    ("openai", "AI ChatGPT"),
    ("anthropic", "AI Claude"),
    ("xai", "AI Grok"),
    ("google-gemini", "AI Gemini"),
    ("perplexity", "AI Perplexity"),
];

pub fn apply(mut config: Mapping) -> Mapping {
    let has_proxies = config
        .get("proxies")
        .and_then(Value::as_sequence)
        .is_some_and(|items| !items.is_empty());
    let has_providers = config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .is_some_and(|items| !items.is_empty());
    if !has_proxies && !has_providers {
        return config;
    }

    let mut groups = config
        .remove("proxy-groups")
        .and_then(|value| value.as_sequence().cloned())
        .unwrap_or_default();
    let mut rules = config
        .remove("rules")
        .and_then(|value| value.as_sequence().cloned())
        .unwrap_or_default();
    let mut ai_rules = Vec::with_capacity(SERVICES.len());
    for (category, name) in SERVICES {
        if !groups
            .iter()
            .any(|group| group.get("name").and_then(Value::as_str) == Some(name))
        {
            let mut group = Mapping::new();
            group.insert("name".into(), (*name).into());
            group.insert("type".into(), "select".into());
            group.insert("include-all".into(), true.into());
            groups.push(Value::Mapping(group));
        }
        ai_rules.push(Value::from(format!("GEOSITE,{category},{name}")));
    }
    rules.retain(|rule| !ai_rules.contains(rule));
    ai_rules.extend(rules);
    config.insert("proxy-groups".into(), Value::Sequence(groups));
    config.insert("rules".into(), Value::Sequence(ai_rules));
    config
}

#[allow(clippy::expect_used, reason = "tests assert by panicking")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_routes_precede_subscription_rules_and_keep_region_filtering() {
        let config: Mapping = serde_yaml_ng::from_str(
            "proxies:\n\
               - {name: Japan, type: socks5, server: 127.0.0.1, port: 9}\n\
               - {name: Hong Kong, type: socks5, server: 127.0.0.1, port: 9}\n\
             proxy-groups: [{name: Original, type: select, proxies: [Japan]}]\n\
             rules: ['DOMAIN-SUFFIX,openai.com,DIRECT', 'MATCH,Original']",
        )
        .expect("test fixture should be valid");
        let config = apply(config);
        assert_eq!(apply(config.clone()), config);
        let config = crate::enhance::node_filter::apply(config, true, Some("Japan"));
        let config = crate::enhance::cleanup_proxy_groups(config);
        let rules = config["rules"].as_sequence().expect("test fixture should be valid");
        assert_eq!(rules.len(), SERVICES.len() + 2);
        assert_eq!(rules[0], Value::from("GEOSITE,openai,AI ChatGPT"));
        assert_eq!(rules[SERVICES.len()], Value::from("DOMAIN-SUFFIX,openai.com,DIRECT"));
        assert_eq!(rules.last(), Some(&Value::from("MATCH,Original")));
        assert_eq!(config["proxies"].as_sequence().expect("expected a sequence").len(), 1);
        for group in &config["proxy-groups"].as_sequence().expect("expected a sequence")[1..] {
            assert_eq!(group["include-all"], Value::Bool(true));
            assert_eq!(group["filter"], Value::from("Japan"));
        }
    }

    #[test]
    fn provider_only_profiles_get_ai_groups_without_replacing_existing_groups() {
        let config: Mapping = serde_yaml_ng::from_str(
            "proxy-providers: {subscription: {type: file, path: ./provider.yaml}}\n\
             proxy-groups: [{name: AI ChatGPT, type: select, use: [subscription]}]\n\
             rules: ['MATCH,DIRECT']",
        )
        .expect("test fixture should be valid");
        let result = apply(config.clone());
        let groups = result["proxy-groups"]
            .as_sequence()
            .expect("test fixture should be valid");
        assert_eq!(groups.len(), SERVICES.len());
        assert_eq!(groups[0], config["proxy-groups"][0]);
        assert_eq!(result["proxy-providers"], config["proxy-providers"]);
        assert_eq!(apply(Mapping::new()), Mapping::new());
    }
}
