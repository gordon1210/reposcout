# Published search

Index rebuild takes one release ID and replaces that release's entries. It uses the same selected
publication content as other external release representations. Each index record retains release,
document, and revision identity along with searchable title, body, tags, and language. Reindexing
an older release does not advance it to a newer draft or a newer release.

The tokenizer normalizes compatibility characters, lowercases English-style, removes punctuation,
and ignores a small explicit set of common words. It is a deliberately small lexical search, not
stemming, semantic embedding, or a language-specific analyzer. Queries use at most twelve unique
terms. Every query term must appear; title matches score above tag matches, which score above body
matches. Ties use stable record identity.

Visibility is applied before ranking. Search can filter by release, language, or exact tag.
Withdrawn releases cannot appear. Results contain a bounded snippet around the first matching
term, source identities, relevance, and a full match count. Search limits are positive and bounded;
an empty or stopword-only query produces no matches.

Saved searches belong to a single actor. They retain only supported query, language, and tag
parameters and execute under the owner's current access. Saving a search does not copy results or
freeze permissions. Another user cannot run or delete it. Document labels are an organizational
feature with their own queries and are distinct from content revision tags in the search index.
