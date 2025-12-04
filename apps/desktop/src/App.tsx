import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "./App.css";
import { ProfileForm } from "./features/profile/ProfileForm";
import { AgentChat } from "./features/agent/AgentChat";
import { FileExplorer } from "./features/files/FileExplorer";
import { CommandRunner } from "./features/terminal/CommandRunner";

const queryClient = new QueryClient();

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <main className="container">
        <h1>Welcome to IronGraph</h1>
        <div className="row">
          <ProfileForm />
        </div>
        <div className="row" style={{ marginTop: "20px", padding: "0 20px" }}>
            <div style={{ width: "100%", maxWidth: "1200px" }}>
                <AgentChat />
            </div>
        </div>
        <div className="row" style={{ marginTop: "20px", padding: "0 20px" }}>
            <div style={{ width: "100%", maxWidth: "1200px" }}>
                <CommandRunner />
            </div>
        </div>
        <div className="row" style={{ marginTop: "20px", padding: "0 20px" }}>
            <div style={{ width: "100%", maxWidth: "1200px" }}>
                <FileExplorer />
            </div>
        </div>
      </main>
    </QueryClientProvider>
  );
}

export default App;
