import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwind from '@tailwindcss/vite';
export default defineConfig({
  plugins: [react(), tailwind()],
  server: {
    host: '127.0.0.1', port: 5173, strictPort: true,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:4877', changeOrigin: true,
        configure(proxy) {
          proxy.on('proxyReq', (outgoing, request) => {
            if (request.headers.origin === 'http://127.0.0.1:5173') {
              outgoing.setHeader('Origin', 'http://127.0.0.1:4877');
            }
          });
        },
      },
    },
  },
  build: { chunkSizeWarningLimit: 1000 },
});
