import path from "node:path";
import { fileURLToPath } from "node:url";
import { preview } from "vite";

const desktopRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);

export default async function globalSetup() {
  const previewServer = await preview({
    root: desktopRoot,
    configLoader: "runner",
    mode: "preview",
    preview: {
      host: "127.0.0.1",
      port: 1420,
      strictPort: true,
    },
  });

  return async () => {
    const closing = previewServer.close();
    previewServer.httpServer.closeAllConnections?.();
    await closing;
  };
}
