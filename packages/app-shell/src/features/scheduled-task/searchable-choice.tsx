import { useRef, useState } from "react";
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@ora/ui";
import { IconSearch } from "@tabler/icons-react";

export interface SearchableChoiceOption {
  label: string;
  value: string;
  keywords?: string;
}

interface SearchableChoiceProps {
  id: string;
  value: string;
  options: SearchableChoiceOption[];
  disabled?: boolean;
  placeholder: string;
  emptyLabel: string;
  onValueChange: (value: string) => void;
}

function optionMatchesQuery(
  option: SearchableChoiceOption,
  query: string,
  selectedLabel: string,
): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "" || needle === selectedLabel.trim().toLowerCase()) {
    return true;
  }
  return [option.label, option.keywords, option.value].some((part) =>
    part?.toLowerCase().includes(needle),
  );
}

function inputOverflows(root: HTMLElement | null): boolean {
  const input = root?.querySelector("input");
  if (!input) return false;
  return input.scrollWidth - input.clientWidth > 1;
}

/** Searchable single-select: the field is the filter, the list matches its width. */
export function SearchableChoice({
  id,
  value,
  options,
  disabled = false,
  placeholder,
  emptyLabel,
  onValueChange,
}: SearchableChoiceProps) {
  const fieldRef = useRef<HTMLDivElement>(null);
  const menuOpenRef = useRef(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const selected =
    options.find((option) => option.value === value) ??
    (value === "" ? null : { label: value, value });

  return (
    <Popover
      open={previewOpen && !menuOpen}
      onOpenChange={(next, details) => {
        if (details.reason === "trigger-press") {
          details.cancel();
          return;
        }
        if (
          next &&
          (menuOpenRef.current || !inputOverflows(fieldRef.current))
        ) {
          details.cancel();
          return;
        }
        setPreviewOpen(next);
      }}
    >
      <PopoverTrigger
        nativeButton={false}
        openOnHover
        delay={300}
        render={
          <div
            ref={fieldRef}
            role="presentation"
            tabIndex={-1}
            className="min-w-0 w-full"
          />
        }
      >
        <Combobox
          items={options}
          value={selected}
          disabled={disabled}
          isItemEqualToValue={(item, current) => item.value === current.value}
          itemToStringLabel={(item) => item.label}
          filter={(item, query) =>
            optionMatchesQuery(item, query, selected?.label ?? "")
          }
          onOpenChange={(open) => {
            menuOpenRef.current = open;
            setMenuOpen(open);
            if (open) setPreviewOpen(false);
          }}
          onValueChange={(next, details) => {
            if (details.reason === "input-clear") {
              details.cancel();
              return;
            }
            onValueChange(next?.value ?? "");
          }}
        >
          <ComboboxInput
            id={id}
            showTrigger={false}
            placeholder={placeholder}
            className="w-full min-w-0 bg-transparent font-normal shadow-none [&_[data-slot=input-group-addon]]:hidden [&_[data-slot=input-group-control]]:pr-8! [&_[data-slot=input-group-control]]:font-normal"
            onFocus={(event) => {
              event.currentTarget.select();
            }}
          >
            <span className="pointer-events-none absolute inset-y-0 right-2 flex items-center text-muted-foreground">
              <IconSearch className="size-4" aria-hidden="true" />
            </span>
          </ComboboxInput>
          <ComboboxContent
            align="start"
            sideOffset={4}
            className="w-(--anchor-width)! min-w-(--anchor-width)! max-w-(--anchor-width)! p-1"
          >
            <ComboboxEmpty>{emptyLabel}</ComboboxEmpty>
            <ComboboxList className="max-h-52 overflow-y-auto p-0">
              {(option: SearchableChoiceOption) => (
                <ComboboxItem
                  key={option.value}
                  value={option}
                  className="items-start py-1.5 pr-2 pl-2 leading-5 [&>span.absolute]:hidden"
                >
                  <span className="min-w-0 flex-1 text-left wrap-break-word whitespace-normal">
                    {option.label}
                  </span>
                </ComboboxItem>
              )}
            </ComboboxList>
          </ComboboxContent>
        </Combobox>
      </PopoverTrigger>
      {selected ? (
        <PopoverContent
          side="top"
          align="start"
          className="w-auto max-w-sm text-left wrap-break-word whitespace-normal"
        >
          {selected.label}
        </PopoverContent>
      ) : null}
    </Popover>
  );
}
