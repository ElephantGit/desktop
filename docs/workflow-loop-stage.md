# Loop history on the stage

English | [中文](workflow-loop-stage.zh.md)

The Agent inspector's outcomes section shows the selected execution's completed text output alongside file changes and artifacts. Text comes directly from that node execution; absence of file artifacts does not imply absence of results. Streaming text stays out of this section, and selecting another round does not retain the previous round's output.

Condition cards display IF / ELSE IF / ELSE rules, evaluation status, and errors instead of Agent instructions, output placeholders, or conversation hints. Round history exposes the scheduler's committed condition decisions per scope. The card highlights the selected IF / ELSE IF / ELSE branch and names it in the decision result. Missing records remain explicitly unavailable; another round or downstream state is never used as a substitute.

Loop navigation uses a wrapping node grid instead of horizontally nested conditional groups. Recorded executions appear by default; unexecuted members remain inspectable in a disclosure. The grid follows definition structure, not exact chronological execution order. Round buttons show a round number and status mark, with the selected round's detailed outcome displayed separately to avoid repeating long labels.

The outer execution path groups each Loop into one container, alongside ordinary nodes and Iteration containers. Clicking a Loop opens its first persisted round and a round overview. The round buttons show live status; selecting a round changes its member path, node outputs, inspector, and conversation session together. Member selection is retained across round changes; a member absent from the selected round shows as not executed and never borrows another round's session or output.

The stage shows only completed node outputs. Streaming text stays in the existing conversation dock opened from the Agent card. New rounds and background completion update statuses without moving a user's history selection. Entering the Loop through its outer path button resets selection to round one. Loop round numbers are already one-based in the persisted contract; Iteration retains its existing numbering adapter.

The round overview lists each member's status and committed result. The separate **Loop results** action shows the parent's committed exports, error, and recorded termination reason. The workflow result act shows the workflow's committed final output. Running or failed loops do not present partial values as successful exports. Until more detailed metadata is exposed, missing member executions use the neutral “not run this round” label; the UI does not guess why a particular branch was absent. Unscoped artifact lists are not attached to Loop member history; per-execution file changes remain available in the inspector.

Existing persisted round records and run invalidation/refresh are reused. No loop scheduling behavior or storage layout changes are required. Tests cover container grouping, one-based round selection, live refresh preserving selection, returning to round one, hiding partial output, missing-member isolation, and switching the actual chat session between rounds.
