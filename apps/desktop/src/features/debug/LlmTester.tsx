import { useState } from "react";
import { commands, ToolCall } from "../../bindings";

export function LlmTester() {
  const [baseUrl, setBaseUrl] = useState("mock");
  const [apiKey, setApiKey] = useState("sk-dummy");
  const [model, setModel] = useState("gpt-4o");
  const [content, setContent] = useState("");
  const [toolCalls, setToolCalls] = useState<ToolCall[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSend() {
    setLoading(true);
    setContent("");
    setToolCalls(null);
    setError(null);
    try {
      const res = await commands.sendChat({
        messages: [{ role: "user", content: "Hello" }],
        config: {
          base_url: baseUrl,
          api_key: apiKey,
          model: model,
          temperature: 0.7,
        },
      });

      if (res.status === "ok") {
        setContent(res.data.content);
        setToolCalls(res.data.tool_calls);
      } else {
        setError(`Error: ${res.error}`);
      }
    } catch (e) {
      setError(`Exception: ${e}`);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div style={{ padding: "20px", border: "1px solid #444", borderRadius: "8px", marginTop: "20px", background: "#222", color: "#fff" }}>
      <h3 style={{ marginTop: 0 }}>LLM Gateway Debugger</h3>
      <div style={{ display: "flex", flexDirection: "column", gap: "10px", marginBottom: "15px" }}>
        <div>
            <label style={{ display: "block", fontSize: "0.8em", marginBottom: "4px" }}>Base URL</label>
            <input
                style={{ width: "100%", padding: "8px", background: "#333", border: "1px solid #555", color: "white" }}
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="Base URL (use 'mock' for testing)"
            />
        </div>
        <div>
            <label style={{ display: "block", fontSize: "0.8em", marginBottom: "4px" }}>API Key</label>
            <input
                style={{ width: "100%", padding: "8px", background: "#333", border: "1px solid #555", color: "white" }}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="API Key"
                type="password"
            />
        </div>
        <div>
            <label style={{ display: "block", fontSize: "0.8em", marginBottom: "4px" }}>Model</label>
            <input
                style={{ width: "100%", padding: "8px", background: "#333", border: "1px solid #555", color: "white" }}
                value={model}
                onChange={(e) => setModel(e.target.value)}
                placeholder="Model Name"
            />
        </div>
      </div>
      <button
        onClick={handleSend}
        disabled={loading}
        style={{ padding: "10px 20px", cursor: loading ? "not-allowed" : "pointer" }}
      >
        {loading ? "Sending Request..." : "Send 'Hello' Probe"}
      </button>

      {error && (
        <div style={{ marginTop: "15px", color: "red", border: "1px solid red", padding: "10px" }}>
            {error}
        </div>
      )}

      {/* Top Box: Raw Content (Thoughts) */}
      {content && (
        <div style={{ marginTop: "15px" }}>
            <label style={{ display: "block", fontSize: "0.8em", marginBottom: "4px", color: "#aaa" }}>Thoughts (Raw Content)</label>
            <pre style={{ textAlign: "left", background: "#111", padding: "10px", borderRadius: "4px", overflowX: "auto", border: "1px solid #333" }}>
                {content}
            </pre>
        </div>
      )}

      {/* Bottom Box: Tool Calls (Actions) */}
      {toolCalls && toolCalls.length > 0 && (
        <div style={{ marginTop: "15px", background: "#1e3a8a", padding: "10px", borderRadius: "8px", border: "1px solid #3b82f6" }}>
            <label style={{ display: "block", fontSize: "0.8em", marginBottom: "8px", color: "#93c5fd", fontWeight: "bold" }}>Actions (Tool Calls)</label>
            <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
                {toolCalls.map((tool, idx) => (
                    <div key={idx} style={{ background: "#0f172a", padding: "10px", borderRadius: "4px", border: "1px solid #1e40af" }}>
                        <div style={{ fontWeight: "bold", color: "#60a5fa", marginBottom: "5px" }}>
                            Tool: {tool.name}
                        </div>
                        {Object.entries(tool.arguments).map(([key, value]) => (
                            <div key={key} style={{ display: "flex", fontSize: "0.9em", marginLeft: "10px" }}>
                                <span style={{ color: "#94a3b8", width: "80px" }}>{key}:</span>
                                <span style={{ color: "#e2e8f0" }}>{value}</span>
                            </div>
                        ))}
                    </div>
                ))}
            </div>
        </div>
      )}
    </div>
  );
}
