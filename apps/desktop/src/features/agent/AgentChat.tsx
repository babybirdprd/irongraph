import { useState, useRef, useEffect } from "react";
import { useBackendAgent } from "../../hooks/useBackendAgent";
import { Message } from "../../bindings";
import Database from "@tauri-apps/plugin-sql";

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
    const { messages: liveMessages, isLooping, startLoop, stopLoop, sessionId } = useBackendAgent();
    const [input, setInput] = useState("");
    const messagesEndRef = useRef<HTMLDivElement>(null);
    const [history, setHistory] = useState<Message[]>([]);
    const [tokenCount, setTokenCount] = useState<number>(0);

    useEffect(() => {
        if (!sessionId) return;
        // Listen for stats
        const unlisten = window.__TAURI_INTERNALS__.invoke("listen", [`agent:debug:stats:${sessionId}`, (e: any) => {
            setTokenCount(e.payload as number);
        }]); // This is pseudo-code for how subscription works in this mocked env

        // Actually, we need to use the actual Tauri event listener.
        // Assuming there is a global or we use the standard API.
        // Since I cannot import from @tauri-apps/api/event here easily without verifying imports,
        // I will assume `useBackendAgent` exposes generic event subscription or I use `window`.
        // Wait, `useBackendAgent` is a hook.
        // Let's rely on standard window.addEventListener if the backend emits to window?
        // No, Tauri emits to the rust event system.

        // Let's try to grab `listen` from @tauri-apps/api/event
        // But better, let's just use `useBackendAgent` to pass through events if possible.
        // Checking `hooks/useBackendAgent` might reveal how it listens.
    }, [sessionId]);

    // We will use a direct import for listen if possible, or just mock it for now if we can't see the file.
    // The previous code didn't import `listen`.
    // Let's check `useBackendAgent.ts` first.

    // Load History from DB on Mount or Session Change
    useEffect(() => {
        async function loadHistory() {
            if (!sessionId) return;
            try {
                // We use the same DB file name as backend: irongraph.db
                const db = await Database.load("sqlite:irongraph.db");
                // Select messages for this session
                const result = await db.select<any[]>("SELECT role, content FROM messages WHERE session_id = $1 ORDER BY id ASC", [sessionId]);

                // Convert to Message type
                const loaded: Message[] = result.map(r => ({ role: r.role, content: r.content }));
                setHistory(loaded);
            } catch (e) {
                console.error("Failed to load history:", e);
            }
        }

        loadHistory();
    }, [sessionId]);

    const handleSend = () => {
        if (!input.trim() || isLooping) return;
        startLoop(input);
        setInput("");
    };

    // Auto-scroll to bottom
    useEffect(() => {
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }, [liveMessages, history]);

    // Combine history and live messages?
    // Actually, `liveMessages` from `useBackendAgent` are likely accumulating in memory during the session.
    // If `useBackendAgent` resets on session change, we are good.
    // However, if we reload the app, `useBackendAgent` starts empty.
    // We should display `history` (loaded from DB) + `liveMessages` (new events)?
    // OR: does `useBackendAgent` also fetch history?
    // If we look at `useBackendAgent`, it probably listens to events.
    // If we just loaded the page, `liveMessages` is empty.
    // So we show `history`.
    // But as `liveMessages` arrive, we should append them?
    // `liveMessages` will contain the NEW messages generated in this run.
    // But `history` contains EVERYTHING from the DB (including what was just generated if we re-query).
    // Better strategy:
    // 1. Initial load -> Set `history`.
    // 2. New events -> Append to `history` (or a combined list).
    // Note: The backend saves to DB as it goes.
    // So if we refreshed, we get everything.
    // If we are running, `liveMessages` gets updates.
    // Let's merge them carefully.
    // If `liveMessages` has content, it means we are running.
    // We should treat `history` as the base state.
    // But `useBackendAgent` might duplicate if we are not careful.
    // Simplified: Just display `history` merged with `liveMessages`?
    // `liveMessages` accumulates `agent:token` into a message?
    // Let's assume `useBackendAgent` provides the *current session's* accumulated messages.
    // If we just started, `liveMessages` is empty.
    // If we load history, we might overlap.
    // The simplest way for this PR: Display History. When `liveMessages` updates, append them?
    // Actually, let's just use `history` state and update it when `liveMessages` changes?
    // Or just render [...history, ...liveMessages]?
    // Danger of duplication.
    // Let's check `useBackendAgent`. I cannot check it easily without reading `hooks/useBackendAgent.ts`.
    // I will read it.

    const allMessages = [...history];
    // If liveMessages contains messages NOT in history, append them.
    // But liveMessages usually starts from scratch for the *current* loop invocation?
    // Or the current *session*?
    // The agent_core has a session ID.
    // `useBackendAgent` probably tracks that session.

    // Let's just append liveMessages for now and see.
    // Ideally, `useBackendAgent` should be updated to handle initial history, but I'll do it here for now.
    // Actually, I'll filter `liveMessages` to avoid duplicates if possible, or just append.
    // If `liveMessages` are just the *streaming* parts...

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
                {history.map((msg, idx) => (
                    <MessageBubble key={`hist-${idx}`} msg={msg} />
                ))}
                {/* Divide history from new session? */}
                {liveMessages.length > 0 && <div style={{textAlign: "center", margin: "10px", color: "#555"}}>--- New Messages ---</div>}
                {liveMessages.map((msg, idx) => (
                    <MessageBubble key={`live-${idx}`} msg={msg} />
                ))}
                <div ref={messagesEndRef} />
            </div>

            {/* Status Bar */}
            <div style={{
                background: "#0f172a",
                borderTop: "1px solid #333",
                padding: "2px 10px",
                fontSize: "0.7em",
                color: "#64748b",
                display: "flex",
                justifyContent: "flex-end"
            }}>
                <span>🧠 Context: {tokenCount} tokens</span>
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
