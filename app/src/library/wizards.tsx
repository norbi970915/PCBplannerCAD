import { newLibrary } from "../core/actions";
import { bus } from "../core/bus";
import { activeDoc, useEditor } from "../core/store";
import { useModal } from "../shell/modal";
import { FootprintWizard } from "./FootprintWizard";
import { SymbolWizard } from "./SymbolWizard";

/** Picks a writable target library: the active editor's, else the first writable one. */
function targetLibrary(): string | null {
  const s = useEditor.getState();
  const fromDoc = activeDoc()?.info.library;
  const writable = s.libs.filter((l) => !l.readonly);
  if (fromDoc && writable.some((l) => l.id === fromDoc)) return fromDoc;
  return writable[0]?.id ?? null;
}

export async function openWizardFor(kind: "footprint" | "symbol") {
  let lib = targetLibrary();
  if (!lib) {
    await newLibrary("MyLibrary");
    lib = targetLibrary();
    if (!lib) return;
  }
  const libId = lib;
  const done = () => {
    useEditor.getState().bumpLib(libId);
    bus.emit("showLibrary", { libId });
  };
  useModal.getState().show(kind === "footprint" ? <FootprintWizard libId={libId} onDone={done} /> : <SymbolWizard libId={libId} onDone={done} />);
}
