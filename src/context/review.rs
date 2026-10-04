use crate::model::{ReviewContextFile, ReviewContextTotals};

/// Select unique, already-costed file sides in evidence order. This module
/// neither reads sources nor chooses a tokenizer or an agent assignment.
pub(crate) fn select(
    files: &mut [ReviewContextFile],
    tokens: usize,
    max_files: usize,
    totals: &mut ReviewContextTotals,
) {
    for file in files {
        let Some(cost) = file.tokens else {
            file.selection = "unavailable".into();
            continue;
        };
        let reason = if totals.selected_files >= max_files {
            "file-limit"
        } else if cost > tokens.saturating_sub(totals.selected_tokens) {
            "token-budget"
        } else {
            "selected"
        };
        file.selection = reason.into();
        if reason == "selected" {
            totals.selected_files += 1;
            totals.selected_tokens = totals.selected_tokens.saturating_add(cost);
        } else {
            totals.selection_omitted_files += 1;
            totals.selection_omitted_tokens = totals.selection_omitted_tokens.saturating_add(cost);
        }
    }
}
