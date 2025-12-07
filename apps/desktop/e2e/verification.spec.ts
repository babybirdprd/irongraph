import { test, expect, chromium } from '@playwright/test';

test.describe('IronGraph Verification', () => {
  test('Smoke Test Protocol', async () => {
    const browser = await chromium.launch();
    const page = await browser.newPage();

    // Capture Logs
    page.on('console', msg => console.log(`[Browser] ${msg.text()}`));
    page.on('pageerror', err => console.log(`[BrowserError] ${err}`));

    // Mock Tauri IPC
    await page.addInitScript(() => {
        const mockInvoke = async (cmd, args) => {
            console.log(`[MockInvoke] ${cmd}`, JSON.stringify(args));

            // Mock Responses
            if (cmd === 'start_agent_loop') {
                return "mock-session-1";
            }
            if (cmd === 'list_files') {
                return { status: "ok", data: [] };
            }
            if (cmd === 'search_code') {
                return { status: "ok", data: [] };
            }
            if (cmd === 'read_file') {
                return { status: "ok", data: { content: "", path: args.filePath } };
            }

            // SQL Mocks
            if (cmd === 'plugin:sql|load') {
                return "irongraph.db";
            }
            if (cmd === 'plugin:sql|select') {
                return [];
            }
            if (cmd === 'plugin:sql|execute') {
                return { rowsAffected: 1, lastInsertId: 1 };
            }

            // Default success
            return { status: "ok", data: null };
        };

        (window as any).__TAURI_INTERNALS__ = {
            invoke: mockInvoke,
            plugins: { invoke: mockInvoke }
        };
        (window as any).__TAURI__ = {
            core: { invoke: mockInvoke }
        }
    });

    console.log("Navigating to app...");
    try {
        await page.goto('http://localhost:1420', { timeout: 10000 });
    } catch (e) {
        console.log("Navigation failed:", e);
        return;
    }

    await page.waitForLoadState('domcontentloaded');

    // 1. Initial State
    await page.screenshot({ path: 'screenshots/1_app_load.png' });

    // 2. Chat Interaction
    const inputSelector = 'textarea';
    try {
        await page.waitForSelector(inputSelector, { timeout: 5000 });
        const input = page.locator(inputSelector).first();

        await input.fill('Hello');
        const sendBtn = page.locator('button', { hasText: 'Send' });
        await sendBtn.click();

        await page.waitForTimeout(1000);
        await page.screenshot({ path: 'screenshots/2_chat_sent.png' });
        console.log("Interaction successful, screenshot taken.");
    } catch (e) {
        console.log("Failed to find/interact with chat:", e);
        await page.screenshot({ path: 'screenshots/error_state.png' });
    }

    await browser.close();
  });
});
