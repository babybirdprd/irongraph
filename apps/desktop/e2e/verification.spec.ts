import { test } from '@playwright/test';
import { execSync } from 'child_process';
import path from 'path';
import fs from 'fs';

test.describe('IronGraph Real Verification', () => {
  test('Run Real App Interaction', async () => {
    console.log("Running real app verification script...");
    const scriptPath = path.resolve('e2e/run_real.sh');
    const cwd = path.dirname(scriptPath);

    try {
        execSync(`bash run_real.sh`, { stdio: 'inherit', cwd: cwd });
    } catch (e) {
        console.error("Script failed:", e);
        throw e;
    }

    // Verify screenshot exists
    const screenshotPath = path.resolve('screenshots/real_app_interaction.png');
    if (!fs.existsSync(screenshotPath)) {
        throw new Error(`Screenshot was not generated at ${screenshotPath}`);
    }
    console.log("Screenshot generated successfully.");
  });
});
