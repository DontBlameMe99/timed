//! String lookup with a single allocation for the common case.
//!
//! `Context::tr` clones the template and allocates a new string for every
//! parameter. Renders ask for the same small set of strings on every event, so
//! that work is measurable. This helper builds the final string in one pass
//! and falls back to the context's own substitution when a parameter value
//! contains a placeholder, which keeps the sequential replacement semantics.

use noctalia_rs::Context;
use serde_json::Value;

fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let mut value = root.get(parts.next()?)?;
    for part in parts {
        value = value.get(part)?;
    }
    Some(value)
}

#[must_use]
pub fn tr(ctx: &Context, key: &str, params: &[(&str, String)]) -> String {
    if params.is_empty() {
        return ctx.tr(key, params);
    }
    let template = lookup(ctx.translations(), key)
        .and_then(Value::as_str)
        .unwrap_or(key);

    if template.find('{').is_none()
        || params.iter().any(|(_, value)| value.contains('{'))
    {
        return ctx.tr(key, params);
    }

    let extra = params.iter().map(|(_, value)| value.len()).sum::<usize>();
    let capacity = template.len().saturating_add(extra);
    let mut out = String::with_capacity(capacity);
    let mut parts = template.split('{');
    out.push_str(parts.next().unwrap_or(""));
    for part in parts {
        let Some((name, rest)) = part.split_once('}') else {
            out.push('{');
            out.push_str(part);
            continue;
        };
        if let Some((_, value)) =
            params.iter().find(|(param, _)| *param == name)
        {
            out.push_str(value);
        } else {
            out.push('{');
            out.push_str(name);
            out.push('}');
        }
        out.push_str(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_context;
    use crate::translations::TRANSLATIONS;

    #[test]
    fn tr_matches_the_context_for_every_template() {
        let ctx = test_context::context(
            "render",
            serde_json::json!({}),
            serde_json::json!({}),
            serde_json::json!({}),
        );
        let params: &[(&str, String)] = &[
            ("duration", "1h 5m".to_owned()),
            ("time", "12:34".to_owned()),
            ("message", "wake up".to_owned()),
            ("count", "3".to_owned()),
        ];
        for (key, _) in TRANSLATIONS {
            assert_eq!(tr(&ctx, key, params), ctx.tr(key, params), "{key}");
        }
    }

    #[test]
    fn tr_preserves_sequential_substitution_for_braced_values() {
        let ctx = test_context::context(
            "render",
            serde_json::json!({}),
            serde_json::json!({}),
            serde_json::json!({}),
        );
        let params: &[(&str, String)] = &[
            ("duration", "{message}".to_owned()),
            ("message", "wake up".to_owned()),
        ];
        assert_eq!(
            tr(&ctx, "launcher.message", params),
            ctx.tr("launcher.message", params)
        );
    }
}
