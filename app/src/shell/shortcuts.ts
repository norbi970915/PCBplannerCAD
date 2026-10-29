import { activeDoc, useEditor } from "../core/store";
import { COMMANDS, commandAvailable, runCommand, type CommandId } from "./commands";
import { useContextMenu } from "./ContextMenu";

let bindings: Map<string, CommandId[]> | null = null;
let sequencePrefixes = new Set<string>();

function build() {
  bindings = new Map();
  sequencePrefixes = new Set();
  for (const [id, cmd] of Object.entries(COMMANDS)) {
    for (const key of cmd.keys ?? []) {
      bindings.set(key, [...(bindings.get(key) ?? []), id]);
      if (/^[A-Z] [A-Z]$/.test(key)) sequencePrefixes.add(key[0]);
    }
  }
}

/** First available command bound to `key` for the active editor. */
function resolve(key: string): CommandId | undefined {
  if (!bindings) build();
  return bindings!.get(key)?.find(commandAvailable);
}

export interface SequenceEntry {
  /** Second key of the sequence. */
  key: string;
  id: CommandId;
  label: string;
  enabled: boolean;
}

/** Commands reachable from a prefix key in the current editor, by second key. */
export function sequenceEntries(prefix: string): SequenceEntry[] {
  if (!bindings) build();
  const out: SequenceEntry[] = [];
  for (const [chord, ids] of bindings!) {
    const m = /^([A-Z]) ([A-Z])$/.exec(chord);
    if (!m || m[1] !== prefix) continue;
    // One row per second key: the command that would run, else the first one
    // that belongs to this editor (shown disabled).
    const available = ids.find(commandAvailable);
    const kind = activeDoc()?.info.kind;
    const shown = available ?? ids.find((id) => !COMMANDS[id].when || (kind && COMMANDS[id].when!.includes(kind)));
    if (!shown) continue;
    out.push({ key: m[2], id: shown, label: COMMANDS[shown].label, enabled: !!available });
  }
  return out.sort((a, b) => a.key.localeCompare(b.key));
}

/** Last pointer position, so the key menu opens where the user is looking. */
export const lastPointer: {x:number;y:number;window?:Window} = { x: 200, y: 200 };

let pending: string | null = null;

export function setPending(k: string | null) {
  pending = k;
  useEditor.getState().setPendingKeys(k);
}

/** Runs a command from the key menu (click or Enter) and closes it. */
export function runFromMenu(id: CommandId) {
  setPending(null);
  runCommand(id);
}

/** Normalizes an event to the notation used in COMMANDS, e.g. "Ctrl+Shift+S". */
function chordOf(e: KeyboardEvent): string {
  const isLetter = /^[a-z]$/i.test(e.key);
  const isSymbol = e.key.length === 1 && !isLetter;
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  // Shift is implied by the character itself for symbols like "+" or "*".
  if (e.shiftKey && !isSymbol) parts.push("Shift");
  parts.push(e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key);
  return parts.join("+");
}

function isTextInput(target: EventTarget | null): boolean {
  const el=target as HTMLElement|null;
  return !!el && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
}

export function installShortcuts(viewWindow:Window=window): () => void {
  // Built up front: the prefix check below runs before any resolve() would build it,
  // so the first P after start-up would otherwise not open the Place menu.
  if (!bindings) build();
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.defaultPrevented || isTextInput(e.target) || ["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    if (document.querySelector("[role=dialog]") || viewWindow.document.querySelector('[role="dialog"],[data-slot="popover-content"][data-state="open"],[role="menu"][data-slot],[role="listbox"]') || useContextMenu.getState().at) return;
    lastPointer.window=viewWindow;
    const chord = chordOf(e);
    const plainLetter = /^[A-Z]$/.test(chord);

    if (pending) {
      // The key menu handles its own navigation keys.
      if (["ArrowUp", "ArrowDown", "Enter", "Home", "End"].includes(e.key)) return;
      e.preventDefault();
      const id = plainLetter ? resolve(`${pending} ${chord}`) : undefined;
      if (plainLetter && !id && chord !== pending) return; // not in this menu: keep it open
      setPending(null);
      if (id) runCommand(id);
      return;
    }
    if (plainLetter && !e.repeat && sequencePrefixes.has(chord) && (activeDoc() || chord !== "P")) {
      e.preventDefault();
      setPending(chord);
      return;
    }
    const id = resolve(chord);
    if (id) {
      e.preventDefault();
      runCommand(id);
    } else if (chord === "Ctrl+R" || chord === "F5" || chord === "Ctrl+P" || chord === "Ctrl+F") {
      // Keep browser behaviors (reload, print, find) out of the editor.
      e.preventDefault();
    }
  };
  const onPointer = (e: PointerEvent) => {
    lastPointer.window=viewWindow;
    lastPointer.x = e.clientX;
    lastPointer.y = e.clientY;
  };

  viewWindow.addEventListener("keydown", onKeyDown);
  viewWindow.addEventListener("pointermove", onPointer, true);
  viewWindow.addEventListener("pointerdown", onPointer, true);
  return () => {
    viewWindow.removeEventListener("keydown", onKeyDown);
    viewWindow.removeEventListener("pointermove", onPointer, true);
    viewWindow.removeEventListener("pointerdown", onPointer, true);
    setPending(null);
  };
}
