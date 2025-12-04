import { useState } from "react";
import { commands } from "../../bindings";

export function CommandRunner() {
  const [program, setProgram] = useState("");
  const [args, setArgs] = useState("");
  const [output, setOutput] = useState<{ stdout: string; stderr: string; exit_code: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleRun() {
    if (!program) return;
    setLoading(true);
    setOutput(null);
    setError(null);

    // Split args by space, filtering empty strings
    const argsList = args.trim().length > 0 ? args.trim().split(/\s+/) : [];

    try {
      const res = await commands.runCommand(program, argsList);
      if (res.status === "ok") {
        setOutput(res.data);
      } else {
        setError(`Error: ${JSON.stringify(res.error)}`);
      }
    } catch (e) {
      setError(`Exception: ${e}`);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div style={{
        backgroundColor: "#1e1e1e",
        padding: "20px",
        borderRadius: "8px",
        color: "#fff",
        fontFamily: "monospace",
        marginTop: "20px"
    }}>
      <h3 style={{ marginTop: 0 }}>Terminal Manager</h3>
      <div style={{ display: "flex", gap: "10px", marginBottom: "15px" }}>
        <input
            style={{
                width: "20%",
                padding: "8px",
                background: "#333",
                border: "1px solid #555",
                color: "white"
            }}
            value={program}
            onChange={(e) => setProgram(e.target.value)}
            placeholder="Program (e.g. ls)"
        />
        <input
            style={{
                width: "60%",
                padding: "8px",
                background: "#333",
                border: "1px solid #555",
                color: "white"
            }}
            value={args}
            onChange={(e) => setArgs(e.target.value)}
            placeholder="Args (e.g. -la)"
        />
        <button
            onClick={handleRun}
            disabled={loading}
            style={{
                width: "20%",
                padding: "8px",
                background: loading ? "#555" : "#4CAF50",
                color: "white",
                border: "none",
                cursor: loading ? "not-allowed" : "pointer"
            }}
        >
            {loading ? "Running..." : "Run"}
        </button>
      </div>

      <div style={{
          background: "#000",
          padding: "10px",
          borderRadius: "4px",
          height: "200px",
          overflowY: "auto",
          border: "1px solid #333",
          fontSize: "0.9em"
      }}>
        {error && <div style={{ color: "red" }}>{error}</div>}
        {output && (
            <>
                {output.stdout && <div style={{ color: "#0f0", whiteSpace: "pre-wrap" }}>{output.stdout}</div>}
                {output.stderr && <div style={{ color: "#f55", whiteSpace: "pre-wrap" }}>{output.stderr}</div>}
                <div style={{ color: "#888", marginTop: "10px" }}>Exit Code: {output.exit_code}</div>
            </>
        )}
        {!output && !error && <div style={{ color: "#444" }}>Ready to execute.</div>}
      </div>
    </div>
  );
}
