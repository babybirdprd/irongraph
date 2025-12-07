import { useState, useRef, useEffect } from "react";
import { commands, Message } from "../bindings";
import { listen } from "@tauri-apps/api/event";

interface StreamEvent {
    Token?: string;
    ToolStart?: string;
    ToolArg?: [string, string]; // Rust tuple
    ToolEnd?: null;
    Error?: string;
    Done?: null;
}

export function useBackendAgent() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [isLooping, setIsLooping] = useState(false);
  const sessionIdRef = useRef<string | null>(null);

  // We need to accumulate tokens for the *current* assistant message.
  // And also tool calls?
  // The backend emits events.
  // We should reconstruct the history locally to display it.

  // Current Partial State
  const currentAssistantMsgRef = useRef<string>("");

  useEffect(() => {
      // Listen to events if session ID exists
      let unlistenToken: (() => void) | undefined;
      let unlistenStatus: (() => void) | undefined;
      let unlistenTool: (() => void) | undefined;

      const setupListeners = async (sid: string) => {
          console.log("Setting up listeners for session", sid);

          unlistenToken = await listen<StreamEvent>(`agent:token:${sid}`, (event) => {
              const payload = event.payload;
              // Handle Rust enum serialization: { Token: "..." } or "ToolEnd" (if unit variant, depends on Specta settings)
              // Specta usually serializes enums as { "Variant": content } or "Variant" if unit.
              // My StreamEvent in Rust:
              // Token(String), ToolStart(String), ToolArg(String, String), ToolEnd, Error(String), Done

              if (typeof payload === "string") {
                  if (payload === "ToolEnd") {
                       // End of tool definition in text?
                       // In my loop logic, I append XML to content.
                       // Actually, in `agent_core`, I append `ToolEnd`? No.
                       // `agent_core` handles `ToolEnd`.
                       // BUT `agent_core` ALSO emits the event to frontend.
                  }
                  if (payload === "Done") {
                      // Stream finished (one turn).
                  }
              } else {
                  if ("Token" in payload && payload.Token) {
                      currentAssistantMsgRef.current += payload.Token;

                      // Update the LAST message if it is assistant, or create new?
                      setMessages(prev => {
                          const last = prev[prev.length - 1];
                          if (last && last.role === "assistant") {
                              return [...prev.slice(0, -1), { ...last, content: currentAssistantMsgRef.current }];
                          } else {
                              return [...prev, { role: "assistant", content: currentAssistantMsgRef.current }];
                          }
                      });
                  }
                  // We might want to show Tools appearing in real time?
                  // For now, let's just show the raw XML appearing in the text if the Agent outputs it.
                  // Wait, the Agent outputs `<tool_code>...`.
                  // My parser consumes it.
                  // Does `agent_core` emit the RAW text of tool code as tokens?
                  // Let's check `agent_core`.
                  // `StreamEvent::Token(t) => assistant_content.push_str(&t)`
                  // `StreamEvent::ToolStart`...
                  // The parser in `llm_gateway` consumes tags. It does NOT return them as tokens.
                  // So `currentAssistantMsgRef` will MISS the tool code XML.
                  // The user won't see "Thinking... <tool>".
                  // They will see "Thinking... " and then tool executes.
                  // The Prompt "Summary" says: "The user sees 'Thoughts' immediately, and tools execute...".
                  // The "Verification Steps" don't explicitly say we must see the XML tags.
                  // But usually seeing the plan is good.
                  // If `llm_gateway` parser consumes tags, they are gone from `Token` stream.

                  // However, `agent_core` receives `ToolStart`, `ToolArg`.
                  // We could reconstruct the XML for display if we want.
                  // Or just render a UI representation of "Using tool...".

                  if ("ToolStart" in payload && payload.ToolStart) {
                       // Maybe append [Using Tool: name] to content?
                       const toolInfo = `\n[Using Tool: ${payload.ToolStart} ...`;
                       currentAssistantMsgRef.current += toolInfo;
                       setMessages(prev => {
                          const last = prev[prev.length - 1];
                          if (last && last.role === "assistant") {
                              return [...prev.slice(0, -1), { ...last, content: currentAssistantMsgRef.current }];
                          }
                          return prev;
                       });
                  }

                  if ("ToolArg" in payload && payload.ToolArg) {
                       // payload.ToolArg is [key, value]
                       const [k, v] = payload.ToolArg;
                       // We can append args?
                  }
              }
          });

          unlistenStatus = await listen<string>(`agent:status:${sid}`, (event) => {
               const status = event.payload; // "running" | "waiting"
               console.log("Agent Status:", status);
               if (status === "waiting") {
                   setIsLooping(false);
                   currentAssistantMsgRef.current = ""; // Reset for next turn
               } else if (status === "running") {
                   setIsLooping(true);
               }
          });

          unlistenTool = await listen<string>(`agent:tool_output:${sid}`, (event) => {
               const output = event.payload;
               // Add as a User message (Tool Output)
               setMessages(prev => [...prev, { role: "user", content: output }]);
          });
      };

      if (sessionIdRef.current) {
          setupListeners(sessionIdRef.current);
      }

      return () => {
          if (unlistenToken) unlistenToken();
          if (unlistenStatus) unlistenStatus();
          if (unlistenTool) unlistenTool();
      };
  }, []); // We rely on sessionIdRef not changing often, or we need to depend on it if we put it in state.

  const startLoop = async (userPrompt: string) => {
    setIsLooping(true); // Optimistic
    setMessages(prev => [...prev, { role: "user", content: userPrompt }]);
    currentAssistantMsgRef.current = "";

    try {
        // Start backend loop
        const res = await commands.startAgentLoop(userPrompt);
        if (res.status === "ok") {
            const sid = res.data;
            if (sid !== sessionIdRef.current) {
                sessionIdRef.current = sid;
                // Trigger listener setup?
                // UseEffect with empty deps won't see this change.
                // We need to manually call setup or force re-render.
                // Better: put sid in state.
                setSessionId(sid);
            }
        } else {
            console.error("Failed to start agent:", res.error);
            setIsLooping(false);
            setMessages(prev => [...prev, { role: "system", content: `Error: ${res.error}` }]);
        }
    } catch (e) {
        console.error(e);
        setIsLooping(false);
    }
  };

  const [sessionId, setSessionId] = useState<string | null>(null);

  // Re-trigger effect when sessionId changes
  useEffect(() => {
      if (!sessionId) return;
      let unlistenToken: (() => void) | undefined;
      let unlistenStatus: (() => void) | undefined;
      let unlistenTool: (() => void) | undefined;

      const setup = async () => {
          unlistenToken = await listen<any>(`agent:token:${sessionId}`, (event) => {
              const payload = event.payload;
              // Logic same as above
              if (typeof payload === "string") {
                 // "Done", "ToolEnd"
              } else {
                  if ("Token" in payload) {
                       currentAssistantMsgRef.current += payload.Token;
                       setMessages(prev => {
                          const last = prev[prev.length - 1];
                          // Check if last message is assistant.
                          // NOTE: tool outputs are "user". So if tool ran, last msg is user.
                          // Then we need NEW assistant message.
                          if (last && last.role === "assistant") {
                              return [...prev.slice(0, -1), { ...last, content: currentAssistantMsgRef.current }];
                          } else {
                              return [...prev, { role: "assistant", content: currentAssistantMsgRef.current }];
                          }
                       });
                  }
                  if ("ToolStart" in payload) {
                       const toolName = payload.ToolStart;
                       currentAssistantMsgRef.current += `\n[Using Tool: ${toolName}]`;
                       setMessages(prev => {
                          const last = prev[prev.length - 1];
                          if (last && last.role === "assistant") {
                              return [...prev.slice(0, -1), { ...last, content: currentAssistantMsgRef.current }];
                          }
                          return prev; // Should have been created by Token or we create it?
                       });
                  }
                  // Ignore others for display simplicity
              }
          });

          unlistenStatus = await listen<string>(`agent:status:${sessionId}`, (event) => {
               if (event.payload === "waiting") {
                   setIsLooping(false);
                   currentAssistantMsgRef.current = "";
               } else if (event.payload === "running") {
                   setIsLooping(true);
               }
          });

          unlistenTool = await listen<string>(`agent:tool_output:${sessionId}`, (event) => {
               const output = event.payload;
               setMessages(prev => [...prev, { role: "user", content: output }]);
               // After tool output, assistant usually continues thinking (next loop iteration).
               // So next Token will create new assistant message or append?
               // Logic: `currentAssistantMsgRef` was reset? No, I only reset on "waiting".
               // If I receive tool output, the NEXT token comes from `agent_core` loop's NEXT iteration (new stream).
               // So I SHOULD reset `currentAssistantMsgRef`?
               // The `agent_core` loop:
               // 1. stream response (append tokens).
               // 2. execute tools (emit tool_output).
               // 3. loop -> stream response.

               // So yes, after tool execution, we expect a NEW assistant message (Thought).
               currentAssistantMsgRef.current = "";
          });
      };

      setup();

      return () => {
          if (unlistenToken) unlistenToken();
          if (unlistenStatus) unlistenStatus();
          if (unlistenTool) unlistenTool();
      };
  }, [sessionId]);

  const stopLoop = () => {
      // Backend doesn't support explicit stop yet (status atomic bool).
      // We could add a command `stop_agent`.
      // For now, just set state.
      setIsLooping(false);
  };

  return {
    messages,
    isLooping,
    startLoop,
    stopLoop
  };
}
