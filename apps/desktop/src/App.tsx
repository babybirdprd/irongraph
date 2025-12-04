import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "./App.css";
import { ProfileForm } from "./features/profile/ProfileForm";
import { LlmTester } from "./features/debug/LlmTester";

const queryClient = new QueryClient();

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <main className="container">
        <h1>Welcome to IronGraph</h1>
        <div className="row">
          <ProfileForm />
        </div>
        <div className="row">
          <LlmTester />
        </div>
      </main>
    </QueryClientProvider>
  );
}

export default App;
