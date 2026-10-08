# Publication lifecycle

Editors create a document and submit a specific revision to review. The review carries an
author, assignee, and revision ID. Approval creates an approval record; rejecting a review does
not. An approval does not itself publish or alter the document. Editing is allowed while a review
is open, but the existing review continues to address its original source.

A publisher creates a release from one or more approval IDs. The release catalog resolves each
approval into its document and revision identity, checks any supplied document ID, and rejects
duplicate document entries. The stored entry includes a checksum and the approval link for audit.
A release represents a publication decision, not a live query for the newest document text.
Two releases can contain different approved revisions of one document and remain independently
exportable. Review and approval records are therefore retained even if a document gains new drafts.

Publication representations are built by selecting entries from this catalog and carrying the
selection through their content preparation path. Authors do not choose a current revision while
rendering an existing release. Content from another document or an unapproved later draft cannot
stand in for the selected entry merely because its title is similar or its revision ID is newer.
The representation's revision ID and checksum must match its delivered body.

Withdrawal is a publication state change. A withdrawn release no longer has active shares or
search results and cannot produce new export jobs. Pending exports fail on execution after
withdrawal. Completed artifacts remain records of previously delivered bytes. Withdrawing one
release does not withdraw another release containing the same document.

Workspace previews serve a different user need: an authorized reader may inspect the current
working copy while approval is pending. Preview output is labelled and not durable publication.
The ordinary document screen and digest subject lines also follow the working revision. These
features must not silently become publication inputs simply because they share content renderers.
