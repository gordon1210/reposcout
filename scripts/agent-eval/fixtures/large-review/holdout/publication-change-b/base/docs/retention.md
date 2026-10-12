# Retention, history, and audit

A retention rule belongs to one collection and states how many recent revisions of each document
to keep. A rule must be configured before previewing or applying retention. The preview returns
candidate revision IDs, their document IDs, and removed character totals. Applying recomputes the
same eligibility under the current state and removes only eligible revisions atomically.

Current revision pointers are protected regardless of the recent count. Approvals, reviews,
release entries, and comments also preserve every source revision they reference. This rule is
deliberately conservative: withdrawal and closed reviews do not automatically erase provenance.
An old unreferenced intermediate working draft can be removed. History then lists retained
revisions, so parent links can refer to an earlier retained or removed predecessor; the history
view is not a promise of a contiguous edit graph after retention.

The audit journal uses monotonically increasing operation sequence numbers. It records successful
changes and public share reads. It contains identity/provenance details rather than storing a full
copy of every source body. Queries support a sequence cursor, action, actor, and target filters.
JSON and CSV export describe the same selected events. Read-only diagnostic operations do not
create events simply because an administrator looked at them.

Workspace statistics count visible content only. The administrator integrity check verifies
protected revision references, orphan revisions, and completed job artifacts. A healthy result is
limited to these checked references; it is not a proof of business correctness, comprehensive access
control, or an assertion that every application test ran.
