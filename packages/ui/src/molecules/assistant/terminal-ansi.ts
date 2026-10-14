/** SGR parsing strips terminal control strings without interpreting markup or links. */
export type AnsiSegment = {
  text: string;
  offset: number;
  color?: number | undefined;
  bold: boolean;
  dim: boolean;
};
const ANSI_COLORS = new Set([30, 31, 32, 33, 34, 35, 36, 37, 90, 91, 92, 93, 94, 95, 96, 97]);

const findCsiEnd = (line: string, start: number) => {
  for (let index = start; index < line.length; index += 1) {
    const code = line.codePointAt(index) ?? 0;
    if (code >= 0x40 && code <= 0x7e) {
      return index;
    }
  }
  return line.length;
};

const findStringEnd = (line: string, start: number, bell: boolean) => {
  for (let index = start; index < line.length; index += 1) {
    if (bell && line[index] === "\u0007") {
      return index + 1;
    }
    if (line[index] === "\u001B" && line[index + 1] === "\\") {
      return index + 2;
    }
  }
  return line.length;
};

const applySgr = (value: string, state: Omit<AnsiSegment, "text" | "offset">) => {
  const parts = value.split(";");
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index];
    const code = part === "" ? 0 : Number(part);
    if (code === 38 || code === 48) {
      let skipped = 1;
      if (parts[index + 1] === "2") {
        skipped = 4;
      } else if (parts[index + 1] === "5") {
        skipped = 2;
      }
      index += skipped;
    } else if (code === 0) {
      state.color = undefined;
      state.bold = false;
      state.dim = false;
    } else if (code === 1) {
      state.bold = true;
    } else if (code === 2) {
      state.dim = true;
    } else if (code === 22) {
      state.bold = false;
      state.dim = false;
    } else if (code === 39) {
      state.color = undefined;
    } else if (ANSI_COLORS.has(code)) {
      state.color = code;
    }
  }
};

export function parseAnsi(line: string): AnsiSegment[] {
  const state: Omit<AnsiSegment, "text" | "offset"> = {
    color: undefined,
    bold: false,
    dim: false,
  };
  const segments: AnsiSegment[] = [];
  let index = 0;

  const append = (text: string) => {
    if (text === "") {
      return;
    }
    const previous = segments.at(-1);
    if (
      previous &&
      previous.color === state.color &&
      previous.bold === state.bold &&
      previous.dim === state.dim
    ) {
      previous.text += text;
      return;
    }
    segments.push({
      text,
      offset: segments.reduce((sum, segment) => sum + segment.text.length, 0),
      ...state,
    });
  };

  while (index < line.length) {
    const escape = line.indexOf("\u001B", index);
    const c1Offset = line.slice(index).search(/[\u009B\u009D]/u);
    const c1 = c1Offset === -1 ? -1 : index + c1Offset;
    let next = Math.min(escape, c1);
    if (escape === -1) {
      next = c1;
    } else if (c1 === -1) {
      next = escape;
    }

    if (next === -1) {
      append(line.slice(index));
      break;
    }

    append(line.slice(index, next));
    const csi = line[next] === "\u009B" || line[next + 1] === "[";
    const osc = line[next] === "\u009D" || line[next + 1] === "]";

    if (csi) {
      const start = next + (line[next] === "\u001B" ? 2 : 1);
      const end = findCsiEnd(line, start);
      if (end === line.length) {
        break;
      }
      if (line[end] === "m") {
        applySgr(line.slice(start, end), state);
      }
      index = end + 1;
    } else if (osc) {
      index = findStringEnd(line, next + (line[next] === "\u001B" ? 2 : 1), true);
    } else if (line[next] === "\u001B" && ["P", "^", "_"].includes(line[next + 1] ?? "")) {
      index = findStringEnd(line, next + 2, false);
    } else {
      index =
        line[next] === "\u001B" &&
        ["(", ")", "*", "+", "-", ".", "/"].includes(line[next + 1] ?? "")
          ? next + 3
          : next + 2;
    }
  }

  return segments;
}
