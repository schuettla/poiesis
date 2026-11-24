import { useCallback, useEffect } from "react";
import { Lockup } from "./components/Art";
import Chat from "./screens/Chat";
import Chats from "./screens/Chats";
import Pair from "./screens/Pair";
import { useStore } from "./store";
import { leaveVoice } from "./voice/controller";
import { useVoiceStore } from "./voice/voiceStore";

/** Back to the list. Voice belongs to a chat, so it ends with it. */
function closeChat() {
  if (useVoiceStore.getState().shown) void leaveVoice();
  useStore.getState().showChats();
}

export default function App() {
  const { ready, paired, screen, active, approvals, chats, boot, resume, openChat } = useStore();

  useEffect(() => {
    void boot();
  }, [boot]);

  // Back to the front: reconnect, refresh the list and listen again (REM-UI-8).
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible" && useStore.getState().paired) void resume();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, [resume]);

  // An open chat is one step in the webview's history, so the phone's own back gesture returns to the list
  // (Tauri goes back in the history while there is one, and closes the app when there is none).
  useEffect(() => {
    if (screen === "chat" && window.history.state?.screen !== "chat") window.history.pushState({ screen: "chat" }, "");
  }, [screen]);
  useEffect(() => {
    const onPop = () => {
      if (useStore.getState().screen === "chat") closeChat();
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);
  const back = useCallback(() => {
    if (window.history.state?.screen === "chat") window.history.back();
    else closeChat();
  }, []);

  if (!ready) {
    return (
      <main className="app boot">
        <Lockup />
        <p className="boot-line">Looking for your desktop ...</p>
      </main>
    );
  }
  if (!paired) {
    return (
      <main className="app">
        <Pair />
      </main>
    );
  }

  // A question from another chat: a pill under the bar opens the chat where its card is.
  const elsewhere = approvals.find((a) => a.conversationId && (a.conversationId !== active || screen !== "chat"));
  const where = elsewhere ? chats.find((c) => c.id === elsewhere.conversationId)?.title : undefined;
  return (
    <main className="app">
      {screen === "chat" ? <Chat onBack={back} /> : <Chats />}
      {elsewhere?.conversationId && (
        <button className="ask-pill" onClick={() => void openChat(elsewhere.conversationId!)}>
          <span className="ask-dot" aria-hidden />
          {where ? `Poiesis is asking you something in "${where}"` : "Poiesis is asking you something"}
        </button>
      )}
    </main>
  );
}
