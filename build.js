// Build script for TauriTavern
// This script builds the frontend and starts the Tauri application

import { spawn } from 'child_process';
import { fileURLToPath } from 'url';
import path from 'path';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Run webpack to build the frontend
async function runWebpack() {
    return new Promise((resolve, reject) => {
        console.log('Running webpack...');

        const webpack = spawn('pnpm', ['exec', 'webpack', '--config', 'webpack.config.js'], {
            cwd: __dirname,
            shell: process.platform === 'win32',
            stdio: 'pipe'
        });

        webpack.stdout.on('data', (data) => {
            console.log(data.toString());
        });

        webpack.stderr.on('data', (data) => {
            console.error(data.toString());
        });

        webpack.on('close', (code) => {
            if (code === 0) {
                console.log('Webpack build completed successfully');
                resolve();
            } else {
                console.error(`Webpack build failed with code ${code}`);
                reject(new Error(`Webpack build failed with code ${code}`));
            }
        });
    });
}

// Start the Tauri application
async function startTauri(dev = true) {
    return new Promise((resolve, reject) => {
        console.log(`Starting Tauri in ${dev ? 'development' : 'production'} mode...`);

        const tauri = spawn('pnpm', ['exec', 'tauri', dev ? 'dev' : 'build'], {
            cwd: __dirname,
            shell: process.platform === 'win32',
            stdio: 'pipe'
        });

        tauri.stdout.on('data', (data) => {
            console.log(data.toString());
        });

        tauri.stderr.on('data', (data) => {
            console.error(data.toString());
        });

        tauri.on('close', (code) => {
            if (code === 0) {
                console.log('Tauri completed successfully');
                resolve();
            } else {
                console.error(`Tauri failed with code ${code}`);
                reject(new Error(`Tauri failed with code ${code}`));
            }
        });
    });
}

// Main function
async function main() {
    try {
        // Get command line arguments
        const args = process.argv.slice(2);
        const buildOnly = args.includes('--build-only');
        const prod = args.includes('--prod');

        // Build library bundle directly to src/dist
        await runWebpack();

        // Start Tauri if not build-only
        if (!buildOnly) {
            await startTauri(!prod);
        }
    } catch (error) {
        console.error('Build failed:', error);
        process.exit(1);
    }
}

// Run the main function
main();
