# Export jobs and formats

An export job identifies one release and one renderer. Queueing captures a manifest from the
release entries. A request key belongs to the requesting actor; exact replay returns that actor's
existing job, and an incompatible reuse fails. The job's identity includes the manifest and format.
This identity is an application-level input check, not a cryptographic authorization token.

The one-shot runner executes a pending job synchronously. It validates access and release state,
materializes the manifest's content selection, calls a renderer, and persists a checksum-bearing
artifact. Running a completed job returns its existing artifact. The content does not change if
authors edit documents after that artifact is created. Pending jobs retain the selected release's
meaning even if editing happens before execution.

JSON exports include release metadata and document IDs, revision IDs, title, body, tags, language,
and checksum. Text exports include a readable heading and body followed by revision/checksum
provenance. CSV exports are catalog rows rather than full bodies. CSV cells escape embedded
commas, quotes, and newlines, and prefix spreadsheet formula-like values with a quote. UTF-8 byte
length is measured independently of character count.

Only the job owner or administrator can run, cancel, or inspect a job. A pending job may be
cancelled; cancellation creates no artifact. Listing returns the caller's jobs. A release's
withdrawal prevents new execution, while already completed records remain accessible for audit.

Working-copy previews reuse renderers but have no durable job or artifact. They use a synthetic
preview release label and explicitly current content. A shared renderer does not merge these
selection contracts. Public shares and search indexing use publication selections as described
in the publication guide.
