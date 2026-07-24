import { defineConfig } from "vite";

// Config Vite pensée pour Tauri : port fixe, pas d'ouverture de navigateur.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Ne pas surveiller le dossier Rust (recompilé par Tauri lui-même).
      ignored: ["**/src-tauri/**"],
    },
  },
});
