import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // public/assets holds the taste-skill kit, so the bundle goes to its own folder
  build: { assetsDir: 'app' },
});
