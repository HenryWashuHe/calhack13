// Eric's Claude: dev-only local preview (docs/invention-lab/ui.md#preview).
// The operator picks retained record files; nothing is fetched, written or sent.

import { StrictMode, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import { intake, orgsIn, type Intake } from "./intake";
import { buildLineage } from "./lineage";
import { LineagePanel } from "./LineagePanel";

export function Preview({ initial }: { initial?: Intake }) {
  const [loaded, setLoaded] = useState<Intake | undefined>(initial);
  const [orgA, setOrgA] = useState("");
  const [orgB, setOrgB] = useState("");
  const lineage = useMemo(() => (loaded ? buildLineage(loaded.records) : undefined), [loaded]);
  const orgs = loaded ? orgsIn(loaded) : [];

  async function choose(list: FileList | null) {
    const files = await Promise.all([...(list ?? [])].map(async (f) => ({ name: f.webkitRelativePath || f.name, text: await f.text() })));
    setLoaded(intake(files));
  }

  return (
    <main style={{ padding: 16, maxWidth: 960 }}>
      <h2>Invention-lab evidence (local preview)</h2>
      <p>Choose retained record files or a whole evidence folder. Files are read in this browser only.</p>
      <input type="file" multiple onChange={(e) => void choose(e.currentTarget.files)} aria-label="record files" />{" "}
      <input type="file" {...{ webkitdirectory: "" }} onChange={(e) => void choose(e.currentTarget.files)} aria-label="evidence folder" />
      {loaded && lineage && (
        <>
          <p>
            A:{" "}
            <select value={orgA} onChange={(e) => setOrgA(e.currentTarget.value)} aria-label="organization A">
              <option value="">—</option>
              {orgs.map((o) => <option key={o}>{o}</option>)}
            </select>{" "}
            B:{" "}
            <select value={orgB} onChange={(e) => setOrgB(e.currentTarget.value)} aria-label="organization B">
              <option value="">—</option>
              {orgs.map((o) => <option key={o}>{o}</option>)}
            </select>
          </p>
          <details>
            <summary>
              {loaded.used.length} file(s) used, {loaded.skipped.length} not used
            </summary>
            <ul>
              {[...loaded.used.map((n) => ({ ...n, tag: "used" })), ...loaded.skipped.map((n) => ({ ...n, tag: "not used" }))].map((n) => (
                <li key={n.file}>
                  {n.tag}: <code>{n.file}</code> — {n.note}
                </li>
              ))}
            </ul>
          </details>
          <LineagePanel lineage={lineage} orgA={orgA || undefined} orgB={orgB || undefined} />
        </>
      )}
    </main>
  );
}

const root = typeof document !== "undefined" ? document.getElementById("root") : null;
if (root) createRoot(root).render(<StrictMode><Preview /></StrictMode>);
