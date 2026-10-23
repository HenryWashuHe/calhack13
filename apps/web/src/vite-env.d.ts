/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_GENESIS_ORGS?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
