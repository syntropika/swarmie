/** Redacts known credential values only in operational errors and stderr. */
export function diagnosticRedactor(environment = process.env) {
  const values = new Set();
  for (const [name, value] of Object.entries(environment)) {
    if (value && /token|secret|password|api_?key|authorization|credential/iu.test(name)) {
      values.add(value);
      values.add(JSON.stringify(value).slice(1, -1));
    }
  }
  const secrets = [...values].toSorted((a, b) => b.length - a.length);
  return (value) => {
    let text = String(value);
    for (const secret of secrets) {
      text = text.split(secret).join("[redacted]");
    }
    return text;
  };
}

/** Holds complete lines so credentials split across subprocess chunks stay private. */
export function diagnosticWriter(write, redact) {
  let pending = "";
  let truncated = false;
  return {
    write(chunk) {
      pending += String(chunk);
      let end;
      while ((end = pending.indexOf("\n")) !== -1) {
        const line = pending.slice(0, end + 1);
        pending = pending.slice(end + 1);
        if (truncated || line.length > 65_536) {
          write("[diagnostic line truncated]\n");
        } else {
          write(redact(line));
        }
        truncated = false;
      }
      if (pending.length > 65_536) {
        pending = "";
        truncated = true;
      }
    },
    flush() {
      if (truncated) {
        write("[diagnostic line truncated]\n");
      } else if (pending) {
        write(redact(pending));
      }
      pending = "";
      truncated = false;
    },
  };
}
