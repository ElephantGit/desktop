# Exit loop node

English | [中文](workflow-loop-exit.zh.md)

Add **Exit loop** from a loop member’s output-port plus menu. It belongs to that Loop, accepts incoming edges, and has no output port. Use a Condition before it to decide whether to exit. Multiple branches may have their own exit nodes. Root workflows and Iteration regions cannot contain this node.

Reaching an exit stops the owning Loop, not the outer workflow. Existing loop output bindings export values from the current committed round. The exit does not read next-round feedback or evaluate the normal end conditions. A missing output binding value fails explicitly; previous-round node outputs are never substituted. Loop variables still refer to their committed carried values.

End conditions can be removed entirely to use exit nodes alone. Without a reached exit or a satisfied end condition, the maximum round limit still fails the Loop. Existing workflows retain their behavior. No filesystem or database schema migration is required.

## Execution and recovery

The scoped scheduler prioritizes ready exit nodes before dispatching siblings in the same wave. An atomic `RequestExit` transaction records the exit node execution, timestamp, and frozen output result in the round state, completes the exit node, and cancels active sibling rows. Already completed nodes retain their output and session history; unstarted branches are not dispatched. Multiple reachable exits choose the first in the graph’s deterministic ready order.

The session executor stops only cancelled sessions in this scope and waits for their drivers, including drivers that had not bound a session when the exit was requested. Late callbacks and attachments are rejected by the existing terminal-row guards. Cleanup runs outside the run gate. A guarded acknowledgement publishes the frozen outputs and completes the Loop before the outer graph continues. Cleanup failure or a 30-second timeout fails the run; it does not report a successful exit. External side effects are not rolled back.

The durable request is replayed on startup. The boot sweep preserves a Loop waiting for exit cleanup. Duplicate acknowledgements and acknowledgements after cancellation cannot revive a completed scope. Parent history records `stop_reason: loop_exit` after a successful explicit exit. The round record also retains the triggering node execution and request time for later history views.

## Verification

Production SQLite tests cover conditional reachability, output publication without feedback, missing output, parallel callback fencing, duplicate acknowledgement, cancellation, and restart. A full Backend reopen test exercises recovery and the production session-cleanup adapter. Editor tests cover menu scope, automatic ownership and connection, terminal ports, persistence, and removing the last end condition. Import and connection validation reject invalid exit placement and outgoing edges.
