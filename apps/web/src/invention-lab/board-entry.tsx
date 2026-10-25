// Eric's Claude: entry for the single-file board built by export-board.mjs.
// The committed evidence is embedded at build time; nothing is fetched.
import { createRoot } from "react-dom/client";
import "../styles.css";
import files, { orgA, orgB } from "virtual:lab-evidence";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import { ResultsBoard } from "./ResultsBoard";

const lineage = buildLineage(intake(files).records);
const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <main style={{ padding: 16, maxWidth: 1180, margin: "0 auto" }}>
      <ResultsBoard lineage={lineage} orgA={orgA} orgB={orgB} />
    </main>,
  );
