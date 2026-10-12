import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
export default defineConfig({ plugins: [react()], clearScreen: false, server: { strictPort: true }, build: { target: 'es2022' }, test: { include: ['src/**/*.test.{ts,tsx}'] } });
