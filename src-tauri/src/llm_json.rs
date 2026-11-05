/// Local LLMs (llama2 in particular) don't reliably follow "respond ONLY
/// with JSON" instructions — they often wrap the JSON object in explanatory
/// prose, or precede/follow it with extra text. Parsing the raw response
/// directly makes that prose a parse failure, which callers then treat as
/// "the model found nothing" even when it clearly did. This pulls out just
/// the outermost `{...}` block so parsing succeeds regardless of the wrapper
/// text around it.
pub fn extract_json_object(text: &str) -> &str {
    match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if end > start => &text[start..=end],
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_wrapped_in_prose() {
        let text = "Thank you! Here's the answer:\n{ \"matched_id\": 3, \"reason\": \"mentions gm\" }\nHope that helps!";
        assert_eq!(extract_json_object(text), "{ \"matched_id\": 3, \"reason\": \"mentions gm\" }");
    }
}
