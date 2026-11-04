import { useEffect, useRef } from "react";
import { inTauri } from "./api";
import { needsFirstSteps } from "./firstSteps";
import { useAppStore } from "./store";

/**
 * Opens the first-steps screen once per launch when there is nothing to chat
 * with. "Once" so that skipping it, or going to Settings, is not undone a second
 * later. Only from the chat view, so it never pulls someone away from a page they
 * chose. The screen stays in Settings for anyone who wants it again.
 */
export function useFirstSteps() {
  const modelsLoaded = useAppStore((s) => s.modelsLoaded);
  const libraryModels = useAppStore((s) => s.libraryModels);
  const providers = useAppStore((s) => s.providers);
  const endpointModels = useAppStore((s) => s.endpointModels);
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const shown = useRef(false);

  useEffect(() => {
    if (shown.current) return;
    if (!needsFirstSteps({ modelsLoaded, libraryModels, providers, endpointModels }, inTauri())) return;
    shown.current = true;
    if (view === "chat") setView("firststeps");
  }, [modelsLoaded, libraryModels, providers, endpointModels, view, setView]);
}
