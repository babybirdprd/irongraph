import { useState, useRef } from "react";
import { commands, Message, ToolCall, LLMConfig } from "../bindings";
import { SYSTEM_PROMPT } from "../lib/systemPrompt";

const DEFAULT_CONFIG: LLMConfig = {
  api_key: "sk-dummy",
  base_url: "mock",
  model: "gpt-4o",
  temperature: 0,
};

// Helper: Parse shell arguments respecting quotes
function parseCommandArgs(input: string): string[] {
  // Regex matches:
  // 1. Quoted string (double quotes)
  // 2. Quoted string (single quotes)
  // 3. Non-whitespace sequence
  const matchRegex = /"([^"]*)"|'([^']*)'|(\S+)/g;
  const matches = [];
  let match;

  while ((match = matchRegex.exec(input)) !== null) {
    // match[1] is double quoted content
    // match[2] is single quoted content
    // match[3] is unquoted content
    matches.push(match[1] ?? match[2] ?? match[3] ?? "");
  }

  return matches;
}

export function useAgentLoop() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [isLooping, setIsLooping] = useState(false);
  const isLoopingRef = useRef(false);

  const stopLoop = () => {
    isLoopingRef.current = false;
    setIsLooping(false);
  };

  const startLoop = async (userPrompt: string) => {
    if (isLoopingRef.current) return;

    isLoopingRef.current = true;
    setIsLooping(true);

    // Initial history with System Prompt and User Prompt
    // Note: Some LLM APIs expect System prompt as the first message with role "system".
    // bindings.ts Message role is string.
    let currentMessages: Message[] = [
      ...messages, // Keep previous context if any? Or reset?
      // If we want a fresh session per startLoop call for simplicity, we might reset.
      // But typically a chat interface appends.
      // However, we need to inject SYSTEM_PROMPT.
      // If messages is empty, inject system prompt.
    ];

    if (messages.length === 0) {
      currentMessages.push({ role: "system", content: SYSTEM_PROMPT });
    }

    currentMessages.push({ role: "user", content: userPrompt });
    setMessages([...currentMessages]);

    try {
      while (isLoopingRef.current) {
        // 1. Send Chat Request
        const res = await commands.sendChat({
          messages: currentMessages,
          config: DEFAULT_CONFIG,
        });

        if (res.status === "error") {
            // Append error message as system/assistant info and break?
            const errorMsg = `API Error: ${JSON.stringify(res.error)}`;
            currentMessages.push({ role: "system", content: errorMsg });
            setMessages([...currentMessages]);
            break;
        }

        const responseData = res.data;

        // Append Assistant Response
        currentMessages.push({
            role: responseData.role,
            content: responseData.content // The text content (Thought)
        });
        setMessages([...currentMessages]);

        // 2. Check Tool Calls
        if (!responseData.tool_calls || responseData.tool_calls.length === 0) {
            // No tools -> Agent is done or waiting for input
            break;
        }

        // 3. Execute Tools
        for (const tool of responseData.tool_calls) {
            if (!isLoopingRef.current) break;

            let output = "";
            let isError = false;

            try {
                switch (tool.name) {
                    case "run_command": {
                        const program = tool.arguments["program"];
                        const argsStr = tool.arguments["args"] || "";
                        if (!program) throw new Error("Missing 'program' argument");

                        const args = parseCommandArgs(argsStr);
                        const cmdRes = await commands.runCommand(program, args);

                        if (cmdRes.status === "ok") {
                            output = cmdRes.data.stdout + cmdRes.data.stderr;
                            // Check exit code?
                            if (cmdRes.data.exit_code !== 0) {
                                output += `\n(Exit Code: ${cmdRes.data.exit_code})`;
                            }
                        } else {
                            throw new Error(JSON.stringify(cmdRes.error));
                        }
                        break;
                    }
                    case "list_files": {
                        const dirPath = tool.arguments["dir_path"] || null; // Map empty/undefined to null
                        // If empty string specifically, map to null as well
                        const effectiveDirPath = dirPath === "" ? null : dirPath;

                        const lsRes = await commands.listFiles(effectiveDirPath);
                        if (lsRes.status === "ok") {
                             // Format file entries
                             output = lsRes.data.map(f => `${f.is_dir ? '[DIR] ' : ''}${f.name}`).join("\n");
                        } else {
                             throw new Error(JSON.stringify(lsRes.error));
                        }
                        break;
                    }
                    case "read_file": {
                        const filePath = tool.arguments["file_path"];
                        if (!filePath) throw new Error("Missing 'file_path' argument");

                        const readRes = await commands.readFile(filePath);
                        if (readRes.status === "ok") {
                            output = readRes.data.content;
                        } else {
                            throw new Error(JSON.stringify(readRes.error));
                        }
                        break;
                    }
                    case "write_file": {
                         const filePath = tool.arguments["file_path"];
                         const content = tool.arguments["content"];
                         if (!filePath) throw new Error("Missing 'file_path' argument");
                         // content can be empty string, check for undefined
                         if (content === undefined) throw new Error("Missing 'content' argument");

                         const writeRes = await commands.writeFile(filePath, content);
                         if (writeRes.status === "ok") {
                             output = `Successfully wrote to ${filePath}`;
                         } else {
                             throw new Error(JSON.stringify(writeRes.error));
                         }
                         break;
                    }
                    default:
                        throw new Error(`Unknown tool: ${tool.name}`);
                }
            } catch (e: any) {
                isError = true;
                output = e.message || String(e);
            }

            // 4. Format Output
            const formattedOutput = isError
                ? `Tool Error [${tool.name}]:\n${output}`
                : `Tool Output [${tool.name}]:\n${output}`;

            // 5. Append Result to History
            currentMessages.push({
                role: "user", // Convention: Tool outputs come from "user" role or strictly "tool" role if API supports it.
                              // OpenAI supports "tool" role but we are using "user" as per instructions ("Convention for feeding back tool outputs").
                content: formattedOutput
            });
            setMessages([...currentMessages]);
        }
        // Loop continues with updated history
      }
    } catch (error) {
        console.error("Agent Loop Error:", error);
        currentMessages.push({ role: "system", content: `Agent Loop Crashed: ${error}` });
        setMessages([...currentMessages]);
    } finally {
        isLoopingRef.current = false;
        setIsLooping(false);
    }
  };

  return {
    messages,
    isLooping,
    startLoop,
    stopLoop
  };
}
