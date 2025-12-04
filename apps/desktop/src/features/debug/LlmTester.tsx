import { useState } from "react";
import { commands } from "../../bindings";

export function LlmTester() {
  const [baseUrl, setBaseUrl] = useState("mock");
  const [apiKey, setApiKey] = useState("sk-dummy");
  const [model, setModel] = useState("gpt-4o");
  const [response, setResponse] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSend() {
    setLoading(true);
    setResponse("");
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
        setResponse(res.data.content);
      } else {
        setResponse(`Error: ${res.error}`);
      }
    } catch (e) {
      setResponse(`Exception: ${e}`);
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

      {response && (
        <div style={{ marginTop: "15px" }}>
            <label style={{ display: "block", fontSize: "0.8em", marginBottom: "4px" }}>Response</label>
            <pre style={{ textAlign: "left", background: "#111", padding: "10px", borderRadius: "4px", overflowX: "auto", border: "1px solid #333" }}>
                {response}
            </pre>
        </div>
      )}
    </div>
  );
}
