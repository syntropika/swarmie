const panelType = "application/x-swarmie-panel";
export function startPanelDrag(transfer: DataTransfer, panel: "conversation" | "view") {
  transfer.effectAllowed = "move";
  transfer.setData(panelType, panel);
}
export function draggedPanel(transfer: DataTransfer): "conversation" | "view" | null {
  const value = transfer.getData(panelType);
  return value === "conversation" || value === "view" ? value : null;
}
