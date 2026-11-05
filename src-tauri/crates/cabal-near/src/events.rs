//! Reads what the verifier logged, so a settlement can be counted from the chain.

use serde::Deserialize;

#[derive(Deserialize)]
struct Event {
    standard: String,
    event: String,
    data: Vec<EventEntry>,
}

#[derive(Deserialize)]
struct EventEntry {
    account_id: Option<String>,
}

/// How many `token_diff` intents in these logs `account` signed.
///
/// A diff is the trade itself. A fee `transfer` in the same payload logs its own
/// event and is not a second settlement. Lines that are not verifier events are
/// skipped, since a transaction logs other things too.
#[must_use]
pub fn settlements_for(account: &str, logs: &[String]) -> u32 {
    let signed = logs
        .iter()
        .filter_map(|log| log.strip_prefix("EVENT_JSON:"))
        .filter_map(|json| serde_json::from_str::<Event>(json).ok())
        .filter(|event| event.standard == "dip4" && event.event == "token_diff")
        .flat_map(|event| event.data)
        .filter(|entry| entry.account_id.as_deref() == Some(account))
        .count();
    u32::try_from(signed).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SELLER: &str = "b6336d522196d849252c5dbef77215710abf748ac53afc63ece79378d188b1ff";
    const BUYER: &str = "b66ef074474ba8b60f630afe03dafce202086ae2e568e653f64b9dd367d8c0f3";

    /// A real pair with fee transfers, from testnet.
    fn captured() -> Vec<String> {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/verifier_events.json")).unwrap();
        serde_json::from_value(fixture["logs"].clone()).unwrap()
    }

    #[test]
    fn counts_only_diffs_signed_by_account() {
        assert_eq!(settlements_for(SELLER, &captured()), 1);
        assert_eq!(settlements_for(BUYER, &captured()), 1);
        assert_eq!(settlements_for("nobody.testnet", &captured()), 0);
    }

    #[test]
    fn other_log_lines_count_for_nothing() {
        let logs = [
            "plain text".to_owned(),
            "EVENT_JSON:not json".to_owned(),
            format!(
                r#"EVENT_JSON:{{"standard":"other","event":"token_diff","data":[{{"account_id":"{SELLER}"}}]}}"#
            ),
        ];
        assert_eq!(settlements_for(SELLER, &logs), 0);
    }
}
