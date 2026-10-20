import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import type { Edge } from "@xyflow/react";

const CANVAS_INSERT_HOVER_HOLD_MS = 180;

interface WorkflowEdgeHover {
  hoveredEdgeId: string | null;
  reveal: (edgeId: string) => void;
  concealSoon: () => void;
}

const WorkflowEdgeHoverContext = createContext<WorkflowEdgeHover | null>(null);

const INACTIVE_HOVER: WorkflowEdgeHover = {
  hoveredEdgeId: null,
  reveal: () => {},
  concealSoon: () => {},
};

/** Tracks which canvas edge currently shows the Dify-style midpoint insert control. */
export function WorkflowEdgeHoverProvider({
  children,
}: {
  children: ReactNode;
}) {
  const [hoveredEdgeId, setHoveredEdgeId] = useState<string | null>(null);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (hideTimer.current !== null) {
        clearTimeout(hideTimer.current);
      }
    },
    [],
  );
  const reveal = useCallback((edgeId: string) => {
    if (hideTimer.current !== null) {
      clearTimeout(hideTimer.current);
      hideTimer.current = null;
    }
    setHoveredEdgeId(edgeId);
  }, []);
  const concealSoon = useCallback(() => {
    if (hideTimer.current !== null) {
      clearTimeout(hideTimer.current);
    }
    hideTimer.current = setTimeout(() => {
      setHoveredEdgeId(null);
      hideTimer.current = null;
    }, CANVAS_INSERT_HOVER_HOLD_MS);
  }, []);
  const value = useMemo(
    () => ({ hoveredEdgeId, reveal, concealSoon }),
    [concealSoon, hoveredEdgeId, reveal],
  );
  return (
    <WorkflowEdgeHoverContext.Provider value={value}>
      {children}
    </WorkflowEdgeHoverContext.Provider>
  );
}

/** Reads canvas-edge insert hover. Missing provider keeps the control hidden until selected. */
// eslint-disable-next-line react-refresh/only-export-components
export function useWorkflowEdgeHover(): WorkflowEdgeHover {
  return useContext(WorkflowEdgeHoverContext) ?? INACTIVE_HOVER;
}

/** Binds React Flow edge pointer events to the insert-hover tracker. */
export function WorkflowEdgeHoverBridge({
  children,
}: {
  children: (handlers: {
    onEdgeMouseEnter: (event: MouseEvent, edge: Edge) => void;
    onEdgeMouseLeave: (event: MouseEvent, edge: Edge) => void;
  }) => ReactNode;
}) {
  const { reveal, concealSoon } = useWorkflowEdgeHover();
  const onEdgeMouseEnter = useCallback(
    (_event: MouseEvent, edge: Edge) => {
      reveal(edge.id);
    },
    [reveal],
  );
  const onEdgeMouseLeave = useCallback(() => {
    concealSoon();
  }, [concealSoon]);
  return children({ onEdgeMouseEnter, onEdgeMouseLeave });
}
