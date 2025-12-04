import { commands, Result, UserProfile } from "../../bindings";
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";

export function ProfileForm() {
  const [name, setName] = useState("");
  const [bio, setBio] = useState("");
  const [result, setResult] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (data: { name: string; bio: string }) =>
      commands.updateProfile(data),
    onSuccess: (data: Result<UserProfile, string>) => {
        if (data.status === "ok") {
            setResult(`Updated: ${data.data.name} (${data.data.bio})`);
        } else {
            setResult(`Error: ${data.error}`);
        }
    },
    onError: (err) => {
        setResult(`Failed: ${err}`);
    }
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    mutation.mutate({ name, bio });
  };

  return (
    <div style={{ padding: "20px", border: "1px solid #ccc" }}>
      <h2>Update Profile</h2>
      <form onSubmit={handleSubmit}>
        <div>
          <label>Name: </label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Name"
          />
        </div>
        <div>
          <label>Bio: </label>
          <input
            value={bio}
            onChange={(e) => setBio(e.target.value)}
            placeholder="Bio"
          />
        </div>
        <button type="submit" disabled={mutation.isPending}>
          {mutation.isPending ? "Saving..." : "Save"}
        </button>
      </form>
      {result && <p>{result}</p>}
    </div>
  );
}
