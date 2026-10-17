//! Usage & Spend dashboard preferences (Mac `costUsageEnabled`, dashboard
//! period, hidden sources).

/// Hidden source ids are stored sorted and unique. Ids are trimmed and
/// lowercased; empty entries are dropped. Unknown ids are kept, so a source
/// that is missing today stays hidden when it comes back.
pub fn normalize_spend_dashboard_hidden_source_ids(
    ids: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut ids: Vec<String> = ids
        .into_iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_source_ids_are_sorted_unique_and_trimmed() {
        let ids = ["codex", " Claude ", "", "codex", "pi"]
            .into_iter()
            .map(String::from);
        assert_eq!(
            normalize_spend_dashboard_hidden_source_ids(ids),
            vec!["claude", "codex", "pi"]
        );
    }
}
