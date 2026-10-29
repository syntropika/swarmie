import { Button, ResponsiveDialog } from "@swarmie/ui-web";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import "../../../apps/web-client/src/styles.css";
import { FolderPicker } from "../../../plugins/projects/browser/src/folder-picker";
import type { DirectoryBrowser } from "../../../plugins/projects/browser/src/directories";

const browser: DirectoryBrowser = { open: () => ({
  close: () => undefined,
  async list(request, signal) {
    await new Promise<void>((resolve) => { setTimeout(resolve, 40); });
    signal.throwIfAborted();
    const path = request.path ?? "/user";
    if (path === "/user/restricted") { throw new Error("This directory cannot be read."); }
    return {
      path,
      parent: path === "/user" ? null : "/user",
      directories: path === "/user" ? [
        { name: "Documents", path: "/user/Documents" },
        { name: "Projects", path: "/user/Projects" },
        { name: "Restricted", path: "/user/restricted" },
      ] : [],
      next: null,
    };
  },
}) };
function Fixture() {
  const [open, setOpen] = useState(true);
  const [chosen, setChosen] = useState("");
  return <main className="min-h-dvh bg-background p-6 text-foreground">
    <p>Synthetic node folder picker fixture.</p><output>{chosen}</output>
    <Button onPress={() => { setOpen(true); }}>Browse folders</Button>
    <ResponsiveDialog isOpen={open} onOpenChange={setOpen}><ResponsiveDialog.Backdrop><ResponsiveDialog.Container><ResponsiveDialog.Dialog aria-label="Choose a folder">
      <ResponsiveDialog.Header><ResponsiveDialog.Heading>Choose a folder</ResponsiveDialog.Heading><ResponsiveDialog.CloseTrigger /></ResponsiveDialog.Header>
      <FolderPicker browser={browser} nodeId="fixture" choose={(path) => { setChosen(path); setOpen(false); }} cancel={() => { setOpen(false); }} />
    </ResponsiveDialog.Dialog></ResponsiveDialog.Container></ResponsiveDialog.Backdrop></ResponsiveDialog>
  </main>;
}
const root = document.getElementById("root");
if (root) { createRoot(root).render(<Fixture />); }
