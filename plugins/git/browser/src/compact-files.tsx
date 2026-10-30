import { Button, Drawer } from "@swarmie/ui-web";
import { ChevronDown, FileCode2, X } from "lucide-react";
import type { ReactNode } from "react";

import type { Selection } from "./file-selection";
import type { DiffMode } from "./protocol";
import { ChangeFilter, DiffLayout } from "./view-controls";
import type { ChangesFilter } from "./view-controls";

export function CompactFiles({
  pickerOpen,
  setPickerOpen,
  selection,
  mode,
  filter,
  layout,
  onLayoutChange,
  onFilterChange,
  counts,
  stageButton,
  fileList,
}: {
  pickerOpen: boolean;
  setPickerOpen: (open: boolean) => void;
  selection: Selection | null;
  mode: "explore" | "changes";
  filter: ChangesFilter;
  layout: DiffMode;
  onLayoutChange: (layout: DiffMode) => void;
  onFilterChange: (filter: ChangesFilter) => void;
  counts: ReactNode;
  stageButton: ReactNode;
  fileList: ReactNode;
}) {
  const staged = selection?.staged === true ? " · staged" : "";
  return (
    <div data-git-file-controls="" className="shrink-0">
      <Drawer isOpen={pickerOpen} onOpenChange={setPickerOpen}>
        <Button
          data-git-file-picker=""
          variant="tertiary"
          onPress={() => {
            setPickerOpen(true);
          }}
          aria-label={mode === "changes" ? "Choose changed file" : "Choose repository file"}
        >
          <FileCode2 size={16} className="shrink-0 text-muted" />
          <span className="min-w-0 flex-1 truncate text-left">
            {selection?.path ?? "Choose a file"}
            {staged}
          </span>
          {counts}
          <ChevronDown size={16} className="shrink-0 text-muted" />
        </Button>
        <Drawer.Backdrop>
          <Drawer.Content
            placement="bottom"
            data-git-workspace=""
            data-git-compact="true"
            data-git-file-sheet=""
          >
            <Drawer.Dialog className="h-[min(75dvh,36rem)]">
              <Drawer.Handle />
              <Drawer.Header className="flex flex-row items-center justify-between">
                <Drawer.Heading>
                  {mode === "changes" ? "Changed files" : "Repository files"}
                </Drawer.Heading>
                <Button slot="close" variant="tertiary" aria-label="Close file selector">
                  <X size={18} />
                </Button>
              </Drawer.Header>
              {mode === "changes" ? (
                <div className="px-4 pb-2">
                  <ChangeFilter value={filter} onChange={onFilterChange} />
                </div>
              ) : null}
              <Drawer.Body className="min-h-0 flex-1 overflow-hidden">{fileList}</Drawer.Body>
            </Drawer.Dialog>
          </Drawer.Content>
        </Drawer.Backdrop>
      </Drawer>
      <div
        data-git-document-toolbar=""
        className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2"
      >
        {mode === "changes" ? (
          <DiffLayout value={layout} onChange={onLayoutChange} />
        ) : (
          <span className="text-xs text-muted">File preview</span>
        )}
        {stageButton}
      </div>
    </div>
  );
}
