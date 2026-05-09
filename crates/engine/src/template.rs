use regex::Regex;
use serde_json::Value;

pub fn resolve_template(template: &str, context: &Value) -> String {
    // Matches {{ key.path }}
    let re = Regex::new(r"\{\{\s*([\w.]+)\s*\}\}").expect("Invalid hardcoded regex");
    re.replace_all(template, |caps: &regex::Captures| {
        let path = &caps[1];
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = context;

        for part in parts {
            match current.get(part) {
                Some(v) => current = v,
                None => return caps[0].to_string(), // Keep original if not found
            }
        }

        match current {
            Value::String(s) => s.to_string(),
            _ => current.to_string(),
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_template() {
        let template = "Hello {{ user.name }}!";
        let context = serde_json::json!({
            "user": {
                "name": "Alice"
            }
        });
        let result = resolve_template(template, &context);
        assert_eq!(result, "Hello Alice!");
    }
}
