/** Scoped DOM bridge between a portalled information panel and its mounted transcript owner. */
export const SESSION_CHILD_NAVIGATION = "ora-session-child-navigation";

/** Requests an existing display row; only its transcript can acknowledge and mount it. */
export function locateSessionChild(sessionId: string, rowKey: string): boolean {
  const transcript = Array.from(
    document.querySelectorAll<HTMLElement>("[data-session-transcript]"),
  ).find((node) => node.dataset.sessionTranscript === sessionId);
  if (!transcript) return false;
  const request = new CustomEvent<string>(SESSION_CHILD_NAVIGATION, {
    detail: rowKey,
    cancelable: true,
  });
  transcript.dispatchEvent(request);
  return request.defaultPrevented;
}
