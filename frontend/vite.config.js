import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// MEEV build settings:
//  * code-splitting: vendors + route pages split into many small chunks
//    for faster page loading (the user requirement: dozens of small files).
//  * assets are never inlined so they stay cacheable separately.
export default defineConfig({
  plugins: [react()],
  server: {
    host: '0.0.0.0',
    port: 5173,
    proxy: {
      '/api': { target: 'http://localhost:8080', changeOrigin: true },
      '/ws': { target: 'ws://localhost:8080', ws: true },
      '/uploads': { target: 'http://localhost:8080', changeOrigin: true },
    },
  },
  preview: {
    host: '0.0.0.0',
    port: 4173,
  },
  build: {
    outDir: 'dist',
    assetsDir: 'assets',
    assetsInlineLimit: 0,
    cssCodeSplit: true,
    sourcemap: false,
    minify: 'esbuild',
    target: 'es2020',
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('react-router')) return 'chunk-router';
          if (id.includes('react-dom')) return 'chunk-react-dom';
          if (id.includes('/react/')) return 'chunk-react';
          if (id.includes('scheduler')) return 'chunk-scheduler';
          return 'chunk-vendor';
        },
        chunkFileNames: 'assets/chunk-[name]-[hash:8].js',
        entryFileNames: 'assets/entry-[name]-[hash:8].js',
        assetFileNames: 'assets/[name]-[hash:8][extname]',
      },
    },
  },
});
