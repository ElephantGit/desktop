# Subagent Details

English | [中文](subagent-details.zh.md)

The parent conversation can show read-only OpenCode child-session cards. Expand a card to inspect
its text, thoughts, plans and tool input/output; nested cards follow the explicit parent relationship.
Parent replies remain in the ordinary timeline. The original Stop action controls the parent prompt;
these cards do not add child cancellation, steering or closing.

Child state is unknown unless a supported provider report establishes it. A completed launch tool or
ended parent turn does not establish the child's outcome. An unfinished child tool displays an unknown
outcome when the parent turn ends. Missing or cyclic relationships remain independent readable cards.

Attribution comes from `_meta["opencode/child-session"]`, never from a tool title, ID prefix, or depth
alone. Unsupported or malformed sources remain in the parent timeline. Complete metadata and text
annotations are retained during live batching and replay; distinct sources cannot merge on colliding
or missing message IDs. Incremental tool metadata replaces supplied top-level namespaces, including
explicit null, without recursively patching unknown provider objects.

The [history owner](session-history.md) must persist the source metadata. This UI cannot recover
attribution omitted from older files, and ordinary OpenCode projection may stop when the parent prompt
ends. Cards therefore state that only received content is available and do not promise complete
background history. Live observation diagnostics and historical content are separate concerns.
