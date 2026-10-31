/** Display preferences never change the node's history or execution. */
export type ToolDetails = "grouped" | "separate" | "hidden";

export type ToolDetailsPersistence = {
  load: () => unknown;
  save: (value: ToolDetails) => void;
};

export function isToolDetails(value: unknown): value is ToolDetails {
  return value === "grouped" || value === "separate" || value === "hidden";
}

export function loadToolDetails(persistence?: ToolDetailsPersistence): ToolDetails {
  try {
    const value = persistence?.load();
    return isToolDetails(value) ? value : "grouped";
  } catch {
    return "grouped";
  }
}

export function browserToolDetailsPersistence(): ToolDetailsPersistence {
  const key = "swarmie.conversation.tool-details";
  return {
    load: () => localStorage.getItem(key),
    save: (value) => {
      localStorage.setItem(key, value);
    },
  };
}
