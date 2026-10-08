# Collection access

The initialized users demonstrate four account roles. Administrators manage the whole workspace.
Ordinary accounts need memberships to access private collections. Membership roles can be owner,
editor, reviewer, or reader. Owners may manage, edit, review, and publish within their collection;
editors author content, reviewers make independent decisions and publish, and readers inspect.
The independent-author rule still applies even if a membership includes review capability.

Membership resolution walks from the selected collection toward the root and uses the first
explicit membership. A child membership may narrow or broaden inherited access. Removing a child
membership restores the inherited behavior; it does not insert an explicit denial. Collections
cannot be moved beneath themselves or their descendants. A move validates management rights at
both ends and rejects a duplicate sibling slug.

Public collections allow read access to active accounts without a membership. They do not grant
edit permission. Private collection listings and document listings omit invisible content; direct
reads fail with a domain error. Moving a document requires edit rights in both collections and may
change who can read it. Explicit revision reads must belong to that document.

Shares are release-specific capabilities. The API uses a synthetic deterministic token for this
in-memory demonstration; it is not a production token-generation recommendation. A valid share
delivers the selected release while available and increments its view count. Revocation, exhaustion,
or release withdrawal prevents delivery. Share access is the only route that does not require a
workspace actor. It does not grant collection browsing or draft previews.

Disabled accounts cannot call ordinary actions. User administration prevents the active
administrator from disabling their own account. Audit and integrity reports require workspace
administration because they can describe objects across private collections.
