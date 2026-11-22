//! `cargo xtask check-csp`: the production Content Security Policy in
//! `tauri.conf.json` stays strict. It fails when the policy is missing, when a
//! source is `'unsafe-*'` or a bare `*`, when a core directive is missing, or
//! when Tauri's own nonce and hash injection is switched off.

use serde_json::Value;

const REQUIRED: &[&str] = &["default-src", "script-src", "style-src", "object-src"];

pub fn violations(config: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let security = &config["app"]["security"];

    match &security["csp"] {
        Value::Null => found.push(
            "`app.security.csp` is missing or null: the app would run without a CSP".to_owned(),
        ),
        Value::String(policy) => {
            for directive in policy.split(';').map(str::trim).filter(|d| !d.is_empty()) {
                let mut parts = directive.split_whitespace();
                let name = parts.next().unwrap_or_default();
                check_sources(name, parts, &mut found);
            }
        }
        Value::Object(directives) => {
            for (name, sources) in directives {
                let text = match sources {
                    Value::String(text) => text.clone(),
                    Value::Array(items) => items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" "),
                    other => other.to_string(),
                };
                check_sources(name, text.split_whitespace(), &mut found);
            }
            for name in REQUIRED {
                if !directives.contains_key(*name) {
                    found.push(format!("the CSP has no `{name}`"));
                }
            }
        }
        other => found.push(format!(
            "`app.security.csp` has an unexpected shape: {other}"
        )),
    }

    if !matches!(
        security["dangerousDisableAssetCspModification"],
        Value::Null | Value::Bool(false)
    ) {
        found.push(
            "`dangerousDisableAssetCspModification` is set: Tauri would stop adding its nonces and hashes"
                .to_owned(),
        );
    }

    found
}

fn check_sources<'a>(
    directive: &str,
    sources: impl Iterator<Item = &'a str>,
    found: &mut Vec<String>,
) {
    for source in sources {
        let lower = source.to_ascii_lowercase();
        if lower.contains("unsafe-") || lower == "*" {
            found.push(format!("`{directive}` allows {source}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(csp: Value) -> Value {
        json!({ "app": { "security": { "csp": csp } } })
    }

    fn strict() -> Value {
        json!({
            "default-src": "'self'",
            "script-src": "'self'",
            "style-src": "'self'",
            "object-src": "'none'",
            "connect-src": ["ipc:", "http://ipc.localhost"]
        })
    }

    #[test]
    fn a_strict_policy_passes() {
        assert_eq!(violations(&config(strict())), Vec::<String>::new());
        assert_eq!(
            violations(&config(json!(
                "default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'"
            ))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn each_loosening_is_reported() {
        let with = |key: &str, value: Value| {
            let mut csp = strict();
            csp[key] = value;
            config(csp)
        };
        let mut no_script = strict();
        no_script.as_object_mut().map(|m| m.remove("script-src"));
        let mut disabled = config(strict());
        disabled["app"]["security"]["dangerousDisableAssetCspModification"] = json!(true);

        let cases: Vec<(&str, Value, &str)> = vec![
            ("no policy", config(Value::Null), "would run without a CSP"),
            (
                "inline styles",
                with("style-src", json!("'self' 'unsafe-inline'")),
                "`style-src` allows 'unsafe-inline'",
            ),
            (
                "eval",
                with("script-src", json!(["'self'", "'unsafe-eval'"])),
                "`script-src` allows 'unsafe-eval'",
            ),
            (
                "wildcard",
                with("img-src", json!("*")),
                "`img-src` allows *",
            ),
            (
                "string policy",
                config(json!("default-src 'self'; script-src 'unsafe-inline'")),
                "`script-src` allows 'unsafe-inline'",
            ),
            (
                "missing directive",
                config(no_script),
                "the CSP has no `script-src`",
            ),
            (
                "injection disabled",
                disabled,
                "dangerousDisableAssetCspModification",
            ),
        ];
        for (label, input, expected) in cases {
            let found = violations(&input);
            assert!(
                found.iter().any(|v| v.contains(expected)),
                "{label}: expected {expected:?} in {found:?}"
            );
        }
    }
}
