import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  build: {
    chunkSizeWarningLimit: 1500,
    rollupOptions: {
      output: {
        manualChunks: {
          'lucide-react': ['lucide-react'],
          'tauri-api': ['@tauri-apps/api/core', '@tauri-apps/api/event'],
          'monaco-editor': ['monaco-editor'],
          'stores': [
            './src/stores/fileStore.ts',
            './src/stores/hardwareStore.ts',
            './src/stores/projectStore.ts',
            './src/stores/chatStore.ts',
            './src/stores/settingsStore.ts',
          ],
        },
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  optimizeDeps: {
    include: ['monaco-editor'],
  },
});