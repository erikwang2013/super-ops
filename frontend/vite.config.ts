import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 3000,
    proxy: {
      '/api': 'http://localhost:8080',
      '/ws': { target: 'ws://localhost:8080', ws: true },
    },
  },
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    // antd v5 单依赖 minified ≈786 kB，原子 vendor 块无法再拆，800 仅作真实回归护栏
    chunkSizeWarningLimit: 800,
    rollupOptions: {
      output: {
        manualChunks(id: string) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('@ant-design/pro-')) return 'vendor-pro';
          if (id.includes('antd') || id.includes('@ant-design/icons')) return 'vendor-antd';
          if (id.includes('/rc-')) return 'vendor-rc';
          if (id.includes('echarts') || id.includes('zrender')) return 'vendor-echarts';
          if (id.includes('xterm')) return 'vendor-terminal';
          if (id.includes('/react') || id.includes('react-router')) return 'vendor-react';
          if (id.includes('dayjs') || id.includes('@babel/runtime')) return 'vendor-util';
          return 'vendor';
        },
      },
    },
  },
});
