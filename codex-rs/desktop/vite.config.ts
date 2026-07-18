import react from "@vitejs/plugin-react";
import { createLogger, defineConfig } from "vite";

const logger = createLogger();
const logWarning = logger.warn.bind(logger);
logger.warn = (message, options) => {
  if (
    process.env.CI &&
    /(?:css|lightningcss|stylesheet|selector|media query)/iu.test(message)
  ) {
    throw new Error(`CSS warning treated as an error: ${message}`);
  }
  logWarning(message, options);
};

export default defineConfig({
  customLogger: logger,
  plugins: [react()],
  resolve: {
    dedupe: ["react", "react-dom"],
  },
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
