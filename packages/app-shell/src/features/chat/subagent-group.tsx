import { useState } from "react";
import { IconChevronDown, IconUsersGroup } from "@tabler/icons-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@ora/ui";
import { useTranslation } from "react-i18next";
import type { ChatTurnItem, ChatTurnStatus } from "@ora/chat";
import type { SubagentGroupItem } from "./subagent-items";
import { MessageBubble } from "./message-bubble";
import { MarkdownMessage } from "./markdown-message";
import { ToolCallBlock } from "./tool-call-block";
import { ContentBlock } from "./content-block";
import { PlanBlock } from "./plan-block";

/** Presents a worker's own timeline without interpreting a launch tool as its lifecycle. */
export function SubagentGroup({
  group,
  turnStatus,
  userName,
}: {
  group: SubagentGroupItem;
  turnStatus: ChatTurnStatus;
  userName: string;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const name = group.title ?? t("chat.subagentUnnamed", { index: group.index });
  const label = t("chat.subagentTitle", { name });
  const status =
    turnStatus === "streaming"
      ? "active"
      : turnStatus === "completed"
        ? "ended"
        : "stopped";
  return (
    <section
      data-subagent-group={group.sourceId}
      aria-label={label}
      className="rounded-xl border border-border/80 bg-muted/10"
    >
      <Collapsible open={open} onOpenChange={setOpen}>
        <CollapsibleTrigger className="flex min-h-12 w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left hover:bg-muted/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <IconUsersGroup className="size-4 shrink-0 text-violet-500" />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">{label}</span>
            <span className="block text-[11px] text-muted-foreground">
              {t("chat.subagentCounts", {
                messages: group.items.filter(
                  (i) => i.kind === "message" || i.kind === "content",
                ).length,
                thoughts: group.items.filter((i) => i.kind === "thought")
                  .length,
                tools: group.items.filter((i) => i.kind === "toolCall").length,
              })}
            </span>
          </span>
          <span className="text-[11px] text-muted-foreground">
            {t("chat.subagentStateUnknown")}
            <span aria-hidden="true"> · </span>
            <span data-subagent-status>{t(`chat.subagentTurn.${status}`)}</span>
          </span>
          <IconChevronDown
            className={`size-3.5 text-muted-foreground transition-transform ${open ? "rotate-180" : ""}`}
          />
        </CollapsibleTrigger>
        <CollapsibleContent>
          <div className="space-y-3 border-t border-border/60 px-3 py-3">
            {group.relationshipUnknown && (
              <p className="text-xs text-muted-foreground">
                {t("chat.subagentRelationshipUnknown")}
              </p>
            )}
            {group.timeline.map((item, index) =>
              item.kind === "subagentGroup" ? (
                <SubagentGroup
                  key={item.id}
                  group={item}
                  turnStatus={turnStatus}
                  userName={userName}
                />
              ) : (
                <div
                  key={item.id}
                  data-subagent-item={item.kind}
                  data-child-session={group.sourceId}
                >
                  <WorkerItem
                    item={item}
                    userName={userName}
                    streaming={
                      turnStatus === "streaming" &&
                      index === group.timeline.length - 1
                    }
                  />
                </div>
              ),
            )}
            <p className="text-[11px] text-muted-foreground">
              {t("chat.subagentHistoryLimit")}
            </p>
          </div>
        </CollapsibleContent>
      </Collapsible>
    </section>
  );
}

function WorkerItem({
  item,
  userName,
  streaming,
}: {
  item: ChatTurnItem;
  userName: string;
  streaming: boolean;
}) {
  const { t } = useTranslation();
  switch (item.kind) {
    case "message":
      return (
        <MessageBubble
          message={item}
          userName={userName}
          embeddedAssistant
          streaming={streaming}
        />
      );
    case "thought":
      return (
        <details className="rounded-md bg-muted/25 px-3 py-2 text-xs text-muted-foreground">
          <summary className="cursor-pointer font-medium">
            {t("chat.subagentThought")}
          </summary>
          <div className="pt-2">
            <MarkdownMessage content={item.content} streaming={streaming} />
          </div>
        </details>
      );
    case "toolCall":
      return <ToolCallBlock tool={item} />;
    case "content":
      return <ContentBlock content={item.content} />;
    case "plan":
      return <PlanBlock plan={item} />;
  }
}
