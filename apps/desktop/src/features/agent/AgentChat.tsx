import { useState, useRef, useEffect } from "react";
import { useBackendAgent } from "../../hooks/useBackendAgent";
import { Message } from "../../bindings";

// Helper component for message rendering
const MessageBubble = ({ msg }: { msg: Message }) => {
  const isUser = msg.role === "user";
  // Identify if it's a tool output/log message.
  // User messages that start with "Tool Output" or "Tool Error" are logs.
  // BUT: The initial user prompt is also role "user". We distinguish by content prefix.
  const isToolOutput = (msg.role === "user") && (msg.content.startsWith("Tool Output") || msg.content.startsWith("Tool Error"));

  if (isToolOutput) {
      // Collapsible Tool Log
      return <ToolLog content={msg.content} />;
  }

  // System prompts could be hidden or shown differently.
  if (msg.role === "system") {
      return (
        <div style={{ textAlign: "center", fontSize: "0.8em", color: "#666", marginBottom: "10px" }}>
            [System Prompt / Message]
        </div>
      );
  }

  return (
    <div style={{
        alignSelf: isUser ? "flex-end" : "flex-start",
        background: isUser ? "#3b82f6" : "#374151",
        color: "white",
        padding: "10px",
        borderRadius: "10px",
        maxWidth: "80%",
        whiteSpace: "pre-wrap",
        marginBottom: "10px"
    }}>
        <div style={{ fontWeight: "bold", fontSize: "0.8em", marginBottom: "5px", color: isUser ? "#bfdbfe" : "#9ca3af" }}>
            {msg.role.toUpperCase()}
        </div>
        {msg.content}
    </div>
  );
};

const ToolLog = ({ content }: { content: string }) => {
    const [collapsed, setCollapsed] = useState(true);
    const isError = content.startsWith("Tool Error");
    const header = content.split("\n")[0];
    const body = content.substring(header.length + 1);

    return (
        <div style={{
            alignSelf: "stretch",
            background: "#1e293b",
            border: `1px solid ${isError ? "#ef4444" : "#475569"}`,
            borderRadius: "5px",
            marginBottom: "10px",
            overflow: "hidden"
        }}>
            <div
                onClick={() => setCollapsed(!collapsed)}
                style={{
                    padding: "8px",
                    cursor: "pointer",
                    fontSize: "0.85em",
                    fontFamily: "monospace",
                    color: isError ? "#fca5a5" : "#94a3b8",
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center"
                }}
            >
                <span>{header}</span>
                <span>{collapsed ? "▼" : "▲"}</span>
            </div>
            {!collapsed && (
                <div style={{
                    padding: "8px",
                    borderTop: "1px solid #334155",
                    fontSize: "0.8em",
                    fontFamily: "monospace",
                    whiteSpace: "pre-wrap",
                    color: "#cbd5e1",
                    maxHeight: "300px",
                    overflowY: "auto"
                }}>
                    {body}
                </div>
            )}
        </div>
    );
}

export function AgentChat() {
    const { messages, isLooping, startLoop, stopLoop } = useBackendAgent();
    const [input, setInput] = useState("");
    const messagesEndRef = useRef<HTMLDivElement>(null);

    const handleSend = () => {
        if (!input.trim() || isLooping) return;
        startLoop(input);
        setInput("");
    };

    // Auto-scroll to bottom
    useEffect(() => {
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }, [messages]);

    return (
        <div style={{
            display: "flex",
            flexDirection: "column",
            height: "600px",
            border: "1px solid #444",
            borderRadius: "8px",
            background: "#111",
            color: "white"
        }}>
            {/* Header */}
            <div style={{ padding: "10px", borderBottom: "1px solid #333", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <h3 style={{ margin: 0 }}>Agent Loop</h3>
                {isLooping && <span style={{ color: "#4ade80", fontSize: "0.8em" }}>● Running...</span>}
            </div>

            {/* Messages Area */}
            <div style={{ flex: 1, overflowY: "auto", padding: "20px", display: "flex", flexDirection: "column" }}>
                {messages.map((msg, idx) => (
                    <MessageBubble key={idx} msg={msg} />
                ))}
                <div ref={messagesEndRef} />
            </div>

            {/* Input Area */}
            <div style={{ padding: "15px", borderTop: "1px solid #333", display: "flex", gap: "10px" }}>
                <textarea
                    value={input}
                    onChange={(e) => setInput(e.target.value)}
                    onKeyDown={(e) => {
                        if (e.key === "Enter" && !e.shiftKey) {
                            e.preventDefault();
                            handleSend();
                        }
                    }}
                    placeholder="Ask the agent to do something..."
                    disabled={isLooping}
                    style={{
                        flex: 1,
                        background: "#222",
                        border: "1px solid #444",
                        color: "white",
                        borderRadius: "5px",
                        padding: "10px",
                        resize: "none",
                        height: "50px"
                    }}
                />
                <div style={{ display: "flex", flexDirection: "column", gap: "5px" }}>
                    <button
                        onClick={handleSend}
                        disabled={isLooping || !input.trim()}
                        style={{
                            padding: "8px 20px",
                            background: isLooping ? "#555" : "#2563eb",
                            color: "white",
                            border: "none",
                            borderRadius: "5px",
                            cursor: isLooping ? "not-allowed" : "pointer",
                            flex: 1
                        }}
                    >
                        Send
                    </button>
                    {isLooping && (
                        <button
                            onClick={stopLoop}
                            style={{
                                padding: "5px",
                                background: "#dc2626",
                                color: "white",
                                border: "none",
                                borderRadius: "5px",
                                cursor: "pointer",
                                fontSize: "0.8em"
                            }}
                        >
                            Stop
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
}
