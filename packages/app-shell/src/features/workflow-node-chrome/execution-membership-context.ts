import { createContext, useContext, useSyncExternalStore } from "react";

interface UnusedMembership {
  subscribe: (listener: () => void) => () => void;
  isUnused: (nodeId: string) => boolean;
  replace: (ids: readonly string[]) => boolean;
  emit: () => void;
}

const UnusedMembershipContext = createContext<UnusedMembership | null>(null);

function createUnusedMembership(): UnusedMembership {
  let key = "";
  let ids = new Set<string>();
  const listeners = new Set<() => void>();
  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    isUnused(nodeId) {
      return ids.has(nodeId);
    },
    replace(next) {
      const nextKey = next.join("\0");
      if (nextKey === key) {
        return false;
      }
      key = nextKey;
      ids = new Set(next);
      return true;
    },
    emit() {
      for (const listener of listeners) {
        listener();
      }
    },
  };
}

const emptyMembership = createUnusedMembership();

export function createWorkflowUnusedMembership(): UnusedMembership {
  return createUnusedMembership();
}

/**
 * One card reads only its own unused flag. A new list with the same members
 * does not repaint every card; a changed flag repaints that card alone.
 */
export function useWorkflowNodeUnused(nodeId: string): boolean {
  const membership = useContext(UnusedMembershipContext) ?? emptyMembership;
  return useSyncExternalStore(membership.subscribe, () =>
    membership.isUnused(nodeId),
  );
}

export { UnusedMembershipContext };
