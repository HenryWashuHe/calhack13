// Eric's Claude: the build-time module export-board.mjs provides.
declare module "virtual:lab-evidence" {
  const files: { name: string; text: string }[];
  export default files;
  export const orgA: string;
  export const orgB: string;
}
