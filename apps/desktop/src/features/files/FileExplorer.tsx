import { useState, useEffect } from "react";
import { commands, FileEntry } from "../../bindings";

// Recursive Tree Node Component
function FileNode({ entry, onSelect, depth }: { entry: FileEntry; onSelect: (entry: FileEntry) => void; depth: number }) {
  const [expanded, setExpanded] = useState(false);

  const toggle = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (entry.is_dir) {
      setExpanded(!expanded);
    } else {
      onSelect(entry);
    }
  };

  return (
    <div style={{ paddingLeft: `${depth * 10}px` }}>
      <div
        onClick={toggle}
        style={{
          cursor: "pointer",
          padding: "2px",
          color: entry.is_dir ? "#e0e0e0" : "#a0a0a0",
          fontWeight: entry.is_dir ? "bold" : "normal",
          display: "flex",
          alignItems: "center"
        }}
      >
        <span style={{ marginRight: "5px", width: "15px", display: "inline-block", textAlign: "center" }}>
            {entry.is_dir ? (expanded ? "▼" : "▶") : "•"}
        </span>
        {entry.name}
      </div>
      {expanded && entry.children && (
        <div>
          {entry.children.map((child) => (
            <FileNode key={child.path} entry={child} onSelect={onSelect} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

export function FileExplorer() {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [selectedFile, setSelectedFile] = useState<FileEntry | null>(null);
  const [content, setContent] = useState("");
  const [status, setStatus] = useState("");

  useEffect(() => {
    loadFiles();
  }, []);

  async function loadFiles() {
    const res = await commands.listFiles(null);
    if (res.status === "ok") {
      setFiles(res.data);
    } else {
      setStatus(`Error loading files: ${JSON.stringify(res.error)}`);
    }
  }

  async function handleSelect(entry: FileEntry) {
    if (entry.is_dir) return;
    setSelectedFile(entry);
    setStatus("Loading...");
    const res = await commands.readFile(entry.path);
    if (res.status === "ok") {
      setContent(res.data.content);
      setStatus(`Loaded ${entry.name}`);
    } else {
      setStatus(`Error reading file: ${JSON.stringify(res.error)}`);
    }
  }

  async function handleSave() {
    if (!selectedFile) return;
    setStatus("Saving...");
    const res = await commands.writeFile(selectedFile.path, content);
    if (res.status === "ok") {
      setStatus("Saved!");
    } else {
      setStatus(`Error saving: ${JSON.stringify(res.error)}`);
    }
  }

  return (
    <div style={{
        width: "100%",
        display: "flex",
        height: "600px",
        border: "1px solid #444",
        background: "#1e1e1e",
        color: "#ddd",
        textAlign: "left",
        marginTop: "20px",
        borderRadius: "8px",
        overflow: "hidden"
    }}>
      {/* Left Pane: Tree */}
      <div style={{ width: "30%", borderRight: "1px solid #444", overflowY: "auto", padding: "10px", background: "#252526" }}>
        <h3 style={{ marginTop: 0, fontSize: "1em", color: "#bbb" }}>Workspace</h3>
        {files.length === 0 && <p style={{fontSize: "0.8em", color: "#666"}}>Loading files...</p>}
        {files.map((file) => (
          <FileNode key={file.path} entry={file} onSelect={handleSelect} depth={0} />
        ))}
      </div>

      {/* Right Pane: Editor */}
      <div style={{ width: "70%", display: "flex", flexDirection: "column", padding: "0" }}>
        <div style={{
            padding: "10px",
            background: "#333",
            borderBottom: "1px solid #444",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center"
        }}>
          <span style={{ fontSize: "0.9em", fontFamily: "monospace" }}>
              {selectedFile ? selectedFile.path : "Select a file to edit"}
          </span>
          <button
            onClick={handleSave}
            disabled={!selectedFile}
            style={{
                padding: "5px 15px",
                background: selectedFile ? "#007acc" : "#555",
                color: "white",
                border: "none",
                borderRadius: "4px",
                cursor: selectedFile ? "pointer" : "default",
                opacity: selectedFile ? 1 : 0.5
            }}
          >
            Save
          </button>
        </div>
        <textarea
          style={{
            flex: 1,
            background: "#1e1e1e",
            color: "#d4d4d4",
            border: "none",
            padding: "15px",
            fontFamily: "monospace",
            fontSize: "14px",
            resize: "none",
            outline: "none"
          }}
          value={content}
          onChange={(e) => setContent(e.target.value)}
          disabled={!selectedFile}
          spellCheck={false}
        />
        <div style={{ padding: "5px 10px", fontSize: "0.8em", color: "#888", background: "#252526", borderTop: "1px solid #333" }}>
            {status || "Ready"}
        </div>
      </div>
    </div>
  );
}
