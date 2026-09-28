/** Joins a selector array into its dotted text form for the editor input. */
export function selectorToText(selector: string[]): string {
  return selector.join(".");
}

/** Splits the dotted selector text into `[nodeId, root, ...nested]` parts. */
export function textToSelector(text: string): string[] {
  return text
    .split(".")
    .map((part) => part.trim())
    .filter((part) => part !== "");
}
