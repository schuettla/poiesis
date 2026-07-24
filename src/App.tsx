import { useEffect } from "react";
import { useAppStore } from "./lib/store";
import TopBar from "./components/TopBar/TopBar";
import Rail from "./components/Rail/Rail";
import Chat from "./routes/Chat";
import Models from "./routes/Models";
import Engine from "./routes/Engine";
import Apps from "./routes/Apps";
import Settings from "./routes/Settings";
import Library from "./routes/Library";
import PermissionPanel from "./components/SidePanel/PermissionPanel";
import "./App.css";

export default function App() {
  const view = useAppStore((s) => s.view);
  const bootstrap = useAppStore((s) => s.bootstrap);
  const railCollapsed = useAppStore((s) => s.railCollapsed);

  useEffect(() => {
    bootstrap();
  }, [bootstrap]);

  return (
    <div className={`app ${railCollapsed ? "rail-collapsed" : ""}`}>
      <TopBar />
      <Rail />
      {view === "chat" && <Chat />}
      {view === "models" && <Models />}
      {view === "engine" && <Engine />}
      {view === "apps" && <Apps />}
      {view === "settings" && <Settings />}
      {view === "library" && <Library />}
      <PermissionPanel />
    </div>
  );
}
