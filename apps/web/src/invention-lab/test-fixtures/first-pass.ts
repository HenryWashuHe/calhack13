// Test-only archive selection. These were the first-pass folders before #215;
// later rehearsal archives must not silently change historical test inputs.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../../../../docs/invention-lab/evidence/", import.meta.url));
export const FIRST_PASS_FOLDERS = [
  "live-method-20261024",
  "live-method-20261024-a-return",
  "live-method-20261024-b-revision",
  "live-method-20261024-b-revision-receiver",
  "live-method-20261024-diagnostic",
  "live-method-20261024-generated",
  "live-method-20261024-receiver",
  "selective-transfer-20261024",
] as const;

export interface EvidenceFile { name: string; text: string; bytes: Uint8Array }

/** Read only the explicitly selected retained folders; never execute their contents. */
function readEvidenceFolders(folders: readonly string[]): EvidenceFile[] {
  const out: EvidenceFile[] = [];
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (!/\.(md|txt)$|SHA256SUMS$/.test(e.name)) {
        const bytes = new Uint8Array(fs.readFileSync(p));
        out.push({ name: path.relative(root, p), text: new TextDecoder().decode(bytes), bytes });
      }
    }
  };
  for (const folder of folders) walk(path.join(root, folder));
  return out;
}

export const firstPassEvidenceFiles = (): EvidenceFile[] => readEvidenceFolders(FIRST_PASS_FOLDERS);
