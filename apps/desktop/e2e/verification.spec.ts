import { test, expect } from '@playwright/test';
import { _electron as electron } from 'playwright';
import path from 'path';

test.describe('IronGraph Agent Verification', () => {
  let app: any;
  let page: any;
  let window: any;

  test.beforeAll(async () => {
    // Launch the application
    // If running against a built binary (common for Tauri E2E), set TAURI_APP_PATH
    // Otherwise, this expects the user to have configured the environment or built the app.
    // For local dev without a build, one might use the Vite dev server URL directly if mocking IPC.

    // Example: Launching as an Electron app (if using a shim) or connecting to a debugging port.
    // Note: Tauri != Electron, but Playwright is often used to drive the WebView.
    // This setup assumes the user provides the executable path via env var.

    const executablePath = process.env.TAURI_APP_PATH;

    if (executablePath) {
        app = await electron.launch({ executablePath });
        window = await app.firstWindow();
        page = window;
    } else {
        // Fallback: Assume web-only test against dev server (requires running 'pnpm tauri dev' separately)
        // This is useful for testing the UI flow if the backend is mocked or reachable.
        const browser = await electron.launch(); // This might fail if no exe, so strictly we need a browser type
        // Actually, let's use standard browser launch if no exe provided
        // But we need to switch imports. For now, let's fail gracefully or assume user sets path.
        console.warn("No TAURI_APP_PATH provided. Attempting to connect to localhost:1420...");
        /*
           To run this against the dev server:
           1. pnpm tauri dev
           2. TAURI_APP_PATH=... pnpm test:e2e
        */
        throw new Error("Please set TAURI_APP_PATH to the built executable path to run E2E tests.");
    }
  });

  test.afterAll(async () => {
      if (app) {
          await app.close();
      }
  });

  test('Smoke Test Protocol', async () => {
     // 1. Compilation was checked by Agent.

     // 2. Configuration Check (Implicit if app launches)

     // 3. Hello World Stream
     // Mocking user input
     await page.fill('[data-testid="chat-input"]', 'Hello');
     await page.click('[data-testid="send-button"]');

     // Verify streaming - we expect "Running" then "Waiting"
     await expect(page.locator('[data-testid="status-bar"]')).toContainText('Running');
     await page.screenshot({ path: 'screenshots/step3a_running.png' });
     await expect(page.locator('[data-testid="status-bar"]')).toContainText('Waiting');

     // Verify response text exists
     const messages = page.locator('[data-testid="chat-message"]');
     await expect(messages.last()).toContainText(/Hello|Hi|Greetings/);
     await page.screenshot({ path: 'screenshots/step3b_hello_response.png' });

     // 4. Tool Execution
     await page.fill('[data-testid="chat-input"]', 'List the files in the current directory.');
     await page.click('[data-testid="send-button"]');

     await expect(page.locator('[data-testid="status-bar"]')).toContainText('Running');
     // Wait for tool output
     await expect(page.locator('[data-testid="tool-output"]')).toBeVisible();
     await page.screenshot({ path: 'screenshots/step4_tool_output.png' });
     await expect(page.locator('[data-testid="status-bar"]')).toContainText('Waiting');

     // 5. Persistence
     await page.reload();
     // Check if history remains
     const count = await messages.count();
     expect(count).toBeGreaterThan(2); // Initial hello + tool request
     await page.screenshot({ path: 'screenshots/step5_persistence.png' });
  });
});
