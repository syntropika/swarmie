import { createRoot } from "react-dom/client";
import { useState } from "react";
import "../../../packages/ui/web/src/styles.css";
import { ModelPickerExample } from "../../../packages/ui/web/src/elements/model-picker.stories";
import { Button } from "../../../packages/ui/web/src/atoms/button";

function Fixture() {
  const [dark, setDark] = useState(false);
  return <div className={dark ? "dark bg-background text-foreground" : "light bg-background text-foreground"}>
    <main className="flex min-h-dvh flex-col gap-8 p-6">
      <p className="text-caption text-muted">UI fixture with synthetic accounts and models.</p>
      <Button onPress={() => { setDark(!dark); document.documentElement.classList.toggle("dark", !dark); document.documentElement.classList.toggle("light", dark); }}>Toggle theme</Button>
      <div className="mt-auto self-end"><ModelPickerExample /></div>
    </main>
  </div>;
}
const root = document.getElementById("root");
if (root) createRoot(root).render(<Fixture />);
